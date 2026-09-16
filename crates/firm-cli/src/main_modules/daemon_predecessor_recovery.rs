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
    /// The latest moment the predecessor was known to be alive: the exact
    /// lease renewal the death proof is anchored to.
    pub anchor_unix_ms: u64,
    pub spaces: Vec<(harness_core::ExecutionSpace, harness_core::NodeDaemonLease)>,
}

/// Standard process-death probe for a predecessor instance id of the form
/// `<pid>:<minted unix-ms>:<daemon label>`, anchored at the last moment the
/// predecessor was known to be alive.
///
/// The proof itself belongs to `harness_runtime_host::predecessor_process`, so
/// this CLI path and the successor daemon's automatic recovery answer the
/// death question with one implementation instead of two (ADR 0073).
pub(crate) fn predecessor_process_proof(
    instance_id: &str,
    anchor_unix_ms: u64,
) -> Result<harness_runtime_host::PredecessorProcessProof, String> {
    harness_runtime_host::probe_predecessor_process(
        instance_id,
        anchor_unix_ms,
        current_unix_ms_u64(),
    )
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
        // Bound to the Firm home this verb was invoked for: after ADR 0075 the
        // machine lease is named by `(FIRM_HOME, node_id)`, and a Store opened
        // by bare path would fall back to a directory derived from the Space
        // root's shape — a different document whenever a registered root lives
        // outside the home.
        let store = HarnessStore::new(space.store_root.clone()).with_firm_home(firm_home);
        if let Some(lease) = store
            .current_authorized_machine_lease(node_id)
            .map_err(|error| {
                (
                    "NODE_DAEMON_PREDECESSOR_RECOVERY_INCOMPLETE".into(),
                    format!("{}: {error}", space.id),
                )
            })?
        {
            leases.push((space, lease));
        }
    }
    // The existing HTTP request authorizes exactly one tuple. Selection of the
    // exact predecessor instance — including the refusal to sweep a second
    // unreleased instance — belongs to the shared Store seam, so this path and
    // the successor daemon's automatic recovery choose identically (ADR 0073).
    let (latest, spaces) = harness_store::select_exact_predecessor_spaces(leases, expected)?;
    let anchor_unix_ms = latest.renewed_unix_ms;
    let proof =
        predecessor_process_proof(&latest.instance_id, anchor_unix_ms).map_err(|error| {
            (
                "NODE_DAEMON_PREDECESSOR_RECOVERY_UNVERIFIED".to_string(),
                error,
            )
        })?;
    if !proof.counts_as_absent() {
        return Err((
            "NODE_DAEMON_PREDECESSOR_RECOVERY_LIVE".into(),
            format!(
                "the exact predecessor process still exists ({}; {})",
                proof.reason(),
                proof.evidence
            ),
        ));
    }
    Ok(PredecessorRecoveryIntent {
        daemon_id: latest.daemon_id,
        instance_id: latest.instance_id,
        anchor_unix_ms,
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
    let proof =
        predecessor_process_proof(&intent.instance_id, intent.anchor_unix_ms).map_err(|error| {
            (
                "NODE_DAEMON_PREDECESSOR_RECOVERY_UNVERIFIED".to_string(),
                error,
            )
        })?;
    if daemon_client::daemon_status_via_socket(firm_home, node_id).is_some()
        || !proof.counts_as_absent()
    {
        return Err((
            "NODE_DAEMON_PREDECESSOR_RECOVERY_LIVE".into(),
            format!(
                "the exact predecessor process or NodeDaemon socket is still live ({})",
                proof.reason()
            ),
        ));
    }
    let mut receipt = harness_store::recover_predecessor_generation_across_spaces(
        node_id,
        &intent.daemon_id,
        &intent.instance_id,
        &intent
            .spaces
            .iter()
            .map(|(space, lease)| harness_store::PredecessorSpaceLease {
                execution_space_id: space.id.clone(),
                store: HarnessStore::new(space.store_root.clone()).with_firm_home(firm_home),
                lease: lease.clone(),
            })
            .collect::<Vec<_>>(),
        actor,
        provider_process_groups_terminated_confirmed,
        evidence_ref,
        idempotency_key_prefix,
        request_fingerprint,
        current_unix_ms_u64(),
    )?;
    receipt["process_death_proof"] = proof.to_receipt_json();
    Ok(receipt)
}

fn execution_space_error_pair(
    code: &str,
    error: execution_space::ExecutionSpaceError,
) -> (String, String) {
    (code.to_string(), error.to_string())
}
