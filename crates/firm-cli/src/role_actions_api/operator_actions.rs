use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum OperatorActionJournalState {
    Prepared,
    InFlight,
    Completed,
    RecoveryRequired,
}

#[derive(Debug, Serialize, Deserialize)]
pub(super) struct OperatorActionReceipt {
    pub(super) request_fingerprint: String,
    pub(super) state: OperatorActionJournalState,
    #[serde(default)]
    pub(super) projection: Option<serde_json::Value>,
    #[serde(default)]
    pub(super) event_id: Option<String>,
    #[serde(default)]
    pub(super) resulting_version: Option<u64>,
    #[serde(default)]
    pub(super) store_sequence: Option<u64>,
    #[serde(default)]
    pub(super) recovery_detail: Option<String>,
}

pub(super) fn operator_receipt_paths(
    firm_home: &std::path::Path,
    node_id: &str,
    auth: &AuthenticatedMutation,
) -> Result<(std::path::PathBuf, std::path::PathBuf, String), StoreError> {
    let request_fingerprint = auth.request_fingerprint.clone().ok_or_else(|| {
        encoded_error(
            "INVALID_STATE_TRANSITION",
            "Operator action is missing its server-bound request fingerprint",
            "execution_node",
            node_id,
            None,
        )
    })?;
    let receipt_root = firm_home
        .join("runtime")
        .join("operator-action-receipts")
        .join(node_id);
    let receipt_id = canonical_json_fingerprint(&json!({
        "node_id": node_id,
        "idempotency_key": auth.idempotency_key,
    }));
    Ok((
        receipt_root.join(format!("{receipt_id}.json")),
        receipt_root.join(format!("{receipt_id}.lock")),
        request_fingerprint,
    ))
}

pub(super) fn operator_journal_result(
    receipt: OperatorActionReceipt,
    node_id: &str,
) -> Result<Option<RoleActionResult>, StoreError> {
    match receipt.state {
        OperatorActionJournalState::Completed => Ok(Some(RoleActionResult {
            ok: true,
            action_protocol_version: "agentfirm.role_actions.v1",
            projection: receipt.projection.ok_or_else(|| {
                encoded_error(
                    "RECOVERY_REQUIRED",
                    "completed Operator journal is missing its projection",
                    "execution_node",
                    node_id,
                    receipt.resulting_version,
                )
            })?,
            event_id: receipt.event_id.ok_or_else(|| {
                encoded_error(
                    "RECOVERY_REQUIRED",
                    "completed Operator journal is missing its event id",
                    "execution_node",
                    node_id,
                    receipt.resulting_version,
                )
            })?,
            resulting_version: receipt.resulting_version.unwrap_or_default(),
            store_sequence: receipt.store_sequence.unwrap_or_default(),
            replayed: true,
        })),
        OperatorActionJournalState::InFlight | OperatorActionJournalState::RecoveryRequired => {
            Err(encoded_error(
                "RECOVERY_REQUIRED",
                receipt.recovery_detail.unwrap_or_else(|| {
                    "prior Operator request may have crossed the external-effect boundary; reconcile before retrying".into()
                }),
                "execution_node",
                node_id,
                receipt.resulting_version,
            ))
        }
        OperatorActionJournalState::Prepared => Ok(None),
    }
}

pub(super) fn read_operator_receipt(
    receipt_path: &std::path::Path,
    node_id: &str,
) -> Result<OperatorActionReceipt, StoreError> {
    let bytes = std::fs::read(receipt_path).map_err(|error| {
        encoded_error(
            "RECOVERY_REQUIRED",
            format!("Operator journal cannot be read safely: {error}"),
            "execution_node",
            node_id,
            None,
        )
    })?;
    serde_json::from_slice(&bytes).map_err(|error| {
        encoded_error(
            "RECOVERY_REQUIRED",
            format!("Operator journal is torn or invalid: {error}"),
            "execution_node",
            node_id,
            None,
        )
    })
}

pub(super) fn replay_receipted_operator_action(
    firm_home: &std::path::Path,
    node_id: &str,
    auth: &AuthenticatedMutation,
) -> Result<Option<RoleActionResult>, StoreError> {
    let (receipt_path, lock_path, request_fingerprint) =
        operator_receipt_paths(firm_home, node_id, auth)?;
    if !receipt_path.exists() {
        return Ok(None);
    }
    let lock_file = std::fs::OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(&lock_path)?;
    lock_file.lock()?;
    let receipt = read_operator_receipt(&receipt_path, node_id)?;
    if receipt.request_fingerprint != request_fingerprint {
        return Err(encoded_error(
            "IDEMPOTENCY_CONFLICT",
            "idempotency key was already bound to a different Operator action fingerprint",
            "execution_node",
            node_id,
            receipt.resulting_version,
        ));
    }
    operator_journal_result(receipt, node_id)
}

pub(super) fn execute_receipted_operator_action<F>(
    firm_home: &std::path::Path,
    node_id: &str,
    auth: &AuthenticatedMutation,
    execute: F,
) -> Result<RoleActionResult, StoreError>
where
    F: FnOnce() -> Result<RoleActionResult, StoreError>,
{
    let (receipt_path, lock_path, request_fingerprint) =
        operator_receipt_paths(firm_home, node_id, auth)?;
    let receipt_root = receipt_path.parent().ok_or_else(|| {
        encoded_error(
            "ACTION_UNAVAILABLE",
            "Operator journal path has no parent",
            "execution_node",
            node_id,
            None,
        )
    })?;
    std::fs::create_dir_all(receipt_root).map_err(|error| {
        encoded_error(
            "ACTION_UNAVAILABLE",
            format!("cannot create Operator receipt directory: {error}"),
            "execution_node",
            node_id,
            None,
        )
    })?;
    let lock_file = std::fs::OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(&lock_path)
        .map_err(|error| {
            encoded_error(
                "ACTION_UNAVAILABLE",
                format!("cannot open Operator receipt lock: {error}"),
                "execution_node",
                node_id,
                None,
            )
        })?;
    lock_file.lock().map_err(|error| {
        encoded_error(
            "ACTION_UNAVAILABLE",
            format!("cannot lock Operator receipt: {error}"),
            "execution_node",
            node_id,
            None,
        )
    })?;
    if receipt_path.exists() {
        let receipt = read_operator_receipt(&receipt_path, node_id)?;
        if receipt.request_fingerprint != request_fingerprint {
            return Err(encoded_error(
                "IDEMPOTENCY_CONFLICT",
                "idempotency key was already committed with a different Operator action fingerprint",
                "execution_node",
                node_id,
                receipt.resulting_version,
            ));
        }
        if let Some(result) = operator_journal_result(receipt, node_id)? {
            return Ok(result);
        }
    }
    let write = |receipt: &OperatorActionReceipt| -> Result<(), StoreError> {
        crate::execution_space::atomic_write_bytes(
            &receipt_path,
            &serde_json::to_vec_pretty(receipt)?,
        )
        .map_err(|error| {
            encoded_error(
                "ACTION_UNAVAILABLE",
                format!("cannot commit Operator action journal: {error}"),
                "execution_node",
                node_id,
                receipt.resulting_version,
            )
        })
    };
    write(&OperatorActionReceipt {
        request_fingerprint: request_fingerprint.clone(),
        state: OperatorActionJournalState::Prepared,
        projection: None,
        event_id: None,
        resulting_version: None,
        store_sequence: None,
        recovery_detail: None,
    })?;
    write(&OperatorActionReceipt {
        request_fingerprint: request_fingerprint.clone(),
        state: OperatorActionJournalState::InFlight,
        projection: None,
        event_id: None,
        resulting_version: None,
        store_sequence: None,
        recovery_detail: Some(
            "external effect was started but no durable completion receipt exists".into(),
        ),
    })?;
    let result = execute().map_err(|error| {
        let recovery = OperatorActionReceipt {
            request_fingerprint: request_fingerprint.clone(),
            state: OperatorActionJournalState::RecoveryRequired,
            projection: None,
            event_id: None,
            resulting_version: None,
            store_sequence: None,
            recovery_detail: Some(format!(
                "Operator external effect returned without a provable completion receipt: {error}"
            )),
        };
        let _ = write(&recovery);
        encoded_error(
            "RECOVERY_REQUIRED",
            recovery.recovery_detail.unwrap_or_default(),
            "execution_node",
            node_id,
            None,
        )
    })?;
    write(&OperatorActionReceipt {
        request_fingerprint,
        state: OperatorActionJournalState::Completed,
        projection: Some(result.projection.clone()),
        event_id: Some(result.event_id.clone()),
        resulting_version: Some(result.resulting_version),
        store_sequence: Some(result.store_sequence),
        recovery_detail: None,
    })
    .map_err(|error| {
        encoded_error(
            "RECOVERY_REQUIRED",
            format!(
                "external effect completed but its durable completion receipt could not be committed: {error}"
            ),
            "execution_node",
            node_id,
            Some(result.resulting_version),
        )
    })?;
    Ok(result)
}

pub(super) fn execute_operator_action(
    store: &HarnessStore,
    auth: AuthenticatedMutation,
    node_id: &str,
    operation: &str,
    body: &[u8],
    confirmed_action: Option<&str>,
) -> Result<RoleActionResult, StoreError> {
    if auth.actor.kind != ActorKind::Service || auth.actor.id != node_id {
        return Err(encoded_error(
            "UNAUTHORIZED_ACTOR",
            "Operator credential is not the exact Execution Node Service",
            "execution_node",
            node_id,
            None,
        ));
    }
    let intent = serde_json::from_slice::<OperatorActionIntent>(body).map_err(|error| {
        encoded_error(
            "INVALID_STATE_TRANSITION",
            format!("invalid Operator intent: {error}"),
            "execution_node",
            node_id,
            None,
        )
    })?;
    let intent_matches = matches!(
        (operation, &intent),
        ("diagnostics", OperatorActionIntent::Diagnose)
            | ("daemon-start", OperatorActionIntent::DaemonStart { .. })
            | ("daemon-stop", OperatorActionIntent::DaemonStop { .. })
            | (
                "daemon-recover-predecessor",
                OperatorActionIntent::RecoverDaemonPredecessor { .. }
            )
            | (
                "provider-admission",
                OperatorActionIntent::AdmitProvider { .. }
            )
    );
    if !intent_matches {
        return Err(encoded_error(
            "INVALID_STATE_TRANSITION",
            "semantic action does not match Operator route",
            "execution_node",
            node_id,
            None,
        ));
    }
    let daemon_action = matches!(
        operation,
        "daemon-start" | "daemon-stop" | "daemon-recover-predecessor"
    );
    let receipted_action = daemon_action || operation == "provider-admission";
    if daemon_action && confirmed_action != Some(operation) {
        return Err(encoded_error(
            "CONFIRMATION_REQUIRED",
            format!("server confirmation must exactly confirm {operation}"),
            "execution_node",
            node_id,
            None,
        ));
    }
    let firm_home = receipted_action
        .then(crate::execution_space::firm_home)
        .transpose()
        .map_err(|error| {
            encoded_error(
                "ACTION_UNAVAILABLE",
                error.to_string(),
                "execution_node",
                node_id,
                None,
            )
        })?;
    if let Some(firm_home) = firm_home.as_deref() {
        if let Some(replay) = replay_receipted_operator_action(firm_home, node_id, &auth)? {
            return Ok(replay);
        }
    }
    let node_revision = store
        .execution_nodes()?
        .into_iter()
        .filter(|node| node.id == node_id)
        .count() as u64;
    if auth.expected_version != node_revision {
        return Err(encoded_error(
            "VERSION_CONFLICT",
            "Operator action requires the exact current ExecutionNode revision",
            "execution_node",
            node_id,
            Some(node_revision),
        ));
    }
    let local_node_id = crate::read_local_node_id().map_err(|error| {
        encoded_error(
            "ACTION_UNAVAILABLE",
            error.to_string(),
            "execution_node",
            node_id,
            Some(node_revision),
        )
    })?;
    if local_node_id != node_id {
        return Err(encoded_error(
            "UNAUTHORIZED_ACTOR",
            "Operator action targets a Node other than this machine's immutable Node",
            "execution_node",
            node_id,
            Some(node_revision),
        ));
    }
    match (operation, intent) {
        ("diagnostics", OperatorActionIntent::Diagnose) => {
            let lease = store.latest_node_daemon_lease(node_id)?;
            Ok(RoleActionResult {
                ok: true,
                action_protocol_version: "agentfirm.role_actions.v1",
                projection: json!({"node_id":node_id,"daemon_lease":lease}),
                event_id: format!("diagnostic:{}", auth.idempotency_key),
                resulting_version: auth.expected_version,
                store_sequence: store
                    .canonical_operations_for_space(&auth.execution_space_id)?
                    .len() as u64,
                replayed: false,
            })
        }
        (
            "daemon-start",
            OperatorActionIntent::DaemonStart {
                max_concurrency,
                daemon_generation,
            },
        ) => {
            if max_concurrency == 0 {
                return Err(encoded_error(
                    "INVALID_STATE_TRANSITION",
                    "daemon concurrency must be non-zero",
                    "execution_node",
                    node_id,
                    None,
                ));
            }
            let firm_home = firm_home.expect("daemon action resolves firm home before dispatch");
            let current_generation = store
                .latest_node_daemon_lease(node_id)?
                .map(|lease| lease.generation)
                .unwrap_or(0);
            if daemon_generation != current_generation {
                return Err(encoded_error(
                    "SUPERVISOR_GENERATION_FENCED",
                    "daemon start intent does not match the current NodeDaemon generation",
                    "node_daemon_lease",
                    node_id,
                    Some(current_generation),
                ));
            }
            if crate::supervisor_daemon::daemon_status_via_socket(&firm_home, node_id).is_some() {
                return Err(encoded_error(
                    "ACTION_UNAVAILABLE",
                    "NodeDaemon is already live; refresh the Operator RoleView",
                    "execution_node",
                    node_id,
                    Some(node_revision),
                ));
            }
            execute_receipted_operator_action(&firm_home, node_id, &auth, || {
                let status = crate::supervisor_daemon::start_daemon_process_fenced(
                    &firm_home,
                    node_id,
                    max_concurrency,
                    &auth.execution_space_id,
                    daemon_generation,
                )
                .map_err(|error| {
                    encoded_error(
                        "DAEMON_START_FAILED",
                        error.to_string(),
                        "execution_node",
                        node_id,
                        Some(node_revision),
                    )
                })?;
                let lease = store.latest_node_daemon_lease(node_id)?;
                Ok(RoleActionResult {
                    ok: true,
                    action_protocol_version: "agentfirm.role_actions.v1",
                    projection: json!({"node_id":node_id,"status":status,"lease":lease}),
                    event_id: format!("daemon-start:{}", auth.idempotency_key),
                    resulting_version: node_revision,
                    store_sequence: store
                        .canonical_operations_for_space(&auth.execution_space_id)?
                        .len() as u64,
                    replayed: false,
                })
            })
        }
        ("daemon-stop", OperatorActionIntent::DaemonStop { daemon_generation }) => {
            let firm_home = firm_home.expect("daemon action resolves firm home before dispatch");
            let lease = store.latest_node_daemon_lease(node_id)?.ok_or_else(|| {
                encoded_error(
                    "SUPERVISOR_GENERATION_FENCED",
                    "daemon stop requires a current NodeDaemon lease",
                    "node_daemon_lease",
                    node_id,
                    None,
                )
            })?;
            if lease.generation != daemon_generation
                || lease.status != harness_core::NodeDaemonLeaseStatus::Active
                || lease.expires_unix_ms <= crate::current_unix_ms_u64()
            {
                return Err(encoded_error(
                    "SUPERVISOR_GENERATION_FENCED",
                    "daemon stop intent does not match the current live NodeDaemon generation",
                    "node_daemon_lease",
                    node_id,
                    Some(lease.generation),
                ));
            }
            execute_receipted_operator_action(&firm_home, node_id, &auth, || {
                let response = crate::supervisor_daemon::daemon_stop_via_socket(
                    &firm_home,
                    node_id,
                    &auth.execution_space_id,
                    daemon_generation,
                )
                .ok_or_else(|| {
                    encoded_error(
                        "ACTION_UNAVAILABLE",
                        "no live NodeDaemon is available to stop; refresh the Operator RoleView",
                        "execution_node",
                        node_id,
                        Some(node_revision),
                    )
                })?;
                let control =
                    serde_json::from_str::<serde_json::Value>(&response).map_err(|_| {
                        encoded_error(
                            "RECOVERY_REQUIRED",
                            "NodeDaemon returned an invalid stop receipt",
                            "node_daemon_lease",
                            node_id,
                            Some(daemon_generation),
                        )
                    })?;
                if control["ok"] != true {
                    // Two different failures arrive on this one path and must
                    // not share a code. A generation fence means the stop was
                    // refused with no effect; a drain that did not complete
                    // means the daemon accepted the stop, is still working,
                    // and deliberately retains machine authority (#584).
                    let message = control["error"]
                        .as_str()
                        .unwrap_or("NodeDaemon rejected the generation-fenced stop");
                    let code = if control["drained"] == false {
                        "NODE_DAEMON_DRAIN_INCOMPLETE"
                    } else {
                        "SUPERVISOR_GENERATION_FENCED"
                    };
                    // A partial release is a real outcome, so name the
                    // Execution Space leases on both sides rather than leaving
                    // the operator to guess (DEV-149-REVIEW-04).
                    let space_ids = |key: &str| {
                        control[key]
                            .as_array()
                            .map(|ids| {
                                ids.iter()
                                    .filter_map(|id| id.as_str())
                                    .collect::<Vec<_>>()
                                    .join(", ")
                            })
                            .filter(|joined| !joined.is_empty())
                            .unwrap_or_else(|| "none".to_string())
                    };
                    let detail = match control["failed_phase"].as_str() {
                        Some(phase) => format!(
                            "{message} (failed phase: {phase}; Execution Space leases already released: {}; release failed: {})",
                            space_ids("released_execution_space_ids"),
                            space_ids("release_failed_execution_space_ids"),
                        ),
                        None => message.to_string(),
                    };
                    return Err(encoded_error(
                        code,
                        detail,
                        "node_daemon_lease",
                        node_id,
                        Some(daemon_generation),
                    ));
                }
                Ok(RoleActionResult {
                    ok: true,
                    action_protocol_version: "agentfirm.role_actions.v1",
                    projection: json!({"node_id":node_id,"status":response}),
                    event_id: format!("daemon-stop:{}", auth.idempotency_key),
                    resulting_version: node_revision,
                    store_sequence: store
                        .canonical_operations_for_space(&auth.execution_space_id)?
                        .len() as u64,
                    replayed: false,
                })
            })
        }
        (
            "daemon-recover-predecessor",
            OperatorActionIntent::RecoverDaemonPredecessor {
                daemon_id,
                instance_id,
                daemon_generation,
                provider_process_groups_terminated_confirmed,
                evidence_ref,
            },
        ) => {
            let firm_home = firm_home.expect("daemon recovery resolves firm home before dispatch");
            let intent = crate::daemon_predecessor_recovery::validate_daemon_predecessor_recovery(
                &firm_home,
                node_id,
                Some((daemon_id.as_str(), instance_id.as_str(), daemon_generation)),
            )
            .map_err(|(code, detail)| {
                encoded_error(
                    &code,
                    detail,
                    "node_daemon_lease",
                    node_id,
                    Some(daemon_generation),
                )
            })?;
            execute_receipted_operator_action(&firm_home, node_id, &auth, || {
                let projection =
                    crate::daemon_predecessor_recovery::recover_daemon_predecessor_spaces(
                        &firm_home,
                        node_id,
                        &intent,
                        &auth.actor,
                        provider_process_groups_terminated_confirmed,
                        &evidence_ref,
                        &auth.idempotency_key,
                        auth.request_fingerprint.clone(),
                    )
                    .map_err(|(code, detail)| {
                        encoded_error(
                            &code,
                            detail,
                            "node_daemon_lease",
                            node_id,
                            Some(daemon_generation),
                        )
                    })?;
                Ok(RoleActionResult {
                    ok: true,
                    action_protocol_version: "agentfirm.role_actions.v1",
                    projection,
                    event_id: format!("daemon-recover-predecessor:{}", auth.idempotency_key),
                    resulting_version: node_revision,
                    store_sequence: store
                        .canonical_operations_for_space(&auth.execution_space_id)?
                        .len() as u64,
                    replayed: false,
                })
            })
        }
        (
            "provider-admission",
            OperatorActionIntent::AdmitProvider {
                provider,
                execution_mode,
                eligibility_fingerprint,
            },
        ) => {
            let binding = provider_admission_action_binding(
                store,
                &auth.execution_space_id,
                node_id,
                node_revision,
                &provider,
                &execution_mode,
            );
            if binding.eligibility_fingerprint != eligibility_fingerprint {
                return Err(encoded_error(
                    "ACTION_BINDING_MISMATCH",
                    "provider admission tuple or eligibility changed; refetch the Operator RoleView",
                    "execution_node",
                    node_id,
                    Some(node_revision),
                ));
            }
            if let Some(reason) = binding.disabled_reason {
                return Err(encoded_error(
                    "ACTION_UNAVAILABLE",
                    reason,
                    "execution_node",
                    node_id,
                    Some(node_revision),
                ));
            }
            let firm_home = firm_home.expect("receipted provider action resolves firm home");
            execute_receipted_operator_action(&firm_home, node_id, &auth, || {
                let (admission, replayed) = crate::admit_provider_from_operator_action(
                    store,
                    &auth.execution_space_id,
                    node_id,
                    &provider,
                    &execution_mode,
                    &auth.idempotency_key,
                )
                .map_err(|error| {
                    encoded_error(
                        "PROVIDER_ADMISSION_FAILED",
                        error,
                        "execution_node",
                        node_id,
                        Some(node_revision),
                    )
                })?;
                Ok(RoleActionResult {
                    ok: true,
                    action_protocol_version: "agentfirm.role_actions.v1",
                    projection: serde_json::to_value(&admission)?,
                    event_id: admission.id.clone(),
                    resulting_version: 1,
                    store_sequence: store.latest_provider_compatibility_admissions()?.len() as u64,
                    replayed,
                })
            })
        }
        _ => Err(encoded_error(
            "INVALID_STATE_TRANSITION",
            "semantic action does not match Operator route",
            "execution_node",
            node_id,
            None,
        )),
    }
}
