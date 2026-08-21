use super::*;

pub(super) fn start_agent_runtime(
    store: &HarnessStore,
    agent_id: &str,
) -> CliResult<ProviderLaunchProfile> {
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

pub(super) struct DeliveryOptions {
    pub(super) agent_id: String,
    pub(super) message_filter: Option<String>,
    pub(super) dry_run: bool,
    pub(super) start_runtime: bool,
    pub(super) timeout_ms: u64,
}

/// Resolve a compatibility Project Binding for legacy provider-delivery commands.
/// Execution Space selection and provider workspace selection remain independent.
pub(super) fn default_project_context(store: &HarnessStore) -> ProjectContext {
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

pub(super) fn delivery_worker_cwd(
    member: &ProviderLaunchProfile,
    project: &ProjectContext,
) -> String {
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
