use super::*;

/// One validated predecessor instance with exact Space-local lease tuples.
///
/// This module is the shared validate+recover seam for captured predecessor
/// NodeDaemonLease tuples. The Operator HTTP role action
/// (role_actions_api/operator_actions.rs) and the
/// `daemon recover-predecessor` CLI (main_modules/daemon_cli.rs) perform the
/// identical checks and store transitions through these two functions; neither
/// may grow a second copy of the pid probe or the per-Space recovery loop.
pub(crate) struct PredecessorRecoveryIntent {
    pub daemon_id: String,
    pub instance_id: String,
    pub spaces: Vec<(harness_core::ExecutionSpace, harness_core::NodeDaemonLease)>,
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

/// Capture exact latest Space-local leases of one predecessor instance
/// (restricted to `expected` for an HTTP-bound intent), prove that no live
/// NodeDaemon socket remains, and that the predecessor process is proven
/// absent. Errors are `(code, detail)` pairs the caller wraps in its own
/// envelope (HTTP role-action error vs CLI usage error).
pub(crate) fn validate_daemon_predecessor_recovery(
    firm_home: &Path,
    node_id: &str,
    expected: Option<(&str, &str, u64)>,
) -> Result<PredecessorRecoveryIntent, (String, String)> {
    if daemon_client::daemon_status_via_socket(firm_home, node_id).is_some() {
        return Err((
            "NODE_DAEMON_PREDECESSOR_RECOVERY_LIVE".into(),
            "a NodeDaemon socket is still live; Stop it before predecessor recovery".into(),
        ));
    }
    let mut leases = Vec::new();
    for space in execution_space::list_spaces(firm_home).map_err(|error| {
        execution_space_error_pair("NODE_DAEMON_PREDECESSOR_RECOVERY_INCOMPLETE", error)
    })? {
        let store = HarnessStore::new(space.store_root.clone());
        if let Some(lease) = store.latest_node_daemon_lease(node_id).map_err(|error| {
            (
                "NODE_DAEMON_PREDECESSOR_RECOVERY_INCOMPLETE".into(),
                format!("{}: {error}", space.id),
            )
        })? {
            leases.push((space, lease));
        }
    }
    // Generations are Space-local counters, never a machine-wide ordering.
    let latest = leases
        .iter()
        .find(|(_, lease)| lease.status != NodeDaemonLeaseStatus::Released)
        .or_else(|| leases.first())
        .map(|(_, lease)| lease.clone())
        .ok_or_else(|| {
            (
                "SUPERVISOR_GENERATION_FENCED".into(),
                "Node has no predecessor lease to recover".into(),
            )
        })?;
    if leases.iter().any(|(_, lease)| {
        lease.status != NodeDaemonLeaseStatus::Released
            && (lease.daemon_id != latest.daemon_id || lease.instance_id != latest.instance_id)
    }) {
        return Err(("SUPERVISOR_GENERATION_FENCED".into(), "Node has different unreleased predecessor instances; recovery must not sweep unrelated instances".into()));
    }
    // The existing HTTP request authorizes exactly one tuple. A same-instance
    // lease with another local generation is outside that request's scope.
    let spaces: Vec<_> = leases
        .into_iter()
        .filter(|(_, lease)| {
            if let Some((daemon, instance, generation)) = expected {
                lease.daemon_id == daemon
                    && lease.instance_id == instance
                    && lease.generation == generation
            } else {
                lease.daemon_id == latest.daemon_id && lease.instance_id == latest.instance_id
            }
        })
        .collect();
    if spaces.is_empty()
        || expected.is_some_and(|(daemon, instance, _)| {
            daemon != latest.daemon_id || instance != latest.instance_id
        })
    {
        return Err((
            "SUPERVISOR_GENERATION_FENCED".into(),
            "recovery intent does not match the exact latest predecessor".into(),
        ));
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
        spaces,
    })
}

/// Perform the per-Space `recover_node_daemon_predecessor` transition for
/// each captured Execution Space and return the recovery
/// projection. `provider_process_groups_terminated_confirmed` is the
/// Operator's external-fact confirmation; the CLI passes `true` after its own
/// pid probe, the HTTP action passes its reviewed request field.
/// On partial failure, the error detail is a JSON receipt retaining successful
/// settlements; both CLI and HTTP preserve this detail in their error envelope.
/// `evidence_ref` identifies this request. Repeating recovery does not replace
/// the evidence ref on rows settled by an earlier successful attempt.
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
    // Re-probe process absence immediately before any settlement, then use
    // only the captured Space/lease tuples. The Store fences each tuple under
    // its write lock, including successors arriving after validation.
    if daemon_client::daemon_status_via_socket(firm_home, node_id).is_some()
        || !predecessor_process_is_absent(&intent.instance_id)
            .map_err(|error| ("NODE_DAEMON_PREDECESSOR_RECOVERY_UNVERIFIED".into(), error))?
    {
        return Err((
            "NODE_DAEMON_PREDECESSOR_RECOVERY_LIVE".into(),
            "the exact predecessor process or NodeDaemon socket is still live".into(),
        ));
    }
    let mut recovered_spaces = Vec::new();
    let mut space_settlements = Vec::new();
    let mut failures = Vec::new();
    for (space, lease) in &intent.spaces {
        let scoped = HarnessStore::new(space.store_root.clone());
        let context = harness_core::agentfirm_api::MutationContext {
            execution_space_id: space.id.clone(),
            authenticated_actor: actor.clone(),
            authority_actor: None,
            command_name: "node_daemon.predecessor_recover".into(),
            idempotency_key: format!("{idempotency_key_prefix}:space:{}", space.id),
            expected_version: lease.generation,
            request_fingerprint: request_fingerprint.clone(),
        };
        match scoped.recover_node_daemon_predecessor(
            &context,
            node_id,
            &intent.daemon_id,
            lease.generation,
            &intent.instance_id,
            true,
            provider_process_groups_terminated_confirmed,
            evidence_ref,
            current_unix_ms_u64(),
            &format!("unix-ms:{}", current_unix_ms_u64()),
        ) {
            // The settlement summary is part of the receipt: an operator must
            // be able to see which Sessions this recovery detached and which
            // it skipped because the dying generation had already settled them
            // (#837), not infer the difference from silence.
            Ok(recovery) => {
                space_settlements.push(serde_json::json!({
                    "execution_space_id": space.id,
                    "generation": lease.generation,
                    "daemon_id": lease.daemon_id,
                    "instance_id": lease.instance_id,
                    "already_released": recovery.already_released,
                    "supervisors_released": recovery.supervisors_released,
                    "sessions_detached": recovery.sessions_detached,
                    "sessions_already_settled": recovery.sessions_already_settled,
                }));
                recovered_spaces.push(space.id.clone());
            }
            Err(error) => failures.push(format!("{}: {error}", space.id)),
        }
    }
    let mut receipt = serde_json::json!({
        "node_id": node_id,
        "daemon_id": intent.daemon_id,
        "instance_id": intent.instance_id,
        "generation": intent.spaces.first().map(|(_, lease)| lease.generation).filter(|generation| intent.spaces.iter().all(|(_, lease)| lease.generation == *generation)),
        "already_released": failures.is_empty() && space_settlements.iter().all(|row| row["already_released"] == true),
        "space_leases": intent.spaces.iter().map(|(space, lease)| serde_json::json!({
            "execution_space_id": space.id, "daemon_id": lease.daemon_id,
            "instance_id": lease.instance_id, "generation": lease.generation,
        })).collect::<Vec<_>>(),
        "status": "released",
        "recovered_spaces": recovered_spaces,
        "space_settlements": space_settlements,
        "evidence_ref": evidence_ref,
    });
    if !failures.is_empty() {
        receipt["status"] = serde_json::json!("partial");
        receipt["failures"] = serde_json::json!(failures);
        return Err((
            "NODE_DAEMON_PREDECESSOR_RECOVERY_INCOMPLETE".into(),
            receipt.to_string(),
        ));
    }
    Ok(receipt)
}

fn execution_space_error_pair(
    code: &str,
    error: execution_space::ExecutionSpaceError,
) -> (String, String) {
    (code.to_string(), error.to_string())
}
