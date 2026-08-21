use super::*;


pub(super) fn start_agent_runtime(store: &HarnessStore, agent_id: &str) -> CliResult<ProviderLaunchProfile> {
    let mut member = latest_member(store, agent_id)?;
    ensure_member_accepts_delivery(&member)?;
    if let Some(runtime_id) = member.provider_runtime_id.as_deref() {
        if let Some(runtime) = latest_runtime(store, runtime_id)? {
            if runtime_is_alive(&runtime) {
                return Ok(member);
            }
        }
    }
    member.status = ProviderLaunchStatus::Creating;
    member.last_seen_at = Some(now_string());
    store.append_member(&member)?;
    let runtime = match start_provider_runtime(store, &member) {
        Ok(runtime) => runtime,
        Err(error) => {
            member.status = ProviderLaunchStatus::Error;
            member.last_seen_at = Some(now_string());
            store.append_member(&member)?;
            append_harness_runtime_control_fact(
                store,
                &member.id,
                member.provider_runtime_id.as_deref(),
                None,
                "runtime_start_failed",
                &format!("{} runtime failed to start: {error}", member.provider),
                None,
            )?;
            return Err(error);
        }
    };
    member.status = ProviderLaunchStatus::Idle;
    member.provider_runtime_id = Some(runtime.id.clone());
    member.control_endpoint = runtime.control_endpoint.clone();
    member.last_seen_at = Some(now_string());
    store.append_runtime(&runtime)?;
    store.append_member(&member)?;
    append_harness_runtime_control_fact(
        store,
        &member.id,
        Some(runtime.id.as_str()),
        None,
        "runtime_started",
        "Codex app-server runtime started",
        None,
    )?;
    Ok(member)
}

pub(super) fn ensure_member_accepts_delivery(member: &ProviderLaunchProfile) -> CliResult<()> {
    if member_status_rejects_delivery(&member.status) {
        return Err(CliError::Usage(format!(
            "agent {} is {:?}; closed, closing, or retired members cannot receive delivery or be restarted",
            member.id, member.status
        )));
    }
    Ok(())
}

pub(super) fn member_status_rejects_delivery(status: &ProviderLaunchStatus) -> bool {
    matches!(
        status,
        ProviderLaunchStatus::Closing
            | ProviderLaunchStatus::Closed
            | ProviderLaunchStatus::Retired
    )
}

pub(super) fn runtime_is_alive(runtime: &ProviderProcess) -> bool {
    // Exec-stream runtimes don't have persistent PIDs or sockets.
    // Runtime is considered alive if its status is Running.
    runtime.status == ProviderProcessStatus::Running && runtime.control_endpoint.is_some()
}

pub(super) fn pid_is_alive(pid: u32) -> bool {
    Command::new("kill")
        .arg("-0")
        .arg(pid.to_string())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

pub(super) struct DeliveryOptions {
    pub(super) agent_id: String,
    pub(super) message_filter: Option<String>,
    pub(super) dry_run: bool,
    pub(super) start_runtime: bool,
    pub(super) timeout_ms: u64,
}

#[derive(Debug, Clone)]
pub(super) struct WorkflowDeliveryOptions {
    pub(super) dry_run: bool,
    #[allow(dead_code)]
    pub(super) start_runtime: bool,
    pub(super) timeout_ms: u64,
    pub(super) default_model: Option<String>,
    pub(super) default_effort: Option<String>,
    pub(super) max_budget_usd: Option<f64>,
    pub(super) progress: bool,
    pub(super) project: ProjectContext,
}

pub(super) const HARNESS_WORKFLOW_CHILD_STORE_ROOT_ENV: &str = "HARNESS_WORKFLOW_CHILD_STORE_ROOT";
pub(super) const HARNESS_WORKFLOW_ALLOW_STORE_MUTATION_ENV: &str = "HARNESS_WORKFLOW_ALLOW_STORE_MUTATION";

pub(super) fn workflow_child_store_root(session_dir: &Path) -> PathBuf {
    session_dir.join("nested-harness-store")
}

pub(super) fn workflow_child_firm_home(session_dir: &Path) -> PathBuf {
    session_dir.join("nested-harness-home")
}

pub(super) fn workflow_store_mutation_allowed() -> bool {
    env::var(HARNESS_WORKFLOW_ALLOW_STORE_MUTATION_ENV).as_deref() == Ok("1")
}

pub(super) fn apply_workflow_child_store_guard(
    cmd: &mut Command,
    session_dir: &Path,
    allow_store_mutation: bool,
) {
    cmd.env("HARNESS_PARENT_WORKFLOW_SESSION_DIR", session_dir);
    if allow_store_mutation {
        return;
    }
    cmd.env(
        HARNESS_WORKFLOW_CHILD_STORE_ROOT_ENV,
        workflow_child_store_root(session_dir),
    )
    .env("HARNESS_HOME", workflow_child_firm_home(session_dir))
    .env("HARNESS_WORKFLOW_STORE_GUARD", "isolated")
    .env_remove("FIRM_PROJECT")
    .env_remove("HARNESS_PROJECT");
}

/// Emit one compact NDJSON progress event to STDERR (used when `--progress` is on).
/// Stderr — not stdout — so stdout stays a single parseable JSON document; an agent
/// caller's shell tool captures both streams, so it still sees the live timeline.
pub(super) fn emit_progress(event: &serde_json::Value) {
    eprintln!("{event}");
}

pub(super) fn workflow_effective_model<'a>(
    options: &'a WorkflowDeliveryOptions,
    spec: &'a workflow::AgentStepSpec,
) -> Option<&'a str> {
    spec.model.as_deref().or(options.default_model.as_deref())
}

pub(super) fn workflow_effective_effort<'a>(
    options: &'a WorkflowDeliveryOptions,
    spec: &'a workflow::AgentStepSpec,
) -> Option<&'a str> {
    spec.effort.as_deref().or(options.default_effort.as_deref())
}

/// The REAL agent-step driver. Drives one provider delivery through the neutral
/// seam: (1) queue a RegistryMessage addressed to the member, (2) deliver exactly that
/// message via `deliver_agent_messages_value` (which claims + runs
/// `run_provider_delivery`), and (3) reduce the explicit provider outcome into
/// a [`workflow::StepResult`]. Provider history remains in its native session.
///
/// This fn is TOTAL: any error (store failure, no runtime, provider failure) is
/// reported as `StepResult { ok: false, .. }` so the workflow's control flow —
/// and the `parallel()` barrier — stays in charge rather than unwinding.
///
/// Build the TERMINAL `WorkflowStep` row for a finished step. The real
/// completion time is `started_at + duration_ms` (the worker's measured
/// duration), not the journal `now`: at finalize every step is journaled with
/// the same `now`, which would make a serial step falsely overlap the later
/// parallel ones. Shared by the live per-step journal (in the driver) and the
/// finalize journal (for mock/test drivers).
pub(super) fn build_terminal_step(
    run_id: &str,
    step_id: String,
    started_at: String,
    result: &workflow::StepResult,
) -> WorkflowStep {
    let now = now_string();
    let ended_at = match (
        Some(created_ms(&started_at)).filter(|&ms| ms > 0),
        result
            .details
            .as_ref()
            .and_then(|d| d.get("duration_ms"))
            .and_then(|v| v.as_u64()),
    ) {
        (Some(start_ms), Some(dur)) => {
            format!("unix-ms:{}", start_ms.saturating_add(u128::from(dur)))
        }
        _ => now,
    };
    WorkflowStep {
        id: step_id,
        run_id: run_id.to_string(),
        phase: result.phase.clone(),
        label: result.label.clone(),
        native_session: result
            .details
            .as_ref()
            .and_then(|details| details.get("native_session"))
            .and_then(|value| serde_json::from_value(value.clone()).ok()),
        status: result.step_status(),
        output_summary: Some(result.output_summary.clone()),
        result: Some(workflow::step_result_json(result)),
        started_at,
        ended_at: Some(ended_at),
        terminal_reason: Some(if result.ok {
            WorkflowTerminalReason::Completed
        } else {
            WorkflowTerminalReason::ProviderFailed
        }),
        partial: false,
    }
}

pub(super) fn workflow_real_agent_step(
    store: &HarnessStore,
    run_id: &str,
    options: &WorkflowDeliveryOptions,
    spec: &workflow::AgentStepSpec,
) -> workflow::StepResult {
    // Mint an internal transport-attempt id before launch. It scopes temporary
    // files only; the provider-reported NativeSessionRef is attached after
    // discovery and remains the sole drill-in/resume locator.
    let step_id = generated_id("wfstep");
    let session_id = generated_id("session");
    let started_at = now_string();
    let running = WorkflowStep {
        id: step_id.clone(),
        run_id: run_id.to_string(),
        phase: spec.phase.clone(),
        label: spec.label.clone(),
        native_session: None,
        status: WorkflowStepStatus::Running,
        output_summary: None,
        result: None,
        started_at: started_at.clone(),
        ended_at: None,
        terminal_reason: None,
        partial: false,
    };
    // A failure to journal the start row must not abort the step; the terminal
    // row still records the outcome. Best-effort, like the rest of this seam.
    let _ = store.append_workflow_step(&running);

    // Live progress to stderr (opt-in): the caller sees this step go live — its
    // phase and label — the instant it starts, not batched at run finalize.
    if options.progress {
        emit_progress(&serde_json::json!({
            "event": "step",
            "status": "running",
            "phase": spec.phase,
            "label": spec.label,
            "provider": spec.provider,
            "ordinal": spec.ordinal,
        }));
    }

    let result = match try_workflow_real_agent_step(store, options, spec, run_id, &session_id) {
        Ok(mut result) => {
            result.step_id = Some(step_id.clone());
            result.started_at = Some(started_at.clone());
            result
        }
        Err(error) => {
            // A setup/spawn error (e.g. worktree create or process spawn failed)
            // never reached a provider turn, so it has no usage/exit telemetry.
            // We still record a structured failure + the static identity so the
            // dashboard renders the same observability shape as a worker failure.
            let details = serde_json::json!({
                "provider": spec.provider,
                "model": workflow_effective_model(options, spec),
                "failure": {
                    "failed": true,
                    "reason": "spawn",
                    "detail": error.to_string(),
                },
            });
            workflow::StepResult {
                phase: spec.phase.clone(),
                label: spec.label.clone(),
                provider: spec.provider.clone(),
                isolation: spec.isolation.clone(),
                ok: false,
                output_summary: format!("agent step error: {error}"),
                step_id: Some(step_id.clone()),
                started_at: Some(started_at.clone()),
                details: Some(details),
                structured: None,
                ordinal: spec.ordinal,
            }
        }
    };
    // Journal the TERMINAL row the instant this step finishes. The WorkflowStep
    // SSE watcher tails workflow_steps.jsonl, so the dashboard's per-step status +
    // tokens now light up live as each worker completes — not batched at run
    // finalize. `run_workflow_with_driver` recognises this (step_id is Some) and
    // does not re-journal.
    let _ = store.append_workflow_step(&build_terminal_step(run_id, step_id, started_at, &result));

    // Live progress to stderr (opt-in): the step's terminal status the instant it
    // finishes, so the caller tracks completion per phase as the run streams.
    if options.progress {
        emit_progress(&serde_json::json!({
            "event": "step",
            "status": if result.ok { "ok" } else { "failed" },
            "phase": result.phase,
            "label": result.label,
            "ok": result.ok,
            "ordinal": result.ordinal,
        }));
    }
    result
}

pub(super) fn try_workflow_real_agent_step(
    store: &HarnessStore,
    options: &WorkflowDeliveryOptions,
    spec: &workflow::AgentStepSpec,
    run_id: &str,
    session_id: &str,
) -> CliResult<workflow::StepResult> {
    // The node references a PROVIDER (not a pre-existing member). In --dry-run
    // (CI default) we return a MOCK StepResult so the run/steps journal, the
    // dashboard, the acceptance script, and `cargo test` exercise the full
    // contract end-to-end without spawning a provider or spending tokens. The
    // real (non-dry-run) path spins up a one-shot EDITABLE ephemeral worker.
    if options.dry_run {
        let isolation_note = match spec.isolation.as_deref() {
            Some(mode) => format!(", isolation={mode}"),
            None => String::new(),
        };
        let model_note = match spec.model.as_deref() {
            Some(model) => format!(", model={model}"),
            None => String::new(),
        };
        // Include multi-byte (CJK) text in the mock output so the dry-run path
        // exercises the SAME truncation/summary code a real non-ASCII run hits —
        // a dry-run that stays pure-ASCII gave a false green for the CJK
        // byte-slice panic class (issue #89 item 2; the panic itself is fixed in
        // #94, this keeps dry-run representative so a regression can't hide).
        let output_summary = format!(
            "ephemeral {} worker (dry-run) for {}{model_note}{isolation_note} · 校验占位中文输出",
            spec.provider, spec.label,
        );
        // In schema mode, synthesize a mock structured object so `cargo test` +
        // the acceptance script exercise the structured path WITHOUT a live
        // provider. Each value is TYPE-CORRECT for the key's flat schema hint
        // (e.g. a "bool" hint -> `true`), so a compiled phase's verdict gate
        // (`schema={"pass":"bool",...}` -> `_acc.get("pass") == True`) can pass
        // under --dry-run instead of always failing on a "mock pass" string.
        let structured = spec.schema.as_ref().map(|schema| {
            let obj: serde_json::Map<String, serde_json::Value> = schema_required_keys(schema)
                .into_iter()
                .map(|key| {
                    let value = match schema.get(&key).and_then(|h| h.as_str()) {
                        Some("bool" | "boolean") => serde_json::Value::Bool(true),
                        Some("int" | "integer" | "number" | "float") => {
                            serde_json::Value::Number(0.into())
                        }
                        Some("array" | "list") => serde_json::Value::Array(vec![]),
                        Some("object" | "dict" | "map") => {
                            serde_json::Value::Object(serde_json::Map::new())
                        }
                        _ => serde_json::Value::String(format!("mock {key}")),
                    };
                    (key, value)
                })
                .collect();
            serde_json::Value::Object(obj)
        });
        return Ok(workflow::StepResult {
            phase: spec.phase.clone(),
            label: spec.label.clone(),
            provider: spec.provider.clone(),
            isolation: spec.isolation.clone(),
            ok: true,
            // Reuse the caller's session id so the mock terminal row matches the
            // `running` row's `native_session` (consistent in dry-run too).
            output_summary,
            // The journaling identity is assigned by the caller, which already
            // journaled the `running` start row before this step began.
            step_id: None,
            started_at: None,
            // No worker ran (dry-run), so there is no usage/exit telemetry; we
            // still surface the requested model so the dashboard can label it.
            details: Some(serde_json::json!({ "model": spec.model })),
            structured,
            ordinal: spec.ordinal,
        });
    }

    spawn_ephemeral_worker(store, options, spec, run_id, session_id)
}

/// RAII guard owning a harness-created throwaway worktree. Its `Drop` removes the
/// worktree (and any temp branch) no matter how the step exits — normal return,
/// `?` early-return, timeout, or panic — so a failed/timed-out node never leaks
/// an orphan (cleanup layer 2). The normal-path cleanup is the SAME code, just
/// triggered by the guard going out of scope at the end of a successful step.
pub(super) struct WorktreeGuard {
    /// Repo root the `git worktree` commands run against (`git -C <repo>`).
    pub(super) repo_root: PathBuf,
    /// Absolute path of the worktree checkout.
    pub(super) path: PathBuf,
    /// Temp branch created with the worktree, deleted alongside it.
    pub(super) branch: String,
}

/// The throwaway worktree's relative path and temp branch for one leaf, keyed by
/// run + node label + the per-leaf `session_id`. The `session_id` disambiguator is
/// what makes two SAME-LABEL writable nodes (e.g. a fan-out of workers all labeled
/// "fix") get DISTINCT worktrees instead of colliding on one branch+path — the
/// collision that made the 2nd+ such node fail with a cryptic "branch already
/// checked out" git error (issue #139 item 7).
pub(super) fn worktree_paths(run_id: &str, node_label: &str, session_id: &str) -> (String, String) {
    let slug = sanitize_worktree_slug(node_label);
    let unique = sanitize_worktree_slug(session_id);
    (
        format!(".harness/worktrees/{run_id}-{slug}-{unique}"),
        format!("harness/wt/{run_id}-{slug}-{unique}"),
    )
}

impl WorktreeGuard {
    /// `git -C <repo> worktree add -B <branch> <path> HEAD` — a detach-free
    /// throwaway checkout of HEAD the worker mutates in isolation. Uniform for
    /// both providers (the harness owns the worktree; we never use claude's -w).
    /// The branch+path are unique per LEAF (via `session_id`), so concurrent
    /// same-label writable nodes never collide (issue #139 item 7).
    pub(super) fn create(
        repo_root: &Path,
        run_id: &str,
        node_label: &str,
        session_id: &str,
    ) -> CliResult<WorktreeGuard> {
        // A writable / isolation="worktree" step runs in a throwaway git worktree.
        // If the workflow's cwd is NOT a git repo, `git worktree add` fails with a
        // cryptic "fatal: not a git repository". Catch that up front with an
        // actionable message (issue #89 item 5): the user either runs from a git
        // repo or keeps the step read-only and pulls the output via get-output.
        if !is_git_repo(repo_root) {
            return Err(CliError::Usage(format!(
                "node '{node_label}' needs an isolated git worktree (it is writable, \
                 or sets isolation=\"worktree\"), but {} is not a git repository. \
                 Either run the workflow from a git repo (e.g. `git init` there), or \
                 make this step READ-ONLY (drop writable / isolation) and retrieve its \
                 output with `harness workflow get-output <run_id> --step {node_label}`.",
                repo_root.display()
            )));
        }

        let (rel, branch) = worktree_paths(run_id, node_label, session_id);
        let path = repo_root.join(&rel);

        // Defensive: a stale dir from a crashed prior run would make `add` fail.
        if path.exists() {
            let _ = Command::new("git")
                .args([
                    "-C",
                    &repo_root.display().to_string(),
                    "worktree",
                    "remove",
                    "--force",
                ])
                .arg(&path)
                .output();
            let _ = fs::remove_dir_all(&path);
        }

        let output = Command::new("git")
            .args([
                "-C",
                &repo_root.display().to_string(),
                "worktree",
                "add",
                "-B",
                &branch,
            ])
            .arg(&path)
            .arg("HEAD")
            .output()?;
        if !output.status.success() {
            return Err(CliError::Usage(format!(
                "git worktree add failed for node {node_label}: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            )));
        }
        Ok(WorktreeGuard {
            repo_root: repo_root.to_path_buf(),
            path,
            branch,
        })
    }
}

impl Drop for WorktreeGuard {
    fn drop(&mut self) {
        // Bulletproof cleanup: remove the worktree and its temp branch however
        // the step exited. Best-effort — Drop must not panic — but `--force`
        // plus a manual dir sweep makes a leak very unlikely.
        let repo = self.repo_root.display().to_string();
        let _ = Command::new("git")
            .args(["-C", &repo, "worktree", "remove", "--force"])
            .arg(&self.path)
            .output();
        let _ = fs::remove_dir_all(&self.path);
        let _ = Command::new("git")
            .args(["-C", &repo, "branch", "-D", &self.branch])
            .output();
        // Prune any now-dangling administrative entry.
        let _ = Command::new("git")
            .args(["-C", &repo, "worktree", "prune"])
            .output();
    }
}

/// Map a node label to a filesystem-safe worktree slug (no `/`, spaces, etc.).
pub(super) fn sanitize_worktree_slug(label: &str) -> String {
    let slug: String = label
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let trimmed = slug.trim_matches('-');
    if trimmed.is_empty() {
        "node".to_string()
    } else {
        trimmed.to_string()
    }
}

/// Derive the [`ProjectContext`] a workflow run executes against.
///
/// A native Execution Space may name a default Project Binding in its metadata.
/// A project-derived compatibility store may still carry the old project
/// metadata. Raw overrides with neither form preserve the historical cwd
/// fallback.
///
/// BACK-COMPAT: a store with no `metadata.json` — a raw `--store <path>` /
/// `FIRM_ROOT` / legacy cwd-walk-up store — has no pinned project identity, so
/// we fall back to TODAY'S behavior exactly: `project_root` = the harness process
/// cwd (what `workflow_repo_root()` returned before), `store_root` = the store
/// root, git-ness probed live. This keeps existing serve + run-script flows
/// unchanged: a project only overrides the cwd when it was explicitly selected.
pub(super) fn workflow_project_context(store: &HarnessStore) -> ProjectContext {
    let store_root = store.root().to_path_buf();
    if let Ok(Some(space)) = execution_space::read_metadata(&store_root) {
        if let Some(binding_id) = space.default_project_binding_id.as_deref() {
            if let Ok(home) = project::firm_home() {
                if let Ok(Some(context)) = project::context_for_id(&home, binding_id) {
                    return context;
                }
            }
        }
    }
    if let Ok(Some(meta)) = project::read_metadata(&store_root) {
        return ProjectContext {
            id: meta.project_id,
            project_root: meta.canonical_path,
            store_root,
            kind: meta.kind,
            is_git_repo: meta.is_git_repo,
        };
    }
    // No pinned identity → preserve the historical cwd-as-repo-root behavior.
    let project_root = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let is_git_repo = is_git_repo(&project_root);
    ProjectContext {
        id: harness_core::GLOBAL_PROJECT_ID.to_string(),
        project_root,
        store_root,
        kind: ProjectKind::Repo,
        is_git_repo,
    }
}

pub(super) fn workflow_project_context_for_run(
    store: &HarnessStore,
    run_id: &str,
    explicit: Option<&ProjectContext>,
) -> CliResult<ProjectContext> {
    let binding_id = latest_workflow_runs_in_append_order(store)?
        .into_iter()
        .find(|run| run.id == run_id)
        .and_then(|run| run.project_binding_id);
    if let Some(binding_id) = binding_id {
        if let Some(context) = explicit {
            if context.id != binding_id {
                return Err(CliError::Usage(format!(
                    "workflow run {run_id} is pinned to Project Binding {binding_id}, not {}",
                    context.id
                )));
            }
        }
        let home = project::firm_home().map_err(project_err)?;
        if let Some(context) = project::context_for_id(&home, &binding_id).map_err(project_err)? {
            return Ok(context);
        }
        if let Some(context) = explicit {
            return Ok(context.clone());
        }
        return Err(CliError::Usage(format!(
            "workflow run {run_id} is pinned to unavailable Project Binding {binding_id}"
        )));
    }
    if let Some(context) = explicit {
        return Ok(context.clone());
    }
    Ok(workflow_project_context(store))
}

/// Resolve the repo root the worktrees are created under. The shared default
/// workspace is the run's project root (where CLAUDE.md / AGENTS.md / memory live
/// and the git repo is); worktrees live in the gitignored `.harness/worktrees/`
/// beneath it. This is the run's `project.project_root` — NOT the harness process
/// cwd, which a long-running `serve` never `cd`s after a project switch (P3).
pub(super) fn workflow_repo_root(project: &ProjectContext) -> PathBuf {
    project.project_root.clone()
}

/// Resolve the cwd a PERSISTENT provider delivery (codex / claude) runs from
/// (goal-multi-project P3, Stage 3). Precedence:
///   1. `member.provider_cwd_hint` — an explicitly pinned workspace always wins.
///   2. `project.project_root` — the SELECTED project's root, so the worker reads
///      the right `CLAUDE.md` / `AGENTS.md` / `.claude/` even when a long-running
///      `serve` switched projects and never `cd`d.
///   3. `env::current_dir()` — last-resort compatibility fallback (a raw
///      `--store`/`FIRM_ROOT` store with no pinned identity degrades to today's
///      behavior; see `workflow_project_context`).
///
/// Returns a display string (the `Command::current_dir` callers already pass a
/// string) defaulting to `"."` only if even the process cwd is unreadable.
pub(super) fn delivery_worker_cwd(member: &ProviderLaunchProfile, project: &ProjectContext) -> String {
    if let Some(worktree) = member.provider_cwd_hint.clone() {
        return worktree;
    }
    let project_root = project.project_root.as_path();
    if !project_root.as_os_str().is_empty() {
        return project_root.display().to_string();
    }
    env::current_dir()
        .ok()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| ".".to_string())
}

/// Whether `path` is inside a git work tree — `git -C <path> rev-parse
/// --is-inside-work-tree` exits 0 and prints `true`. Used to fail a
/// writable/isolated workflow step with a clear message BEFORE attempting a
/// `git worktree add` that would otherwise error cryptically (issue #89 item 5).
pub(super) fn is_git_repo(path: &Path) -> bool {
    Command::new("git")
        .arg("-C")
        .arg(path)
        .args(["rev-parse", "--is-inside-work-tree"])
        .output()
        .map(|out| out.status.success())
        .unwrap_or(false)
}

/// Does `provider`'s exec mode PHYSICALLY enforce read-only (so a non-writable
/// leaf cannot mutate its cwd)? codex (`--sandbox read-only`) and claude (a
/// read-only tool allowlist `Read,Grep,Glob`) do; kimi's headless `kimi -p`
/// rejects every permission flag, so it does NOT. This remains provider
/// capability metadata; read-only workflow cwd routing is controlled by
/// [`step_needs_isolation`].
#[cfg(test)]
pub(super) fn provider_enforces_read_only(provider: &str) -> bool {
    provider_adapter(provider)
        .map(|a| a.capabilities().enforces_read_only)
        .unwrap_or(false)
}

pub(super) fn step_write_mode_direct(spec: &workflow::AgentStepSpec) -> bool {
    spec.write_mode.as_deref() == Some(workflow::WRITE_MODE_DIRECT)
}

/// Whether an ephemeral leaf must run in a throwaway git worktree instead of the
/// shared repo cwd. A leaf isolates when it explicitly opts into
/// `isolation="worktree"`, when it is `writable` (edits must land in a discardable
/// checkout). Read-only leaves stay in the selected project root even if a
/// provider cannot physically enforce read-only (#190); provider capability gaps
/// should not silently turn a read-only scan/review into a git-worktree
/// requirement. `write_mode="direct"` writes the shared project root in place, so
/// it never isolates either.
pub(super) fn step_needs_isolation(writable: bool, isolation: Option<&str>, write_mode: Option<&str>) -> bool {
    if write_mode == Some(workflow::WRITE_MODE_DIRECT) {
        return false;
    }
    isolation == Some("worktree") || writable
}

pub(super) fn direct_write_diff(repo_root: &Path) -> Option<String> {
    let repo = repo_root.display().to_string();
    let mut diff =
        command_stdout("git", &["-C", &repo, "diff", "--no-ext-diff", "HEAD"]).unwrap_or_default();
    let untracked = command_stdout(
        "git",
        &["-C", &repo, "ls-files", "--others", "--exclude-standard"],
    )
    .unwrap_or_default();
    for path in untracked.lines().map(str::trim).filter(|p| !p.is_empty()) {
        let abs = repo_root.join(path);
        let Ok(bytes) = fs::read(&abs) else {
            continue;
        };
        let text = String::from_utf8_lossy(&bytes);
        diff.push_str(&format!(
            "diff --git a/{path} b/{path}\nnew file mode 100644\n--- /dev/null\n+++ b/{path}\n@@ -0,0 +1,{} @@\n",
            text.lines().count().max(1)
        ));
        for line in text.lines() {
            diff.push('+');
            diff.push_str(line);
            diff.push('\n');
        }
        if text.is_empty() {
            diff.push_str("+\n");
        }
    }
    Some(diff)
}

pub(super) fn ensure_direct_write_ready(
    project: &ProjectContext,
    repo_root: &Path,
    spec: &workflow::AgentStepSpec,
) -> CliResult<()> {
    if !spec.writable {
        return Err(CliError::Usage(format!(
            "node '{}' sets write_mode=\"direct\" but is not writable. Direct shared-repo edits require writable=True so the provider receives edit permissions.",
            spec.label
        )));
    }
    if spec.isolation.as_deref() == Some("worktree") {
        return Err(CliError::Usage(format!(
            "node '{}' sets both write_mode=\"direct\" and isolation=\"worktree\". Choose direct shared-repo writes or an isolated worktree, not both.",
            spec.label
        )));
    }
    if !project.is_git_repo {
        return Err(CliError::Usage(format!(
            "node '{}' sets write_mode=\"direct\", but project '{}' ({}) is not a git repository. Direct writes require a git-backed project so the harness can attribute the resulting diff.",
            spec.label,
            project.id,
            repo_root.display()
        )));
    }
    let status = git_in(repo_root, &["status", "--porcelain"])?;
    if !status.trim().is_empty() {
        return Err(CliError::Usage(format!(
            "node '{}' sets write_mode=\"direct\", but {} has uncommitted changes before the step:\n{}\nDirect writes require a clean repo so the harness can attribute the resulting diff.",
            spec.label,
            repo_root.display(),
            status.trim()
        )));
    }
    Ok(())
}
