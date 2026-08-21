use super::*;


pub(super) fn workflow_command(
    store: &HarnessStore,
    project_context: Option<&ProjectContext>,
    args: &[String],
) -> CliResult<()> {
    require_subcommand(
        args,
        "workflow run|run-script|get-output|patch|list|reap|reap-workers|gc-worktrees",
    )?;
    match args[0].as_str() {
        "patch" => {
            let result = workflow_patch_command(store, project_context, &args[1..])?;
            print_json(&result)?;
        }
        "gc-worktrees" => {
            let result = workflow_gc_worktrees(store, project_context)?;
            print_json(&result)?;
        }
        "reap-workers" => {
            let dry_run = args[1..].iter().any(|a| a == "--dry-run");
            let result = reap_orphaned_workers(store, dry_run)?;
            print_json(&result)?;
        }
        "reap" => {
            // One manual reaper pass (the serve loop runs this on an interval).
            // Useful to clean up abandoned `Running` runs when serve is not up.
            let reaped = reap_stale_workflow_runs(store)?;
            print_json(&serde_json::json!({ "reaped": reaped }))?;
        }
        "list" => {
            let registry = workflow::WorkflowRegistry::builtin();
            let defs: Vec<_> = registry
                .names()
                .into_iter()
                .filter_map(|name| registry.get(name))
                .map(|def| serde_json::json!({ "name": def.name, "summary": def.summary }))
                .collect();
            print_json(&serde_json::json!({ "workflows": defs }))?;
        }
        "run" => {
            let result = workflow_run_value(store, project_context, &args[1..])?;
            print_json(&result)?;
        }
        "get-output" => {
            let result = workflow_get_output_value(store, &args[1..])?;
            if has_flag(&args[1..], "--text") {
                // Plain-text mode: print just the deliverable(s), so a text-producing
                // workflow's output pipes straight to a file (issue #89 item 4).
                if let Some(steps) = result["steps"].as_array() {
                    let multi = steps.len() > 1;
                    for (i, s) in steps.iter().enumerate() {
                        if i > 0 {
                            println!("\n---\n");
                        }
                        if multi {
                            println!("## {}\n", s["label"].as_str().unwrap_or(""));
                        }
                        println!("{}", s["output"].as_str().unwrap_or(""));
                    }
                }
            } else {
                print_json(&result)?;
            }
        }
        "run-script" => {
            // Tell the operator WHICH store this run is written to (stderr, so the
            // JSON result on stdout stays clean) — so a serve reading a different
            // `.harness` is caught immediately (issue #89 item 3).
            let store_display = std::fs::canonicalize(store.root())
                .unwrap_or_else(|_| store.root().to_path_buf())
                .display()
                .to_string();
            eprintln!(
                "workflow store: {store_display}  (point `serve` at the same path: --store <path>)"
            );
            let result = workflow_run_script_value(store, project_context, &args[1..])?;
            print_json(&result)?;
        }
        other => {
            return Err(CliError::Usage(format!(
                "unknown workflow command: {other}"
            )))
        }
    }
    Ok(())
}

pub(super) fn workflow_run_value(
    store: &HarnessStore,
    project_context: Option<&ProjectContext>,
    args: &[String],
) -> CliResult<serde_json::Value> {
    let name = value(args, "--name").unwrap_or_else(|| "investigate".to_string());
    let registry = workflow::WorkflowRegistry::builtin();
    let def = registry
        .get(&name)
        .ok_or_else(|| CliError::Usage(format!("unknown workflow: {name}")))?;

    let prompt = value(args, "--prompt").unwrap_or_else(|| "failure X".to_string());
    let options = WorkflowDeliveryOptions {
        dry_run: has_flag(args, "--dry-run"),
        start_runtime: has_flag(args, "--start-runtime"),
        // Per-node ephemeral-worker timeout. Default 5 min: a real codex/claude
        // turn takes ~30-60s, so 3s would kill every worker now that the timeout
        // actually fires during the read (see run_ndjson_child); this is an IDLE
        // limit — a worker is killed only after this long with NO output, so a slow
        // but productive turn is never cut off. Default 15 min of silence.
        timeout_ms: value(args, "--timeout-ms")
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(900_000),
        default_model: value(args, "--model"),
        default_effort: value(args, "--effort"),
        max_budget_usd: None,
        // Registry runs always retain their trace durably.
        progress: has_flag(args, "--progress"),
        project: project_context
            .cloned()
            .unwrap_or_else(|| workflow_project_context(store)),
    };

    // The run id is minted up front so the driver can journal each step's
    // `running` row against it AS THE STEP STARTS (live progress over SSE),
    // rather than only emitting a terminal row after the whole body returns.
    let run_id = generated_id("wfrun");

    // Read the Copy flag before the `move` driver closure consumes `options`.
    let is_dry_run = options.dry_run;
    let project_binding_id = Some(options.project.id.clone());

    // Build the injectable real driver. The store, run id, and options are
    // captured by reference; the closure is Sync (HarnessStore serializes writes
    // via flock) so it can be shared across the parallel barrier's scoped threads.
    let driver = {
        let run_id = run_id.clone();
        move |spec: &workflow::AgentStepSpec| {
            workflow_real_agent_step(store, &run_id, &options, spec)
        }
    };

    run_workflow_with_driver(
        store,
        &run_id,
        def,
        &prompt,
        is_dry_run,
        project_binding_id,
        &driver,
    )
}

/// `harness workflow run-script <prog.star> [--name <n>] [--args <json>]
///  [--dry-run] [--start-runtime] [--timeout-ms <ms>]
///  [--model <m>] [--effort <e>] [--initiated-by <id>]`
///
/// Reads a runtime-authored Starlark program — the SOLE dynamic authoring
/// surface — evaluates it via `starlark_front::run_starlark`, and journals the
/// run/steps through the shared `journal_workflow_outcome`.
///
/// The program MUST declare a `workflow(name, design_intent)` header (the WHY
/// behind its shape); `run_starlark` rejects it otherwise. The captured
/// `design_intent` is persisted on the run, and the raw script text is
/// snapshotted under `spec = {"lang":"starlark","script": <text>}` for
/// reproducibility. `--name` defaults to the declared meta name (else the file
/// stem).
/// Reconstruct a [`workflow::StepResult`] from a stored terminal [`WorkflowStep`]
/// for the `--resume` replay cache. Returns `None` unless the step carries an
/// ordinal in its `result` JSON (steps journaled before the resume feature have no
/// ordinal, so they are simply skipped → re-run, never incorrectly reused).
///
/// The reconstructed result sets `step_id = None` and `started_at = None` so
/// [`journal_workflow_outcome`] mints a FRESH terminal row for the NEW (resumed)
/// run id — replayed leaves journal like normal new steps. `ok = true` because the
/// caller only feeds Completed steps. `provider`/`isolation`/`structured`/`details`
/// are read back out of the same `result` object [`workflow::step_result_json`] wrote.
pub(super) fn step_result_from_stored(step: &WorkflowStep) -> Option<workflow::StepResult> {
    let result = step.result.as_ref()?;
    let ordinal = result.get("ordinal").and_then(|v| v.as_u64())?;
    let provider = result
        .get("provider")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    let isolation = result
        .get("isolation")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let structured = result.get("structured").cloned().filter(|v| !v.is_null());
    // Carry the captured telemetry blob forward (model/tokens/cost/...). The base
    // keys step_result_json re-writes from the reconstructed fields take precedence
    // on the next journal, so passing the whole object back is safe.
    let details = step.result.clone().filter(|v| v.is_object());
    Some(workflow::StepResult {
        phase: step.phase.clone(),
        label: step.label.clone(),
        provider,
        isolation,
        ok: true,
        output_summary: step.output_summary.clone().unwrap_or_default(),
        step_id: None,
        started_at: None,
        details,
        structured,
        ordinal: Some(ordinal),
    })
}

/// Build the `--resume` replay cache: a map from leaf ordinal to the prior run's
/// succeeded [`workflow::StepResult`]. Loads the prior run's latest terminal steps,
/// keeps only Completed steps carrying an ordinal, and reconstructs each. A prior
/// FAILED leaf is naturally absent → it re-runs. On duplicate ordinals (should not
/// happen post-projection) last wins.
pub(super) fn build_replay_map(
    store: &HarnessStore,
    prior_run_id: &str,
) -> CliResult<std::collections::HashMap<u64, workflow::StepResult>> {
    let mut map = std::collections::HashMap::new();
    for step in latest_workflow_steps_in_append_order(store)? {
        if step.run_id != prior_run_id {
            continue;
        }
        if step.status != WorkflowStepStatus::Completed {
            continue;
        }
        if let Some(result) = step_result_from_stored(&step) {
            if let Some(ord) = result.ordinal {
                map.insert(ord, result);
            }
        }
    }
    Ok(map)
}

/// Fire a best-effort completion hook when a [`WorkflowRun`] reaches a terminal
/// status. Configured by the `HARNESS_WORKFLOW_ON_COMPLETE` env var (a shell
/// command); a NO-OP when the var is unset/blank — so existing runs are unaffected.
/// The command runs via `sh -c`, receives `HARNESS_RUN_ID` / `HARNESS_RUN_STATUS`
/// (snake_case, e.g. `completed`/`failed`) / `HARNESS_RUN_NAME` as env vars and the
/// full run JSON on stdin, and runs to completion BEFORE the run-owning process
/// returns — so a backgrounded `run-script &` reliably notifies even though the
/// caller isn't blocked on it. The hook's stdout is DISCARDED (the run-script JSON
/// contract on stdout stays clean); its stderr is inherited for diagnostics. A hook
/// that fails to spawn or exits non-zero is logged to stderr and NEVER fails or
/// alters the run. Keep the hook quick (or self-detach with `&`): the run-owning
/// process waits for it.
pub(super) fn fire_workflow_completion_hook(run: &WorkflowRun) {
    let cmd = match std::env::var("FIRM_WORKFLOW_ON_COMPLETE")
        .or_else(|_| std::env::var("HARNESS_WORKFLOW_ON_COMPLETE"))
    {
        Ok(c) if !c.trim().is_empty() => c,
        _ => return,
    };
    let status = serde_json::to_value(run.status)
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_else(|| format!("{:?}", run.status));
    let run_json = serde_json::to_string(run).unwrap_or_default();
    let spawned = Command::new("sh")
        .arg("-c")
        .arg(&cmd)
        .env("FIRM_RUN_ID", &run.id)
        .env("FIRM_STATUS", &status)
        .env("FIRM_RUN_STATUS", &status)
        .env("FIRM_RUN_NAME", &run.workflow_name)
        .env("HARNESS_RUN_ID", &run.id)
        .env("HARNESS_STATUS", &status)
        .env("HARNESS_RUN_STATUS", &status)
        .env("HARNESS_RUN_NAME", &run.workflow_name)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::inherit())
        .spawn();
    let mut child = match spawned {
        Ok(child) => child,
        Err(error) => {
            eprintln!("workflow on-complete hook failed to spawn: {error}");
            return;
        }
    };
    if let Some(mut stdin) = child.stdin.take() {
        use std::io::Write;
        let _ = stdin.write_all(run_json.as_bytes());
    }
    match child.wait() {
        Ok(exit) if !exit.success() => {
            eprintln!("workflow on-complete hook exited {exit} for run {}", run.id)
        }
        Err(error) => eprintln!("workflow on-complete hook wait failed: {error}"),
        _ => {}
    }
}

pub(super) fn discarded_worktree_diff_warning(run_id: &str, step: &workflow::StepResult) -> Option<String> {
    let details = step.details.as_ref()?;
    let display_diff = details.get("worktree_diff").and_then(|v| v.as_str())?;
    if display_diff.trim().is_empty() {
        return None;
    }
    let diff = details
        .get("landing_diff")
        .and_then(|v| v.as_str())
        .unwrap_or(display_diff);
    let changed_files = count_unique_worktree_diff_files(diff);
    Some(format!(
        "warning: workflow run {run_id} step '{}' produced {changed_files} changed file(s) \
         in a discarded throwaway worktree; retrieve with `harness workflow get-output \
         {run_id} --step {}` or persist it with `harness workflow patch apply`.",
        step.label, step.label
    ))
}

pub(super) fn warn_discarded_worktree_diffs(run_id: &str, outcome: &workflow::WorkflowOutcome) {
    for step in &outcome.steps {
        if let Some(warning) = discarded_worktree_diff_warning(run_id, step) {
            eprintln!("{warning}");
        }
    }
}

/// The changed paths for a step's captured diff, preferring the robustly
/// enumerated `worktree_changed_paths` (D4a: name-status, both rename sides,
/// c-quote/CJK-safe) recorded at capture time, and falling back to parsing the
/// diff text's `diff --git` headers only for OLD runs / mock steps that predate
/// the field. `details` is the step's `result` JSON; `diff` is its landing diff.
pub(super) fn step_changed_paths(details: Option<&serde_json::Value>, diff: &str) -> Vec<String> {
    let stored = details.and_then(|d| d.get("worktree_changed_paths"));
    if let Some(array) = stored.and_then(|v| v.as_array()) {
        return array
            .iter()
            .filter_map(|v| v.as_str())
            .map(str::to_string)
            .collect();
    }
    diff_changed_paths(diff)
}

pub(super) fn diff_changed_paths(diff: &str) -> Vec<String> {
    let mut paths = BTreeSet::new();
    for line in diff.lines() {
        let Some(rest) = line.strip_prefix("diff --git ") else {
            continue;
        };
        let mut parts = rest.split_whitespace();
        let _a = parts.next();
        let Some(b_path) = parts.next() else {
            continue;
        };
        let path = b_path
            .strip_prefix("b/")
            .unwrap_or(b_path)
            .trim_matches('"')
            .to_string();
        if !path.is_empty() && path != "/dev/null" {
            paths.insert(path);
        }
    }
    paths.into_iter().collect()
}

/// Whether a step was DECLARED `writable` (its details record `writable: true`).
/// D3a: a leaf that isolated only because its provider can't enforce read-only
/// (#167 kimi read-only isolation) is NOT writable — its diff must be discarded,
/// never persisted, so this returns false for it and swallows the diff.
pub(super) fn step_is_writable(details: Option<&serde_json::Value>) -> bool {
    details
        .and_then(|d| d.get("writable"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

/// Whether a captured leaf diff should become a durable pending WorkflowPatch
/// Persist only when the step succeeded and was
/// DECLARED writable, and the author did not opt out via `persist_changes:
/// "discard"`. A failed step or a read-only isolated leaf strands nothing.
pub(super) fn should_persist_workflow_patch(
    ok: bool,
    details: Option<&serde_json::Value>,
    diff: &str,
) -> bool {
    if diff.trim().is_empty() {
        return false;
    }
    if !ok || !step_is_writable(details) {
        return false;
    }
    let persist = details
        .and_then(|d| d.get("persist_changes"))
        .and_then(|v| v.as_str())
        .unwrap_or("patch");
    persist != "discard"
}

pub(super) fn string_array(value: Option<&serde_json::Value>) -> Vec<String> {
    value
        .and_then(|v| v.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

pub(super) fn latest_workflow_patches_in_append_order(store: &HarnessStore) -> CliResult<Vec<WorkflowPatch>> {
    let mut latest: BTreeMap<String, WorkflowPatch> = BTreeMap::new();
    for patch in store.workflow_patches()? {
        latest.insert(patch.id.clone(), patch);
    }
    Ok(latest.into_values().collect())
}

pub(super) fn latest_workflow_artifact_manifests_in_append_order(
    store: &HarnessStore,
) -> CliResult<Vec<WorkflowArtifactManifest>> {
    let mut latest: BTreeMap<String, WorkflowArtifactManifest> = BTreeMap::new();
    for manifest in store.workflow_artifact_manifests()? {
        latest.insert(manifest.id.clone(), manifest);
    }
    Ok(latest.into_values().collect())
}

pub(super) fn patch_file_path(store: &HarnessStore, patch: &WorkflowPatch) -> PathBuf {
    let path = PathBuf::from(&patch.patch_ref);
    if path.is_absolute() {
        path
    } else {
        store.root().join(path)
    }
}

pub(super) fn workflow_patch_update(
    store: &HarnessStore,
    patch: &WorkflowPatch,
    status: WorkflowPatchStatus,
    actor: Option<String>,
    reason: Option<String>,
    conflict_detail: Option<String>,
) -> CliResult<WorkflowPatch> {
    let now = now_string();
    let mut updated = patch.clone();
    updated.status = status;
    updated.updated_at = Some(now.clone());
    updated.actor = actor;
    updated.reason = reason;
    updated.conflict_detail = conflict_detail;
    match status {
        WorkflowPatchStatus::Applied => updated.applied_at = Some(now),
        WorkflowPatchStatus::Rejected => updated.rejected_at = Some(now),
        _ => {}
    }
    store.append_workflow_patch(&updated)?;
    Ok(updated)
}

pub(super) fn apply_patch_bytes(repo_root: &Path, bytes: &[u8], check_only: bool) -> CliResult<()> {
    let mut child = Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .args(if check_only {
            vec!["apply", "--check", "--whitespace=nowarn", "-"]
        } else {
            vec!["apply", "--whitespace=nowarn", "-"]
        })
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(bytes)?;
    }
    let out = child.wait_with_output()?;
    if !out.status.success() {
        return Err(CliError::Usage(
            String::from_utf8_lossy(&out.stderr).trim().to_string(),
        ));
    }
    Ok(())
}

/// The set of repo-relative paths that currently have local changes (staged or
/// unstaged) or are untracked, parsed from `git status --porcelain -z`. The `-z`
/// form NUL-delimits records and emits raw (un-c-quoted) UTF-8 paths. Each record
/// is `XY<space>path` with a rename/copy (`R`/`C` in either status column)
/// appending a second NUL-separated original path — both sides are recorded.
pub(super) fn git_dirty_paths(repo_root: &Path) -> CliResult<BTreeSet<String>> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .args(["status", "--porcelain", "-z"])
        .output()?;
    if !output.status.success() {
        return Err(CliError::Usage(format!(
            "git status failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    let mut dirty = BTreeSet::new();
    let mut fields = output
        .stdout
        .split(|b| *b == 0)
        .filter(|f| !f.is_empty())
        .map(|f| String::from_utf8_lossy(f).to_string())
        .peekable();
    while let Some(entry) = fields.next() {
        // `XY path` — the status is the first two bytes, the path starts at byte 3.
        let (xy, path) = if entry.len() > 3 {
            (&entry[..2], entry[3..].to_string())
        } else {
            continue;
        };
        if !path.is_empty() {
            dirty.insert(path);
        }
        // A rename/copy carries the original path as the NEXT NUL record.
        if xy.starts_with('R') || xy.starts_with('C') || xy.ends_with('R') || xy.ends_with('C') {
            if let Some(orig) = fields.next() {
                if !orig.is_empty() {
                    dirty.insert(orig);
                }
            }
        }
    }
    Ok(dirty)
}

pub(super) fn apply_workflow_patch_record(
    store: &HarnessStore,
    project_context: Option<&ProjectContext>,
    patch: &WorkflowPatch,
    actor: Option<String>,
    reason: Option<String>,
    allow_dirty: bool,
) -> CliResult<WorkflowPatch> {
    if patch.status != WorkflowPatchStatus::PendingApply {
        return Err(CliError::Usage(format!(
            "workflow patch {} is {:?}, not pending_apply",
            patch.id, patch.status
        )));
    }
    let project = workflow_project_context_for_run(store, &patch.run_id, project_context)?;
    let repo_root = workflow_repo_root(&project);
    let path = patch_file_path(store, patch);
    let bytes = fs::read(&path)?;
    // D4b: enforce owned_paths against the paths git will ACTUALLY touch, parsed
    // from `git apply --numstat -z` — this reads the patch exactly as git applies
    // it, closing the crafted-`diff --git`-header bypass and the c-quoted-CJK false
    // Conflict. If numstat can't parse the patch, fail closed (a bad patch never
    // applies). We then cross-check against the stored changed_paths and fail
    // closed on disagreement (a numstat path not covered by the recorded set means
    // the patch text and its metadata diverged — refuse rather than trust either).
    let numstat_paths = match git_apply_numstat_paths(&repo_root, &bytes) {
        Ok(paths) => paths,
        Err(error) => {
            let detail = error.to_string();
            let _ = workflow_patch_update(
                store,
                patch,
                WorkflowPatchStatus::Conflict,
                actor,
                reason,
                Some(detail.clone()),
            )?;
            return Err(CliError::Usage(detail));
        }
    };
    let stored_paths: BTreeSet<String> = if patch.changed_paths.is_empty() {
        step_changed_paths(None, &String::from_utf8_lossy(&bytes))
            .into_iter()
            .collect()
    } else {
        patch.changed_paths.iter().cloned().collect()
    };
    // Every path git will touch MUST be covered by the recorded changed_paths
    // (renames record both sides at capture, git apply resolves to the
    // destination, so numstat ⊆ stored). Anything git touches that we did NOT
    // record is a mismatch — fail closed.
    let undisclosed: Vec<String> = numstat_paths
        .iter()
        .filter(|p| !stored_paths.contains(*p))
        .cloned()
        .collect();
    if !undisclosed.is_empty() {
        let detail = format!(
            "patch {} would touch paths not in its recorded changed_paths (numstat vs stored \
             disagree): {:?}",
            patch.id, undisclosed
        );
        let _ = workflow_patch_update(
            store,
            patch,
            WorkflowPatchStatus::Conflict,
            actor,
            reason,
            Some(detail.clone()),
        )?;
        return Err(CliError::Usage(detail));
    }
    let violations = owned_path_violations(&numstat_paths, &patch.owned_paths);
    if !violations.is_empty() {
        let detail = format!(
            "patch touches paths outside owned_paths {:?}: {:?}",
            patch.owned_paths, violations
        );
        let _ = workflow_patch_update(
            store,
            patch,
            WorkflowPatchStatus::Conflict,
            actor,
            reason,
            Some(detail.clone()),
        )?;
        return Err(CliError::Usage(detail));
    }
    if !allow_dirty {
        // D6: scope the dirty guard to the patch's OWN paths. Unrelated untracked
        // files / edits no longer block every apply (and, since one applied patch
        // leaves the tree dirty, no longer cap a run at a single auto-apply). Refuse
        // only when a path THIS patch touches already has local modifications
        // (staged or unstaged) or, for files the patch creates, already exists
        // untracked — those genuinely collide. `--allow-dirty` still bypasses all.
        let dirty = git_dirty_paths(&repo_root)?;
        let colliding: Vec<String> = numstat_paths
            .iter()
            .filter(|p| dirty.contains(*p))
            .cloned()
            .collect();
        if !colliding.is_empty() {
            return Err(CliError::Usage(format!(
                "cannot apply workflow patch {} because paths it touches have uncommitted \
                 changes: {:?}\nrerun with --allow-dirty after checking the patch is independent",
                patch.id, colliding
            )));
        }
    }
    if let Err(error) = apply_patch_bytes(&repo_root, &bytes, true) {
        let detail = error.to_string();
        let _ = workflow_patch_update(
            store,
            patch,
            WorkflowPatchStatus::Conflict,
            actor,
            reason,
            Some(detail.clone()),
        )?;
        return Err(CliError::Usage(detail));
    }
    apply_patch_bytes(&repo_root, &bytes, false)?;
    workflow_patch_update(
        store,
        patch,
        WorkflowPatchStatus::Applied,
        actor,
        reason,
        None,
    )
}

pub(super) fn reject_workflow_patch_record(
    store: &HarnessStore,
    patch: &WorkflowPatch,
    actor: Option<String>,
    reason: Option<String>,
) -> CliResult<WorkflowPatch> {
    if patch.status != WorkflowPatchStatus::PendingApply {
        return Err(CliError::Usage(format!(
            "workflow patch {} is {:?}, not pending_apply",
            patch.id, patch.status
        )));
    }
    workflow_patch_update(
        store,
        patch,
        WorkflowPatchStatus::Rejected,
        actor,
        reason,
        None,
    )
}

pub(super) fn patch_status_is_pending(patch: &WorkflowPatch) -> bool {
    patch.status == WorkflowPatchStatus::PendingApply
}

pub(super) fn resolve_workflow_patch(store: &HarnessStore, args: &[String]) -> CliResult<WorkflowPatch> {
    let key = value(args, "--patch")
        .or_else(|| args.iter().find(|arg| !arg.starts_with("--")).cloned())
        .ok_or_else(|| {
            CliError::Usage(
                "workflow patch command requires <patch_id|run_id> or --patch <id>".to_string(),
            )
        })?;
    let step = value(args, "--step");
    let patches = latest_workflow_patches_in_append_order(store)?;
    if let Some(step) = step {
        return patches
            .into_iter()
            .rev()
            .find(|patch| patch.run_id == key && (patch.step_id == step || patch.label == step))
            .ok_or_else(|| {
                CliError::Usage(format!("no workflow patch for run {key} step {step}"))
            });
    }
    let exact: Vec<_> = patches
        .iter()
        .filter(|patch| patch.id == key)
        .cloned()
        .collect();
    if let Some(patch) = exact.into_iter().next() {
        return Ok(patch);
    }
    let by_run: Vec<_> = patches
        .into_iter()
        .filter(|patch| patch.run_id == key)
        .collect();
    match by_run.len() {
        1 => Ok(by_run.into_iter().next().expect("one patch")),
        0 => Err(CliError::Usage(format!(
            "no workflow patch found for {key}"
        ))),
        _ => Err(CliError::Usage(format!(
            "run {key} has multiple patches; pass --step <label|step_id>"
        ))),
    }
}
