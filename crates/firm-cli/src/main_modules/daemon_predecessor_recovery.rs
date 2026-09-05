use super::*;

/// One validated predecessor-recovery intent: the exact latest predecessor
/// lease this Node may settle.
///
/// This module is the shared validate+recover seam for one exact unreleased
/// predecessor NodeDaemonLease. The Operator HTTP role action
/// (role_actions_api/operator_actions.rs) and the
/// `daemon recover-predecessor` CLI (main_modules/daemon_cli.rs) perform the
/// identical checks and store transitions through these two functions; neither
/// may grow a second copy of the pid probe or the per-Space recovery loop.
pub(crate) struct PredecessorRecoveryIntent {
    pub daemon_id: String,
    pub instance_id: String,
    pub generation: u64,
}

/// Standard process-existence probe for a predecessor instance id of the form
/// `<pid>:<boot token>:<daemon label>`. EPERM still proves that a process
/// exists; only ESRCH is accepted as absence.
pub(crate) fn predecessor_process_is_absent(instance_id: &str) -> Result<bool, String> {
    let pid = instance_id
        .split(':')
        .next()
        .ok_or_else(|| "predecessor instance id has no process id".to_string())?
        .parse::<i32>()
        .map_err(|_| "predecessor instance id does not begin with a process id".to_string())?;
    if pid <= 0 {
        return Err("predecessor process id must be positive".into());
    }
    // SAFETY: kill(pid, 0) sends no signal and is the standard process
    // existence probe.
    let result = unsafe { libc::kill(pid, 0) };
    if result == 0 {
        return Ok(false);
    }
    match std::io::Error::last_os_error().raw_os_error() {
        Some(libc::ESRCH) => Ok(true),
        Some(libc::EPERM) => Ok(false),
        Some(code) => Err(format!(
            "cannot verify predecessor process {pid}: errno {code}"
        )),
        None => Err(format!("cannot verify predecessor process {pid}")),
    }
}

/// Validate that this Node has one exact latest predecessor lease (matching
/// `expected` when the caller carries an HTTP-bound intent), that no live
/// NodeDaemon socket remains, and that the predecessor process is proven
/// absent. Errors are `(code, detail)` pairs the caller wraps in its own
/// envelope (HTTP role-action error vs CLI usage error).
pub(crate) fn validate_daemon_predecessor_recovery(
    firm_home: &Path,
    node_id: &str,
    expected: Option<(&str, &str, u64)>,
) -> Result<PredecessorRecoveryIntent, (String, String)> {
    if supervisor_daemon::daemon_status_via_socket(firm_home, node_id).is_some() {
        return Err((
            "NODE_DAEMON_PREDECESSOR_RECOVERY_LIVE".into(),
            "a NodeDaemon socket is still live; Stop it before predecessor recovery".into(),
        ));
    }
    let mut latest: Option<harness_core::NodeDaemonLease> = None;
    for space in execution_space::list_spaces(firm_home).map_err(|error| {
        execution_space_error_pair("NODE_DAEMON_PREDECESSOR_RECOVERY_INCOMPLETE", error)
    })? {
        let store = HarnessStore::new(space.store_root);
        if let Some(lease) = store.latest_node_daemon_lease(node_id).map_err(|error| {
            (
                "NODE_DAEMON_PREDECESSOR_RECOVERY_INCOMPLETE".to_string(),
                format!("{}: {error}", space.id),
            )
        })? {
            if latest
                .as_ref()
                .is_none_or(|current| lease.generation > current.generation)
            {
                latest = Some(lease);
            }
        }
    }
    let latest = latest.ok_or_else(|| {
        (
            "SUPERVISOR_GENERATION_FENCED".to_string(),
            "Node has no predecessor lease to recover".to_string(),
        )
    })?;
    if let Some((daemon_id, instance_id, generation)) = expected {
        if latest.daemon_id != daemon_id
            || latest.instance_id != instance_id
            || latest.generation != generation
        {
            return Err((
                "SUPERVISOR_GENERATION_FENCED".into(),
                "recovery intent does not match the exact latest predecessor".into(),
            ));
        }
    }
    if !predecessor_process_is_absent(&latest.instance_id).map_err(|error| {
        (
            "NODE_DAEMON_PREDECESSOR_RECOVERY_UNVERIFIED".to_string(),
            error,
        )
    })? {
        return Err((
            "NODE_DAEMON_PREDECESSOR_RECOVERY_LIVE".into(),
            "the exact predecessor process still exists".into(),
        ));
    }
    Ok(PredecessorRecoveryIntent {
        daemon_id: latest.daemon_id,
        instance_id: latest.instance_id,
        generation: latest.generation,
    })
}

/// Perform the per-Space `recover_node_daemon_predecessor` transition for
/// every Execution Space belonging to this Node and return the recovery
/// projection. `provider_process_groups_terminated_confirmed` is the
/// Operator's external-fact confirmation; the CLI passes `true` after its own
/// pid probe, the HTTP action passes its reviewed request field.
#[allow(clippy::too_many_arguments)]
pub(crate) fn recover_daemon_predecessor_spaces(
    firm_home: &Path,
    node_id: &str,
    intent: &PredecessorRecoveryIntent,
    actor: &harness_core::agentfirm_api::ActorRef,
    provider_process_groups_terminated_confirmed: bool,
    evidence_ref: &str,
    idempotency_key_prefix: &str,
    request_fingerprint: Option<String>,
) -> Result<serde_json::Value, (String, String)> {
    let spaces = execution_space::list_spaces(firm_home).map_err(|error| {
        execution_space_error_pair("NODE_DAEMON_PREDECESSOR_RECOVERY_INCOMPLETE", error)
    })?;
    let mut recovered_spaces = Vec::new();
    let mut failures = Vec::new();
    for space in spaces {
        let scoped = HarnessStore::new(space.store_root.clone());
        let belongs_to_node = scoped
            .latest_execution_nodes()
            .map(|nodes| nodes.into_iter().any(|node| node.id == node_id));
        match belongs_to_node {
            Ok(false) => continue,
            Err(error) => {
                failures.push(format!("{}: {error}", space.id));
                continue;
            }
            Ok(true) => {}
        }
        let context = harness_core::agentfirm_api::MutationContext {
            execution_space_id: space.id.clone(),
            authenticated_actor: actor.clone(),
            authority_actor: None,
            command_name: "node_daemon.predecessor_recover".into(),
            idempotency_key: format!("{idempotency_key_prefix}:space:{}", space.id),
            expected_version: intent.generation,
            request_fingerprint: request_fingerprint.clone(),
        };
        match scoped.recover_node_daemon_predecessor(
            &context,
            node_id,
            &intent.daemon_id,
            intent.generation,
            &intent.instance_id,
            true,
            provider_process_groups_terminated_confirmed,
            evidence_ref,
            current_unix_ms_u64(),
            &format!("unix-ms:{}", current_unix_ms_u64()),
        ) {
            Ok(_) => recovered_spaces.push(space.id),
            Err(error) => failures.push(format!("{}: {error}", space.id)),
        }
    }
    if !failures.is_empty() {
        return Err((
            "NODE_DAEMON_PREDECESSOR_RECOVERY_INCOMPLETE".into(),
            failures.join("; "),
        ));
    }
    Ok(serde_json::json!({
        "node_id": node_id,
        "daemon_id": intent.daemon_id,
        "instance_id": intent.instance_id,
        "generation": intent.generation,
        "status": "released",
        "recovered_spaces": recovered_spaces,
        "evidence_ref": evidence_ref,
    }))
}

fn execution_space_error_pair(
    code: &str,
    error: execution_space::ExecutionSpaceError,
) -> (String, String) {
    (code.to_string(), error.to_string())
}
