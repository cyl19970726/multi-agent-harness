use super::*;

pub(super) fn workflow_patch_command(
    store: &HarnessStore,
    project_context: Option<&ProjectContext>,
    args: &[String],
) -> CliResult<serde_json::Value> {
    require_subcommand(args, "workflow patch list|show|apply|reject")?;
    match args[0].as_str() {
        "list" => {
            let run = value(&args[1..], "--run");
            let mut patches = latest_workflow_patches_in_append_order(store)?;
            if let Some(run) = run {
                patches.retain(|patch| patch.run_id == run);
            }
            Ok(serde_json::json!({ "patches": patches }))
        }
        "show" => {
            let patch = resolve_workflow_patch(store, &args[1..])?;
            let text = fs::read_to_string(patch_file_path(store, &patch)).unwrap_or_default();
            Ok(serde_json::json!({ "patch": patch, "diff": text }))
        }
        "apply" => {
            let patch = resolve_workflow_patch(store, &args[1..])?;
            let actor = value(&args[1..], "--actor").or_else(|| Some("operator".to_string()));
            let reason = value(&args[1..], "--reason");
            let applied = apply_workflow_patch_record(
                store,
                project_context,
                &patch,
                actor,
                reason,
                has_flag(&args[1..], "--allow-dirty"),
            )?;
            Ok(serde_json::json!({ "patch": applied }))
        }
        "reject" => {
            let patch = resolve_workflow_patch(store, &args[1..])?;
            let actor = value(&args[1..], "--actor").or_else(|| Some("operator".to_string()));
            let reason = value(&args[1..], "--reason");
            let rejected = reject_workflow_patch_record(store, &patch, actor, reason)?;
            Ok(serde_json::json!({ "patch": rejected }))
        }
        other => Err(CliError::Usage(format!(
            "unknown workflow patch command: {other}"
        ))),
    }
}

pub(super) fn manifest_path_with_root(
    repo_root: &Path,
    artifact_root: Option<&str>,
    path: &str,
) -> PathBuf {
    let raw = Path::new(path);
    if raw.is_absolute() {
        return raw.to_path_buf();
    }
    if let Some(root) = artifact_root.filter(|r| !r.trim().is_empty()) {
        let root = root.trim_end_matches('/');
        let root_path = Path::new(root);
        if raw.starts_with(root_path) {
            return repo_root.join(raw);
        }
        return repo_root.join(root_path).join(raw);
    }
    repo_root.join(raw)
}

pub(super) fn manifest_display_path(
    repo_root: &Path,
    artifact_root: Option<&str>,
    path: &str,
    abs: &Path,
) -> String {
    if let Ok(rel) = abs.strip_prefix(repo_root) {
        return rel.display().to_string();
    }
    if Path::new(path).is_absolute() {
        path.to_string()
    } else if let Some(root) = artifact_root.filter(|r| !r.trim().is_empty()) {
        format!("{}/{}", root.trim_end_matches('/'), path)
    } else {
        path.to_string()
    }
}

pub(super) fn build_manifest_file(
    repo_root: &Path,
    artifact_root: Option<&str>,
    path: &str,
) -> WorkflowArtifactFile {
    let abs = manifest_path_with_root(repo_root, artifact_root, path);
    let display = manifest_display_path(repo_root, artifact_root, path, &abs);
    let metadata = fs::metadata(&abs).ok();
    let exists = metadata.as_ref().is_some_and(|m| m.is_file());
    let (size_bytes, hash) = if exists {
        let bytes = fs::read(&abs).unwrap_or_default();
        let lossy = String::from_utf8_lossy(&bytes);
        (Some(bytes.len() as u64), Some(content_hash_hex16(&lossy)))
    } else {
        (None, None)
    };
    WorkflowArtifactFile {
        path: display,
        exists,
        size_bytes,
        hash,
        kind: None,
    }
}

pub(super) fn paths_outside_write_roots(paths: &[String], write_roots: &[String]) -> Vec<String> {
    if write_roots.is_empty() {
        return Vec::new();
    }
    paths
        .iter()
        .filter(|path| {
            !write_roots.iter().any(|root| {
                let root = root.trim_end_matches('/');
                path.as_str() == root || path.starts_with(&format!("{root}/"))
            })
        })
        .cloned()
        .collect()
}

pub(super) fn append_artifact_manifest(
    store: &HarnessStore,
    run_id: &str,
    step_id: Option<String>,
    label: Option<String>,
    artifact_root: Option<String>,
    write_roots: Vec<String>,
    paths: Vec<String>,
) -> CliResult<WorkflowArtifactManifest> {
    if paths.is_empty() {
        return Err(CliError::Usage(
            "artifact manifest requires at least one path".to_string(),
        ));
    }
    let project = workflow_project_context_for_run(store, run_id, None)?;
    let repo_root = workflow_repo_root(&project);
    let files: Vec<_> = paths
        .iter()
        .map(|path| build_manifest_file(&repo_root, artifact_root.as_deref(), path))
        .collect();
    let display_paths: Vec<String> = files.iter().map(|file| file.path.clone()).collect();
    let missing: Vec<_> = files
        .iter()
        .filter(|file| !file.exists)
        .map(|file| file.path.clone())
        .collect();
    let outside = paths_outside_write_roots(&display_paths, &write_roots);
    let (status, reason) = if !missing.is_empty() {
        (
            WorkflowArtifactManifestStatus::Missing,
            Some(format!("missing artifact files: {}", missing.join(", "))),
        )
    } else if !outside.is_empty() {
        (
            WorkflowArtifactManifestStatus::Stale,
            Some(format!(
                "artifact files outside write_roots {:?}: {}",
                write_roots,
                outside.join(", ")
            )),
        )
    } else {
        (WorkflowArtifactManifestStatus::Current, None)
    };
    let manifest = WorkflowArtifactManifest {
        id: generated_id("wfartifact"),
        run_id: run_id.to_string(),
        step_id,
        label,
        artifact_root,
        status,
        files,
        write_roots,
        created_at: now_string(),
        updated_at: None,
        reason,
    };
    store.append_workflow_artifact_manifest(&manifest)?;
    Ok(manifest)
}

pub(super) fn persist_workflow_patches(
    store: &HarnessStore,
    run: &WorkflowRun,
    outcome: &workflow::WorkflowOutcome,
    steps_json: &[serde_json::Value],
) -> CliResult<Vec<WorkflowPatch>> {
    let project = workflow_project_context_for_run(store, &run.id, None)?;
    let repo_root = workflow_repo_root(&project);
    let base_sha = git_in(&repo_root, &["rev-parse", "HEAD"])
        .ok()
        .map(|sha| sha.trim().to_string())
        .filter(|sha| !sha.is_empty());
    let patch_dir = store.root().join("workflow-patches").join(&run.id);
    fs::create_dir_all(&patch_dir)?;

    let mut patches = Vec::new();
    for (idx, result) in outcome.steps.iter().enumerate() {
        let Some(diff) = step_landing_diff(result) else {
            continue;
        };
        let details = result.details.as_ref();
        if !should_persist_workflow_patch(result.ok, details, &diff) {
            continue;
        }
        let step_id = steps_json
            .get(idx)
            .and_then(|v| v.get("id"))
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .or_else(|| result.step_id.clone())
            .unwrap_or_else(|| format!("step-{idx}"));
        let patch_ref = patch_dir.join(format!("{step_id}.patch"));
        fs::write(&patch_ref, diff.as_bytes())?;
        let owned_paths = string_array(details.and_then(|d| d.get("owned_paths")));
        let persist_changes = details
            .and_then(|d| d.get("persist_changes"))
            .and_then(|v| v.as_str())
            .map(str::to_string);
        let changed_paths = step_changed_paths(details, &diff);
        let patch = WorkflowPatch {
            id: format!("wfpatch-{step_id}"),
            run_id: run.id.clone(),
            step_id,
            label: result.label.clone(),
            phase: result.phase.clone(),
            provider: result.provider.clone(),
            status: WorkflowPatchStatus::PendingApply,
            changed_paths,
            patch_ref: patch_ref.display().to_string(),
            base_sha: base_sha.clone(),
            owned_paths,
            persist_changes,
            created_at: now_string(),
            updated_at: None,
            actor: None,
            reason: None,
            conflict_detail: None,
            applied_at: None,
            rejected_at: None,
        };
        store.append_workflow_patch(&patch)?;
        patches.push(patch);
    }
    Ok(patches)
}

pub(super) fn persist_step_artifact_manifests(
    store: &HarnessStore,
    run: &WorkflowRun,
    outcome: &workflow::WorkflowOutcome,
    steps_json: &[serde_json::Value],
) -> CliResult<Vec<WorkflowArtifactManifest>> {
    let mut manifests = Vec::new();
    for (idx, result) in outcome.steps.iter().enumerate() {
        let Some(details) = result.details.as_ref() else {
            continue;
        };
        let declared = details
            .get("expected_artifacts")
            .and_then(|v| v.get("declared"))
            .map(|v| string_array(Some(v)))
            .unwrap_or_default();
        let artifact_root = details
            .get("artifact_root")
            .and_then(|v| v.as_str())
            .map(str::to_string);
        let write_roots = string_array(details.get("write_roots"));
        if declared.is_empty() && artifact_root.is_none() && write_roots.is_empty() {
            continue;
        }
        if declared.is_empty() {
            continue;
        }
        let step_id = steps_json
            .get(idx)
            .and_then(|v| v.get("id"))
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .or_else(|| result.step_id.clone());
        manifests.push(append_artifact_manifest(
            store,
            &run.id,
            step_id,
            Some(result.label.clone()),
            artifact_root,
            write_roots,
            declared,
        )?);
    }
    Ok(manifests)
}

pub(super) fn persist_declared_artifact_manifests(
    store: &HarnessStore,
    run: &WorkflowRun,
    steps_json: &[serde_json::Value],
) -> CliResult<Vec<WorkflowArtifactManifest>> {
    let mut out = Vec::new();
    let Some(items) = run
        .final_output
        .as_ref()
        .and_then(|v| v.get("artifact_manifests"))
        .and_then(|v| v.as_array())
    else {
        return Ok(out);
    };
    for item in items {
        let paths = string_array(item.get("paths"));
        if paths.is_empty() {
            continue;
        }
        let label = item
            .get("label")
            .and_then(|v| v.as_str())
            .map(str::to_string);
        let step_id = label.as_ref().and_then(|label| {
            steps_json.iter().find_map(|step| {
                let step_label = step.get("label").and_then(|v| v.as_str())?;
                if step_label == label {
                    step.get("id").and_then(|v| v.as_str()).map(str::to_string)
                } else {
                    None
                }
            })
        });
        out.push(append_artifact_manifest(
            store,
            &run.id,
            step_id,
            label,
            item.get("artifact_root")
                .and_then(|v| v.as_str())
                .map(str::to_string),
            string_array(item.get("write_roots")),
            paths,
        )?);
    }
    Ok(out)
}

pub(super) fn run_verdict_ok(run: &WorkflowRun) -> bool {
    run.final_output
        .as_ref()
        .and_then(|v| v.get("verdict"))
        .and_then(|v| v.get("ok"))
        .and_then(|v| v.as_bool())
        .unwrap_or(run.status == WorkflowRunStatus::Completed)
}

/// Look up a run's journaled step by `label` in `final_output.steps` and read its
/// `ok` / `writable` flags. Returns `None` when no such step is present. Used to
/// guard in-script `apply_patch()` and `auto_apply_on_verdict` against steps that
/// failed or were not declared writable (D3b).
pub(super) fn outcome_step_ok_and_writable(run: &WorkflowRun, label: &str) -> Option<(bool, bool)> {
    run.final_output
        .as_ref()
        .and_then(|v| v.get("steps"))
        .and_then(|v| v.as_array())
        .and_then(|steps| {
            steps.iter().find_map(|step| {
                if step.get("label").and_then(|v| v.as_str()) == Some(label) {
                    let ok = step.get("ok").and_then(|v| v.as_bool()).unwrap_or(false);
                    let writable = step
                        .get("writable")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);
                    Some((ok, writable))
                } else {
                    None
                }
            })
        })
}

pub(super) fn process_workflow_patch_actions(
    store: &HarnessStore,
    run: &WorkflowRun,
    initial_patches: &[WorkflowPatch],
) -> CliResult<Vec<WorkflowPatch>> {
    let mut latest: BTreeMap<String, WorkflowPatch> = initial_patches
        .iter()
        .cloned()
        .map(|patch| (patch.label.clone(), patch))
        .collect();
    let mut explicit_labels = BTreeSet::new();
    let actions = run
        .final_output
        .as_ref()
        .and_then(|v| v.get("patch_actions"))
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    for action in actions {
        let Some(label) = action.get("label").and_then(|v| v.as_str()) else {
            continue;
        };
        explicit_labels.insert(label.to_string());
        let Some(patch) = latest.get(label).cloned() else {
            // D3b: a standalone apply/reject targeting a step that produced no
            // pending patch — it failed, was not writable, or discarded its diff.
            let why = match outcome_step_ok_and_writable(run, label) {
                Some((false, _)) => " (step failed — nothing to apply)",
                Some((true, false)) => " (step is not writable — nothing to apply)",
                _ => "",
            };
            eprintln!(
                "workflow patch action ignored for run {}: no pending patch labeled '{}'{why}",
                run.id, label
            );
            continue;
        };
        if !patch_status_is_pending(&patch) {
            continue;
        }
        let reason = action
            .get("reason")
            .and_then(|v| v.as_str())
            .map(str::to_string);
        let updated = match action.get("action").and_then(|v| v.as_str()) {
            Some("apply") => match apply_workflow_patch_record(
                store,
                None,
                &patch,
                Some("workflow".to_string()),
                reason,
                false,
            ) {
                Ok(updated) => updated,
                Err(error) => {
                    eprintln!(
                        "workflow patch auto-apply failed for run {} step '{}': {error}",
                        run.id, label
                    );
                    latest_workflow_patches_in_append_order(store)?
                        .into_iter()
                        .find(|p| p.id == patch.id)
                        .unwrap_or(patch)
                }
            },
            Some("reject") => {
                reject_workflow_patch_record(store, &patch, Some("workflow".to_string()), reason)?
            }
            _ => patch,
        };
        latest.insert(label.to_string(), updated);
    }

    if run_verdict_ok(run) {
        for patch in initial_patches {
            if explicit_labels.contains(&patch.label) {
                continue;
            }
            let auto = outcome_step_auto_apply(run, &patch.label);
            if !auto || !patch_status_is_pending(patch) {
                continue;
            }
            let updated = match apply_workflow_patch_record(
                store,
                None,
                patch,
                Some("workflow".to_string()),
                Some("auto_apply_on_verdict".to_string()),
                false,
            ) {
                Ok(updated) => updated,
                Err(error) => {
                    eprintln!(
                        "workflow patch auto_apply_on_verdict failed for run {} step '{}': {error}",
                        run.id, patch.label
                    );
                    latest_workflow_patches_in_append_order(store)?
                        .into_iter()
                        .find(|p| p.id == patch.id)
                        .unwrap_or_else(|| patch.clone())
                }
            };
            latest.insert(patch.label.clone(), updated);
        }
    }
    Ok(latest.into_values().collect())
}

pub(super) fn outcome_step_auto_apply(run: &WorkflowRun, label: &str) -> bool {
    run.final_output
        .as_ref()
        .and_then(|v| v.get("steps"))
        .and_then(|v| v.as_array())
        .and_then(|steps| {
            steps.iter().find_map(|step| {
                if step.get("label").and_then(|v| v.as_str()) == Some(label) {
                    step.get("auto_apply_on_verdict").and_then(|v| v.as_bool())
                } else {
                    None
                }
            })
        })
        .unwrap_or(false)
}

pub(super) fn workflow_run_script_value(
    store: &HarnessStore,
    project_context: Option<&ProjectContext>,
    args: &[String],
) -> CliResult<serde_json::Value> {
    // The script path is the first positional arg (not a --flag) or `--script <path>`.
    let path = value(args, "--script")
        .or_else(|| args.iter().find(|arg| !arg.starts_with("--")).cloned())
        .ok_or_else(|| {
            CliError::Usage("workflow run-script requires a <prog.star> path".to_string())
        })?;

    let script = std::fs::read_to_string(&path)
        .map_err(|error| CliError::Usage(format!("cannot read script {path}: {error}")))?;

    // Optional `--resume <prior_run_id>`: re-run this SAME script but reuse the
    // results of leaves that SUCCEEDED in the prior run, so a crash/kill does not
    // re-spend tokens on already-done work. Build the replay cache here after the
    // safety guard (the prior run must exist and have snapshotted the IDENTICAL
    // script; a changed script would misalign the deterministic leaf ordinals).
    let resume_from = value(args, "--resume");
    let replay = match &resume_from {
        Some(prior_run_id) => {
            let prior = latest_workflow_runs_in_append_order(store)?
                .into_iter()
                .find(|r| &r.id == prior_run_id)
                .ok_or_else(|| {
                    CliError::Usage(format!("cannot resume {prior_run_id}: no such run"))
                })?;
            let prior_script = prior
                .spec
                .as_ref()
                .and_then(|s| s.get("script"))
                .and_then(|v| v.as_str());
            match prior_script {
                Some(prev) if prev == script => {}
                Some(_) => {
                    return Err(CliError::Usage(format!(
                        "cannot resume {prior_run_id}: the script changed since that run"
                    )))
                }
                None => {
                    return Err(CliError::Usage(format!(
                        "cannot resume {prior_run_id}: that run has no snapshotted script"
                    )))
                }
            }
            Some(build_replay_map(store, prior_run_id)?)
        }
        None => None,
    };

    // Default workflow name: explicit `--name`, else the file stem. The Starlark
    // `workflow(...)` header's name can override this default once captured.
    let name = value(args, "--name").unwrap_or_else(|| {
        Path::new(&path)
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or("workflow")
            .to_string()
    });

    // Optional `--args <json>`: parsed into the opaque value injected as the
    // script's `args` global. A typo fails fast.
    let parsed_args = match value(args, "--args") {
        Some(raw) => Some(
            serde_json::from_str::<serde_json::Value>(&raw)
                .map_err(|error| CliError::Usage(format!("invalid --args json: {error}")))?,
        ),
        None => None,
    };

    let options = WorkflowDeliveryOptions {
        dry_run: has_flag(args, "--dry-run"),
        start_runtime: has_flag(args, "--start-runtime"),
        // Per-node ephemeral-worker IDLE timeout: a worker is killed only after this
        // long with NO output (a wedged provider), so a slow-but-streaming turn runs
        // to completion. Default 15 min of silence.
        timeout_ms: value(args, "--timeout-ms")
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(900_000),
        default_model: value(args, "--model"),
        default_effort: value(args, "--effort"),
        max_budget_usd: value(args, "--max-budget-usd").and_then(|v| v.parse::<f64>().ok()),
        progress: has_flag(args, "--progress"),
        project: project_context
            .cloned()
            .unwrap_or_else(|| workflow_project_context(store)),
    };

    // Who initiated the run: an explicit `--initiated-by <id>`, else the
    // ambient agent member id (when an agent shells out), else "operator".
    let initiated_by = value(args, "--initiated-by")
        .or_else(|| std::env::var("HARNESS_AGENT_MEMBER_ID").ok())
        .filter(|id| !id.is_empty())
        .unwrap_or_else(|| "operator".to_string());

    // Reap any orphaned `Running` rows from crashed prior runs before starting a
    // new one, so phantoms never accumulate in the store / dashboard. Best-effort.
    let _ = reap_stale_workflow_runs(store);
    let _ = reap_orphaned_workers(store, false);

    // Mint the run id up front so the real driver can journal each step's
    // `running` row as it starts (live SSE progress).
    let run_id = generated_id("wfrun");

    let mut run = WorkflowRun {
        id: run_id.clone(),
        workflow_name: name.clone(),
        project_binding_id: Some(options.project.id.clone()),
        status: WorkflowRunStatus::Running,
        step_ids: Vec::new(),
        created_at: now_string(),
        ended_at: None,
        summary: None,
        // The script's `args` global is carried opaquely onto the run.
        args: parsed_args.clone(),
        agents_spawned: 0,
        final_output: None,
        // Always-persisted durable audit record: who ran it + the raw script
        // text (the script is not a serializable spec), plus the retention
        // policy governing the heavy trace. `design_intent` is filled in from the
        // captured `workflow(...)` header once evaluation succeeds.
        initiated_by: Some(initiated_by),
        design_intent: None,
        // The resumed run is a NEW run_id; record which prior run it resumed from
        // so the new run has a complete, auditable record (DESIGN step 6).
        spec: Some(match &resume_from {
            Some(prior) => serde_json::json!({
                "lang": "starlark",
                "script": script,
                "resumed_from": prior,
            }),
            None => serde_json::json!({ "lang": "starlark", "script": script }),
        }),
        // Stamp this driver process's pid so the serve-side reaper can detect an
        // abandoned run (driver killed/crashed before journaling a terminal row).
        host_pid: Some(std::process::id()),
        // Mark dry-run validation runs so they are never mistaken for real runs in
        // the jsonl / dashboard (issue #89 item 2).
        dry_run: options.dry_run,
        terminal_reason: None,
        partial_output_available: false,
    };
    store.append_workflow_run(&run)?;

    // Optional per-run spend ceiling: once cumulative step cost reaches it, the
    // runtime short-circuits further agent()/parallel() calls into failed `budget`
    // steps. A `workflow(budget_usd=…)` header may lower it further.
    let max_budget_usd = value(args, "--max-budget-usd").and_then(|v| v.parse::<f64>().ok());

    let started = {
        let run_id = run_id.clone();
        let driver = move |step: &workflow::AgentStepSpec| {
            workflow_real_agent_step(store, &run_id, &options, step)
        };
        harness_workflow::starlark_front::run_starlark_with_budget(
            &script,
            &name,
            parsed_args.as_ref(),
            &driver,
            max_budget_usd,
            replay,
        )
        .map_err(|error| CliError::Usage(error.to_string()))?
    };

    // Persist the captured mandatory meta: the declared `design_intent` and the
    // workflow name (the header's name overrides the CLI default).
    run.design_intent = Some(started.meta.design_intent.clone());
    run.workflow_name = started.meta.name.clone();

    warn_discarded_worktree_diffs(&run.id, &started.outcome);
    journal_workflow_outcome(store, run, &started.outcome)
}

/// Create the WorkflowRun (running), dispatch the workflow body with the given
/// agent-step driver, journal a WorkflowStep per step, and finalize the run.
/// The `driver` is injectable so tests pass a mock instead of the real provider
/// path.
pub(super) fn run_workflow_with_driver(
    store: &HarnessStore,
    run_id: &str,
    def: &workflow::WorkflowDef,
    prompt: &str,
    dry_run: bool,
    project_binding_id: Option<String>,
    driver: &workflow::AgentStepFn<'_>,
) -> CliResult<serde_json::Value> {
    let run = WorkflowRun {
        id: run_id.to_string(),
        workflow_name: def.name.to_string(),
        project_binding_id,
        status: WorkflowRunStatus::Running,
        step_ids: Vec::new(),
        created_at: now_string(),
        ended_at: None,
        summary: None,
        // Registry runs are not parameterized and do not snapshot the scheduler;
        // `journal_workflow_outcome` fills `final_output`/`agents_spawned` (0 here).
        args: None,
        agents_spawned: 0,
        final_output: None,
        // Registry runs are operator-triggered and carry no dynamic spec; they
        // default to durable trace retention.
        initiated_by: Some("operator".to_string()),
        design_intent: None,
        spec: None,
        // Stamp this driver process's pid so the serve-side reaper can detect an
        // abandoned run (see the run-script path and `reap_abandoned_runs`).
        host_pid: Some(std::process::id()),
        dry_run,
        terminal_reason: None,
        partial_output_available: false,
    };
    store.append_workflow_run(&run)?;

    // Dispatch the compiled workflow body (option C registry dispatch).
    let outcome = (def.run)(driver, prompt);

    journal_workflow_outcome(store, run, &outcome)
}

/// Journal the running `run`'s terminal steps + finalize it from a
/// [`workflow::WorkflowOutcome`]. Shared by the registry `run` path and the
/// dynamic `run-script` (Starlark) path so both journal identically.
pub(super) fn journal_workflow_outcome(
    store: &HarnessStore,
    mut run: WorkflowRun,
    outcome: &workflow::WorkflowOutcome,
) -> CliResult<serde_json::Value> {
    let run_id = run.id.clone();
    // Journal one TERMINAL WorkflowStep per StepResult, preserving order. When
    // the driver already journaled a `running` row at step start (real path), we
    // REUSE its `step_id` and real `started_at` so the latest-wins projection
    // updates the same row in place and the journaled window reflects true
    // (overlapping) execution. Mock drivers leave those `None`, so we mint a
    // fresh id and stamp the journal time, preserving the pre-existing behavior.
    let mut steps_json = Vec::new();
    for result in &outcome.steps {
        // The real driver (`workflow_real_agent_step`) already journaled this
        // step's terminal row the instant it completed — for live per-step SSE.
        // It is recognisable by a present `step_id`. Mock/test drivers leave it
        // `None`, so we mint an id and journal the terminal row here.
        let already_journaled = result.step_id.is_some();
        let step_id = result
            .step_id
            .clone()
            .unwrap_or_else(|| generated_id("wfstep"));
        let started_at = result.started_at.clone().unwrap_or_else(now_string);
        let step = build_terminal_step(&run_id, step_id.clone(), started_at, result);
        if !already_journaled {
            store.append_workflow_step(&step)?;
        }
        run.step_ids.push(step_id);
        steps_json.push(serde_json::to_value(&step)?);
    }

    // Finalize the run with the workflow's own status verdict + the collected
    // structured output and the agent count the dispatch spawned.
    run.status = outcome.status;
    run.ended_at = Some(now_string());
    run.summary = Some(outcome.summary.clone());
    run.agents_spawned = outcome.agents_spawned;
    run.final_output = outcome.final_output.clone();
    run.terminal_reason = Some(if outcome.status == WorkflowRunStatus::Completed {
        WorkflowTerminalReason::Completed
    } else {
        WorkflowTerminalReason::ProviderFailed
    });
    store.append_workflow_run(&run)?;
    let mut patches = persist_workflow_patches(store, &run, outcome, &steps_json)?;
    let mut artifact_manifests =
        persist_step_artifact_manifests(store, &run, outcome, &steps_json)?;
    artifact_manifests.extend(persist_declared_artifact_manifests(
        store,
        &run,
        &steps_json,
    )?);
    patches = process_workflow_patch_actions(store, &run, &patches)?;
    // The run has reached a terminal status — notify any configured completion hook
    // (no-op unless HARNESS_WORKFLOW_ON_COMPLETE is set). Fires here, inside the
    // run-owning process, so a backgrounded `run-script &` still notifies.
    fire_workflow_completion_hook(&run);

    Ok(serde_json::json!({
        "run": serde_json::to_value(&run)?,
        "steps": steps_json,
        "patches": patches,
        "artifact_manifests": artifact_manifests,
    }))
}
