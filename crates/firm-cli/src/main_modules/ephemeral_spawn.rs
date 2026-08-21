use super::*;


/// Spin up a NEW one-shot EDITABLE ephemeral worker for one `agent()` node and
/// reduce its result into a [`workflow::StepResult`].
///
/// Workspace: read-only leaves run in the selected project root (#190 — even on a
/// provider that cannot physically enforce read-only). Editable leaves default to
/// a harness-owned throwaway worktree (its `git diff` is collected and the
/// worktree is NOT auto-merged; cleanup is the `WorktreeGuard`'s Drop, bulletproof
/// across success/failure/timeout); `write_mode="direct"` is the explicit simple
/// serial path that writes the selected project root immediately. Worktree diffs
/// are captured as pending patches; direct diffs are recorded as evidence because
/// the change is already in the repo working tree.
pub(super) fn spawn_ephemeral_worker(
    store: &HarnessStore,
    options: &WorkflowDeliveryOptions,
    spec: &workflow::AgentStepSpec,
    run_id: &str,
    session_id: &str,
) -> CliResult<workflow::StepResult> {
    // The worker's shared cwd + worktree base is the PROJECT ROOT (the git repo
    // where CLAUDE.md / AGENTS.md / memory live), NOT the harness process cwd and
    // NOT the centralized store_root (goal-multi-project P3/P4). A long-running
    // `serve` never `cd`s after a project switch, so reading process cwd here would
    // run the worker in the wrong tree.
    let project = &options.project;
    let repo_root = workflow_repo_root(project);

    // Opt-in isolation: harness-owned throwaway worktree, else the shared cwd.
    // The guard (when present) cleans up on every exit path via Drop.
    // A node isolates when it explicitly opts in, or when it is `writable` (an
    // editing worker runs in a throwaway worktree so its writes land in a
    // discardable checkout, never the live repo). Read-only scans/reviews do not
    // implicitly require git worktrees — read-only leaves stay in the selected
    // project root even on a provider that cannot enforce read-only (#190).
    // `write_mode="direct"` writes the shared project root in place instead of a
    // worktree, so it validates the tree up front and never isolates.
    let direct_write = step_write_mode_direct(spec);
    if direct_write {
        ensure_direct_write_ready(project, &repo_root, spec)?;
    }
    let isolate = step_needs_isolation(
        spec.writable,
        spec.isolation.as_deref(),
        spec.write_mode.as_deref(),
    );

    // GLOBAL / non-git policy (P5): an isolated/writable node needs a git worktree,
    // which cannot exist in a non-git project (the reserved `_global` `~/` project,
    // or any non-repo root). Fail LOUD with the same actionable message the
    // `is_git_repo` gate in `WorktreeGuard::create` uses (#89 item 5) — surfaced
    // here BEFORE the worktree attempt so the project id / kind is named.
    if isolate && !project.is_git_repo {
        return Err(CliError::Usage(format!(
            "node '{}' needs an isolated git worktree (it is writable, or sets \
             isolation=\"worktree\"), but project '{}' ({}) is not a git repository. \
             Run this step READ-ONLY (drop writable / isolation=\"none\") and retrieve \
             its output with `harness workflow get-output <run_id> --step {}`, or run \
             the workflow against a git-backed project.",
            spec.label,
            project.id,
            repo_root.display(),
            spec.label,
        )));
    }

    let guard = if isolate {
        let guard = WorktreeGuard::create(&repo_root, run_id, &spec.label, session_id)?;
        let repo = repo_root.display().to_string();
        let branch = command_stdout("git", &["-C", &repo, "branch", "--show-current"])
            .ok()
            .map(|branch| branch.trim().to_string())
            .filter(|branch| !branch.is_empty())
            .unwrap_or_else(|| "detached".to_string());
        let head = command_stdout("git", &["-C", &repo, "rev-parse", "--short", "HEAD"])
            .ok()
            .map(|head| head.trim().to_string())
            .filter(|head| !head.is_empty())
            .unwrap_or_else(|| "unknown".to_string());
        eprintln!(
            "workflow: created worktree for node '{}' from project root {} ({} {}) at {}",
            spec.label,
            repo_root.display(),
            branch,
            head,
            guard.path.display()
        );
        Some(guard)
    } else {
        None
    };
    let cwd = guard
        .as_ref()
        .map(|g| g.path.clone())
        .unwrap_or_else(|| repo_root.clone());

    // This id is a Harness-local execution key, not a provider session id. The
    // provider-owned id is discovered from the native stream and attached to
    // the terminal WorkflowStep as a NativeSessionRef.
    let session_id = session_id.to_string();
    let session_dir = store
        .root()
        .join("runtimes")
        .join("workflow-workers")
        .join(&session_id);
    fs::create_dir_all(&session_dir)?;

    // The structured schema normalized to a real JSON Schema for the providers'
    // native flags (claude `--json-schema`, codex `--output-schema`). `None` for
    // text-mode steps.
    let schema_json = spec.schema.as_ref().map(schema_to_json_schema);

    // One spawn of the configured provider against a (possibly augmented) prompt.
    // Factored into a closure so structured mode can re-run it once for the retry.
    let effective_model = workflow_effective_model(options, spec);
    let effective_effort = workflow_effective_effort(options, spec);
    let default_wall_clock_ms = spec.timeout_s.map(|seconds| seconds.saturating_mul(1_000));
    let spawn_once_with_limits =
        |prompt: &str, timeout_ms: u64, wall_clock_ms: Option<u64>| -> CliResult<EphemeralSpawn> {
            let ctx = EphemeralSpawnContext {
                session_dir: &session_dir,
                session_id: &session_id,
                run_id,
                spec,
                schema_json: schema_json.as_ref(),
                prompt,
                cwd: &cwd,
                model: effective_model,
                effort: effective_effort,
                service_tier: spec.service_tier.as_deref(),
                timeout_ms,
                wall_clock_ms,
                max_budget_usd: options.max_budget_usd,
            };
            match provider_adapter(spec.provider.as_str()) {
                Some(adapter) => adapter.spawn_ephemeral(&ctx),
                None => Err(unknown_provider_error(&spec.provider, "ephemeral worker")),
            }
        };
    let spawn_once = |prompt: &str| -> CliResult<EphemeralSpawn> {
        spawn_once_with_limits(prompt, options.timeout_ms, default_wall_clock_ms)
    };

    // Retry ONCE on a transient PROCESS crash — a non-zero / signalled exit that
    // did NOT time out and produced no reply. That is a blip/crash worth retrying;
    // it deliberately does NOT retry a timeout (we'd just re-hang for another
    // window) nor a clean-exit delivery failure (auth/usage-limit — we'd reproduce
    // it). Distinct from the schema-conformance retry below.
    let spawn_once_resilient = |prompt: &str| -> CliResult<EphemeralSpawn> {
        let first = spawn_once(prompt)?;
        let transient_crash =
            !first.ok && !first.timed_out && first.reply.is_none() && first.exit_code != Some(0);
        if transient_crash {
            std::thread::sleep(Duration::from_millis(500));
            return spawn_once(prompt);
        }
        Ok(first)
    };

    // STRUCTURED mode (spec.schema is Some): append a JSON-only instruction to the
    // prompt, then parse + validate the reply into a structured object. On failure
    // re-run the worker ONCE with a corrective suffix; if it still fails, leave
    // `structured` None and record a "schema" step failure below. Text-mode steps
    // (no schema) just deliver the prompt verbatim, as before.
    let required_keys: Vec<String> = spec
        .schema
        .as_ref()
        .map(schema_required_keys)
        .unwrap_or_default();

    // Wall-clock span of the worker process itself, for the step's `duration_ms`.
    let worker_start = Instant::now();
    let mut structured: Option<serde_json::Value> = None;
    let mut schema_retry_limits: Option<(u64, Option<u64>)> = None;
    let mut schema_retry_timed_out = false;
    let spawn = if let Some(schema) = &spec.schema {
        let instruction = schema_instruction(schema);

        // First attempt: prompt + the JSON-only instruction. Prefer the
        // provider-validated `structured` (native --json-schema/--output-schema);
        // fall back to extracting JSON from the reply text (the prompt-hint path).
        let mut spawn = spawn_once_resilient(&format!("{}{instruction}", spec.prompt))?;
        structured = spawn.structured.clone().or_else(|| {
            spawn
                .reply
                .as_deref()
                .and_then(extract_json_object)
                .filter(|obj| object_has_required_keys(obj, &required_keys))
        });

        // ONE corrective retry when the worker produced no valid JSON.
        if structured.is_none() {
            let (retry_timeout_ms, retry_wall_clock_ms) =
                schema_correction_retry_limits(options.timeout_ms, default_wall_clock_ms);
            schema_retry_limits = Some((retry_timeout_ms, retry_wall_clock_ms));
            let retry_prompt = format!(
                "{}{instruction}\n\nYour previous reply was not valid JSON with keys [{}]; \
                 return ONLY that JSON object.",
                spec.prompt,
                required_keys.join(", "),
            );
            spawn = spawn_once_with_limits(&retry_prompt, retry_timeout_ms, retry_wall_clock_ms)?;
            schema_retry_timed_out = spawn.timed_out;
            structured = spawn.structured.clone().or_else(|| {
                spawn
                    .reply
                    .as_deref()
                    .and_then(extract_json_object)
                    .filter(|obj| object_has_required_keys(obj, &required_keys))
            });
        }
        spawn
    } else {
        spawn_once_resilient(&spec.prompt)?
    };

    let duration_ms = worker_start.elapsed().as_millis() as u64;

    // A schema-mode step that never yielded valid JSON is a FAILURE — surface it
    // so the dashboard shows the same observability shape as a worker failure.
    let schema_failed = spec.schema.is_some() && structured.is_none();

    // Collect the worktree diff as the node's evidence (isolation path only). We
    // read it BEFORE the guard drops (which removes the worktree). Non-git /
    // GLOBAL projects never reach the isolation path (the policy gate above rejects
    // a writable/isolated node there), so diff evidence is necessarily skipped for
    // them — read-only `_global` nodes simply carry no diff (P5, documented).
    let diff = if isolate {
        ephemeral_worktree_diff(&cwd)
    } else {
        None
    };
    // D4a: enumerate the changed paths from the SAME worktree state (before the
    // guard drops it) via `git diff --name-status -z -M`, recording both rename
    // sides. Stored on the step so persist / landing don't re-parse the diff text.
    let worktree_changed_paths = if isolate {
        ephemeral_worktree_changed_paths(&cwd)
    } else {
        None
    };
    let direct_diff = if direct_write {
        direct_write_diff(&repo_root)
    } else {
        None
    };
    let artifact_outcome = collect_expected_artifacts(&cwd, &repo_root, &spec.expected_artifacts);

    let mut output_summary = if let Some(reply) = spawn.reply.clone() {
        // The worker's FINAL answer, FULL and FAITHFUL — NOT truncated. This is the
        // text `agent()` hands the program in text mode: the program splits it
        // (`.splitlines()`, first-line verdicts) AND forward-injects it into the next
        // leaf's prompt. Capping it (the old 4000-char clip) silently truncated the
        // node's output, so chaining a long result into a later leaf (e.g. a synthesis
        // over deep-dive sections) lost most of the input — a real design defect. The
        // full text is the node's durable Workflow outcome; newlines are
        // preserved. Bounding runaway output is the budget/idle-timeout's job.
        reply
    } else {
        format!(
            "{} ephemeral worker for {} ({})",
            spec.provider,
            spec.label,
            if spawn.ok { "ok" } else { "failed" }
        )
    };
    if let Some(diff) = &diff {
        if diff.trim().is_empty() {
            output_summary.push_str(" [worktree diff: empty]");
        } else {
            let lines = diff.lines().count();
            output_summary.push_str(&format!(" [worktree diff: {lines} lines]"));
        }
    }
    if let Some(diff) = &direct_diff {
        if diff.trim().is_empty() {
            output_summary.push_str(" [direct diff: empty]");
        } else {
            let lines = diff.lines().count();
            output_summary.push_str(&format!(" [direct diff: {lines} lines]"));
        }
    }
    if !spawn.ok && !spawn.stderr.trim().is_empty() {
        let err = spawn.stderr.replace('\n', " ");
        let err = truncate_on_char_boundary(&err, 160);
        output_summary.push_str(&format!(" [error: {err}]"));
    }
    if schema_failed {
        output_summary.push_str(" [schema: no valid JSON with required keys]");
    }
    if !artifact_outcome.copied.is_empty() {
        output_summary.push_str(&format!(
            " [expected artifacts copied: {}]",
            artifact_outcome.copied.join(", ")
        ));
    }
    if !artifact_outcome.failures.is_empty() {
        output_summary.push_str(&format!(
            " [expected artifacts missing/empty: {}]",
            artifact_outcome.failures.join("; ")
        ));
    }

    // Drop the guard here (explicitly, for clarity) AFTER the diff is collected —
    // cleanup layer 1 (normal) for the worktree path. For the shared-cwd path the
    // guard is None and there is nothing to remove.
    drop(guard);

    let mut details = build_step_details(
        spec,
        &spawn,
        effective_model,
        duration_ms,
        diff.as_deref(),
        worktree_changed_paths.as_deref(),
    );
    if let Some(direct_diff) = direct_diff.as_deref() {
        if let Some(map) = details.as_object_mut() {
            let (text, truncated) = if direct_diff.len() > WORKTREE_DIFF_CAP {
                (
                    truncate_on_char_boundary(direct_diff, WORKTREE_DIFF_CAP),
                    true,
                )
            } else {
                (direct_diff, false)
            };
            map.insert(
                "direct_diff".into(),
                serde_json::Value::String(text.to_string()),
            );
            map.insert(
                "direct_diff_truncated".into(),
                serde_json::Value::Bool(truncated),
            );
        }
    }
    if let Some((retry_timeout_ms, retry_wall_clock_ms)) = schema_retry_limits {
        if let Some(map) = details.as_object_mut() {
            map.insert(
                "schema_retry".into(),
                serde_json::json!({
                    "attempted": true,
                    "idle_timeout_ms": retry_timeout_ms,
                    "wall_clock_ms": retry_wall_clock_ms,
                    "timed_out": schema_retry_timed_out,
                }),
            );
        }
    }
    // Record a "schema" failure (reusing the same failure shape build_step_details
    // emits for worker failures) so the dashboard renders the schema miss.
    if schema_failed {
        if let Some(map) = details.as_object_mut() {
            map.insert(
                "failure".into(),
                serde_json::json!({
                    "failed": true,
                    "reason": "schema",
                    "detail": schema_failure_detail(
                        &required_keys,
                        schema_retry_limits.is_some(),
                        schema_retry_timed_out,
                    ),
                }),
            );
        }
    }
    if let Some(map) = details.as_object_mut() {
        map.insert(
            "expected_artifacts".into(),
            serde_json::json!({
                "declared": spec.expected_artifacts.clone(),
                "copied": artifact_outcome.copied.clone(),
                "failures": artifact_outcome.failures.clone(),
            }),
        );
        if !artifact_outcome.failures.is_empty() && map.get("failure").is_none() {
            map.insert(
                "failure".into(),
                serde_json::json!({
                    "failed": true,
                    "reason": "expected_artifacts",
                    "detail": artifact_outcome.failures.join("; "),
                }),
            );
        }
    }

    // output-schema and last-message files are process-local transport aids.
    // Remove them after reducing the explicit Workflow outcome; provider-native
    // history remains in the provider store.
    let _ = fs::remove_dir_all(&session_dir);

    // The step is ok iff the worker succeeded AND (text mode OR schema parsed).
    let ok = step_ok_after_gates(spawn.ok, schema_failed, &artifact_outcome);

    Ok(workflow::StepResult {
        phase: spec.phase.clone(),
        label: spec.label.clone(),
        provider: spec.provider.clone(),
        isolation: spec.isolation.clone(),
        ok,
        output_summary,
        step_id: None,
        started_at: None,
        details: Some(details),
        structured,
        ordinal: spec.ordinal,
    })
}
