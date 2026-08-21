use super::*;

pub(super) const HOST_BINDING_LEASE_DEFAULT_TTL_MS: u64 = 30_000;
pub(super) const HOST_BINDING_LEASE_MIN_TTL_MS: u64 = 5_000;
pub(super) const HOST_BINDING_LEASE_MAX_TTL_MS: u64 = 300_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct HostSessionValidationRequest<'a> {
    pub(super) host_surface: &'a str,
    pub(super) host_thread_id: &'a str,
}

/// Exact provider-native identity returned from canonical provider metadata.
/// There is intentionally no CLI boolean or free-form receipt parser. This is
/// same-user filesystem evidence, not live attachment or authentication.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct HostSessionValidationReceipt {
    pub(super) host_surface: String,
    pub(super) host_thread_id: String,
    pub(super) owner_id: String,
    pub(super) discovery_source: &'static str,
}

pub(super) trait HostSessionValidator {
    fn validate(
        &self,
        request: &HostSessionValidationRequest<'_>,
    ) -> Result<HostSessionValidationReceipt, String>;
}

#[derive(Default)]
pub(super) struct RuntimeHostSessionValidator {
    /// Tests may supply an isolated provider root. Production always resolves
    /// the canonical default `<HOME>/.codex` root and does not trust
    /// caller-controlled `CODEX_HOME` as validation evidence.
    pub(super) codex_home: Option<PathBuf>,
}

#[cfg(test)]
impl RuntimeHostSessionValidator {
    pub(super) fn for_codex_home(codex_home: PathBuf) -> Self {
        Self {
            codex_home: Some(codex_home),
        }
    }
}

impl HostSessionValidator for RuntimeHostSessionValidator {
    fn validate(
        &self,
        request: &HostSessionValidationRequest<'_>,
    ) -> Result<HostSessionValidationReceipt, String> {
        let surface = canonical_surface(request.host_surface);
        if surface != "codex" {
            return Err(format!(
                "surface `{}` exposes no trusted native Host-session discovery API",
                request.host_surface
            ));
        }
        let codex_home = match self.codex_home.as_ref() {
            Some(root) => root.clone(),
            None => project::home_dir()
                .map_err(|error| format!("canonical HOME resolution failed: {error}"))?
                .join(".codex"),
        };
        native_session::discover_codex_rollout(&codex_home, request.host_thread_id)
            .map_err(|error| format!("Codex rollout discovery failed: {error}"))?
            .ok_or_else(|| {
                format!(
                    "canonical Codex rollout metadata does not contain exact session `{}`",
                    request.host_thread_id
                )
            })?;
        Ok(HostSessionValidationReceipt {
            host_surface: surface.to_string(),
            host_thread_id: request.host_thread_id.to_string(),
            owner_id: format!("interactive:codex:{}", request.host_thread_id),
            discovery_source: "codex_rollout_session_meta",
        })
    }
}

#[derive(Debug)]
pub(super) struct HostBindLeaseResult {
    pub(super) run: AgentTeamRun,
    pub(super) lease: Option<HostBindingLease>,
    pub(super) validation_warning: Option<String>,
}

pub(super) fn checked_host_binding_lease_ttl_ms(args: &[String]) -> CliResult<u64> {
    let ttl = value(args, "--lease-ttl-ms")
        .map(|raw| {
            raw.parse::<u64>()
                .map_err(|_| CliError::Usage("--lease-ttl-ms must be an integer".to_string()))
        })
        .transpose()?
        .unwrap_or(HOST_BINDING_LEASE_DEFAULT_TTL_MS);
    if !(HOST_BINDING_LEASE_MIN_TTL_MS..=HOST_BINDING_LEASE_MAX_TTL_MS).contains(&ttl) {
        return Err(CliError::Usage(format!(
            "--lease-ttl-ms must be between {HOST_BINDING_LEASE_MIN_TTL_MS} and {HOST_BINDING_LEASE_MAX_TTL_MS}"
        )));
    }
    Ok(ttl)
}

pub(super) fn acquire_validated_interactive_host_lease<V: HostSessionValidator>(
    store: &HarnessStore,
    run: &AgentTeamRun,
    ttl_ms: u64,
    validator: &V,
    now_unix_ms: u64,
) -> CliResult<(Option<HostBindingLease>, Option<String>)> {
    let Some(thread_id) = run.host_thread_id.as_deref() else {
        return Ok((
            None,
            Some("TeamRun has no exact Host thread id".to_string()),
        ));
    };
    let request = HostSessionValidationRequest {
        host_surface: &run.host_surface,
        host_thread_id: thread_id,
    };
    let receipt = match validator.validate(&request) {
        Ok(receipt) => receipt,
        Err(reason) => {
            return Ok((
                None,
                Some(format!(
                    "Host binding remains unleased: {reason}. Codex requires exact session_meta evidence under canonical <HOME>/.codex/sessions; this proves rollout existence only, not live attachment or exclusive ownership"
                )),
            ));
        }
    };
    if canonical_surface(&receipt.host_surface) != canonical_surface(&run.host_surface)
        || receipt.host_thread_id != thread_id
    {
        return Err(CliError::Usage(
            "trusted Host-session validator returned a receipt for a different binding".to_string(),
        ));
    }
    if let Some(current) = store.effective_host_binding_lease_at(&run.id, now_unix_ms)? {
        if current.owner_kind == HostBindingLeaseOwnerKind::Interactive
            && current.owner_id == receipt.owner_id
        {
            return Ok((Some(current), None));
        }
    }
    let lease = store_conflict_as_usage(store.acquire_host_binding_lease(
        &run.id,
        &run.host_surface,
        thread_id,
        HostBindingLeaseOwnerKind::Interactive,
        &receipt.owner_id,
        &generated_id("host-binding-lease"),
        now_unix_ms,
        ttl_ms,
    ))?;
    Ok((Some(lease), None))
}

pub(super) fn bind_host_with_validator<V: HostSessionValidator>(
    store: &HarnessStore,
    team_run_id: &str,
    surface: &str,
    thread_id: &str,
    ttl_ms: u64,
    validator: &V,
    now_unix_ms: u64,
) -> CliResult<HostBindLeaseResult> {
    if surface.trim().is_empty() || thread_id.trim().is_empty() {
        return Err(CliError::Usage(
            "--surface and --thread-id must not be empty".to_string(),
        ));
    }
    let current = latest_team_run(store, team_run_id)?;
    let canonical = canonical_surface(surface).to_string();
    let run = if current.host_surface == canonical
        && current.host_thread_id.as_deref() == Some(thread_id)
    {
        current
    } else {
        let mut next = current.clone();
        next.host_surface = canonical;
        next.host_thread_id = Some(thread_id.to_string());
        next.updated_at = now_string();
        store_conflict_as_usage(store.compare_and_append_team_run(&current, &next))?;
        append_team_run_event(
            store,
            team_run_id,
            next_team_run_seq(store, team_run_id)?,
            TeamRunEventSourceKind::Host,
            None,
            "host_binding",
            team_run_id,
            "updated",
            &format!("Host binding set to {}:{thread_id}", next.host_surface),
        )?;
        next
    };
    let (lease, validation_warning) =
        acquire_validated_interactive_host_lease(store, &run, ttl_ms, validator, now_unix_ms)?;
    Ok(HostBindLeaseResult {
        run,
        lease,
        validation_warning,
    })
}

pub(super) fn parse_host_binding_lease_owner_kind(
    raw: &str,
) -> CliResult<HostBindingLeaseOwnerKind> {
    match raw {
        "interactive" => Ok(HostBindingLeaseOwnerKind::Interactive),
        "dispatcher" => Ok(HostBindingLeaseOwnerKind::Dispatcher),
        _ => Err(CliError::Usage(
            "--owner-kind must be interactive or dispatcher".to_string(),
        )),
    }
}

pub(super) fn exact_host_binding_lease_from_args(
    store: &HarnessStore,
    args: &[String],
) -> CliResult<HostBindingLease> {
    let team_run_id = required(args, "--id")?;
    let latest = store
        .latest_host_binding_lease(&team_run_id)?
        .ok_or_else(|| {
            CliError::Usage(format!("TeamRun {team_run_id} has no Host binding lease"))
        })?;
    let generation = required(args, "--generation")?
        .parse::<u64>()
        .map_err(|_| CliError::Usage("--generation must be an integer".to_string()))?;
    let supplied = (
        canonical_surface(&required(args, "--surface")?).to_string(),
        required(args, "--thread-id")?,
        parse_host_binding_lease_owner_kind(&required(args, "--owner-kind")?)?,
        required(args, "--owner-id")?,
        required(args, "--lease-id")?,
        generation,
    );
    if canonical_surface(&latest.host_surface) != supplied.0
        || latest.host_thread_id != supplied.1
        || latest.owner_kind != supplied.2
        || latest.owner_id != supplied.3
        || latest.lease_id != supplied.4
        || latest.generation != supplied.5
    {
        return Err(CliError::Usage(format!(
            "HOST_BINDING_LEASE_FENCED: supplied Host lease identity is not the latest exact lease for TeamRun {team_run_id}"
        )));
    }
    Ok(latest)
}

pub(super) fn headless_host_project_context(
    resolved: &ResolvedStore,
    run: &AgentTeamRun,
) -> CliResult<ProjectContext> {
    let binding_id = run.project_binding_id.as_str();
    if let Some(context) = resolved.context.as_ref() {
        if binding_id == context.id {
            return Ok(context.clone());
        }
    }
    let home = project::firm_home().map_err(project_err)?;
    project::context_for_id(&home, binding_id)
        .map_err(project_err)?
        .ok_or_else(|| {
            CliError::Usage(format!(
                "Project Binding {binding_id} for TeamRun {} is not registered",
                run.id
            ))
        })
}

pub(super) fn synthetic_headless_host_member(
    run: &AgentTeamRun,
    provider: &str,
    thread_id: &str,
) -> ProviderLaunchProfile {
    let provider_config = ProviderLaunchConfig {
        sandbox_policy: Some("read-only".to_string()),
        ..ProviderLaunchConfig::default()
    };
    ProviderLaunchProfile {
        id: format!("headless-host:{}", run.id),
        name: "headless-host-triage".to_string(),
        description: "Read-only dispatcher for the exact bound Host session".to_string(),
        role: "host-triage".to_string(),
        provider: provider.to_string(),
        model: None,
        profile: None,
        provider_config,
        capabilities: vec!["triage".to_string()],
        team_ids: Vec::new(),
        prompt_ref: None,
        skill_refs: Vec::new(),
        workspace_policy: Some("read_only".to_string()),
        provider_cwd_hint: None,
        permission_profile: Some("read_only".to_string()),
        runtime_workspace_roots: Vec::new(),
        status: ProviderLaunchStatus::Idle,
        current_task_id: None,
        current_proposal_id: None,
        provider_runtime_id: None,
        native_session: Some(provider_native_session_ref(provider, thread_id)),
        provider_thread_id: Some(thread_id.to_string()),
        provider_agent_path: None,
        provider_agent_nickname: None,
        provider_agent_role: None,
        control_endpoint: None,
        created_at: now_string(),
        last_seen_at: None,
    }
}

pub(super) fn dispatch_headless_host_once(
    store: &HarnessStore,
    resolved: &ResolvedStore,
    args: &[String],
) -> CliResult<serde_json::Value> {
    let team_run_id = required(args, "--id")?;
    let timeout_ms = value(args, "--timeout-ms")
        .map(|raw| {
            raw.parse::<u64>()
                .map_err(|_| CliError::Usage("--timeout-ms must be an integer".to_string()))
        })
        .transpose()?
        .unwrap_or(300_000);
    let min_age_secs = value(args, "--min-age-s")
        .map(|raw| {
            raw.parse::<u64>()
                .map_err(|_| CliError::Usage("--min-age-s must be an integer".to_string()))
        })
        .transpose()?
        .unwrap_or_else(HostDispatchConfig::default_age_threshold);
    let config = HostDispatchConfig {
        attention_age_threshold_secs: min_age_secs,
        ..HostDispatchConfig::default()
    };
    let now_ms = current_unix_ms_u64();
    let decision =
        host_dispatcher::schedule_team_run(store, &team_run_id, &config, now_ms, &now_string())?;
    let attention_ids = match decision {
        host_dispatcher::ScheduleDecision::DispatchReady { attention_ids, .. } => attention_ids,
        other => {
            return Ok(serde_json::json!({
                "team_run_id": team_run_id,
                "dispatched": false,
                "decision": format!("{other:?}"),
            }))
        }
    };
    let run = latest_team_run(store, &team_run_id)?;
    let thread_id = run.host_thread_id.as_deref().ok_or_else(|| {
        CliError::Usage(format!(
            "TeamRun {team_run_id} has no exact Host thread binding"
        ))
    })?;
    let provider = canonical_surface(&run.host_surface).to_string();
    let descriptor = harness_application::provider_descriptor(&provider).ok_or_else(|| {
        CliError::Usage(format!(
            "Host surface {:?} is not a production coding-agent provider",
            run.host_surface
        ))
    })?;
    let host_binding = descriptor.headless_host.ok_or_else(|| {
        if provider == "codex" {
            CliError::Usage(
                "HEADLESS_HOST_READ_ONLY_UNAVAILABLE: Codex exact-session resume inherits the existing session sandbox and cannot currently prove a read-only Host turn; use the interactive Host or a provider transport that enforces read-only resume"
                    .to_string(),
            )
        } else {
            CliError::Usage(format!(
                "HEADLESS_HOST_UNSUPPORTED: provider {provider} has no reviewed exact-session read-only Host binding"
            ))
        }
    })?;
    let project_context = headless_host_project_context(resolved, &run)?;
    let execution_mode = host_binding.execution_mode;
    let mut profile = team_member_provider_profile_for_mode(&provider, Some(execution_mode));
    let detected = team_member_provider_version_output(&provider);
    let probe_error = detected.as_ref().err().cloned();
    apply_provider_version(&mut profile, detected.ok());
    let compatibility = resolve_provider_compatibility(store, &profile, probe_error.as_deref())?;
    if !compatibility.allowed {
        return Err(CliError::Usage(format!(
            "PROVIDER_COMPATIBILITY_BLOCKED: headless Host provider {} mode {} status {:?}; complete source review or record an exact operational admission before dispatch",
            profile.provider, profile.execution_mode, compatibility.status
        )));
    }

    let owner_id = format!("dispatcher:cli:{}", std::process::id());
    let lease_id = generated_id("host-dispatch-lease");
    let lease = store_conflict_as_usage(store.acquire_host_binding_lease(
        &run.id,
        &run.host_surface,
        thread_id,
        HostBindingLeaseOwnerKind::Dispatcher,
        &owner_id,
        &lease_id,
        now_ms,
        timeout_ms.saturating_add(30_000),
    ))?;
    let claim_id = generated_id("host-dispatch-claim");
    let delivery_id = generated_id("host-dispatch-delivery");
    let member = synthetic_headless_host_member(&run, &provider, thread_id);
    let runtime = ProviderProcess {
        id: format!("runtime:{delivery_id}"),
        agent_member_id: member.id.clone(),
        provider: provider.clone(),
        status: ProviderProcessStatus::Running,
        pid: None,
        control_endpoint: Some(format!("headless-host://{provider}/{thread_id}")),
        command: provider.clone(),
        args: Vec::new(),
        started_at: now_string(),
        ended_at: None,
        last_event_at: Some(now_string()),
        health: ProviderProcessHealth {
            process_alive: true,
            socket_exists: false,
            protocol_probe: Some("exact-native-session-resume".to_string()),
            delivery_probe: None,
            checked_at: Some(now_string()),
        },
    };
    let cutoff = now_ms.saturating_sub(min_age_secs.saturating_mul(1_000));
    let dispatched = catch_unwind(AssertUnwindSafe(|| {
        host_dispatcher::claim_dispatcher_batch_with_consumer(
            store,
            host_dispatcher::DispatcherBatchRequest {
                lease: &lease,
                older_than_unix_ms: cutoff,
                limit: attention_ids.len(),
                claim_id: &claim_id,
                now_unix_ms: now_ms,
                updated_at: &format!("unix-ms:{now_ms}"),
            },
            |claimed| {
                let delivered_attention_ids = claimed
                    .iter()
                    .map(|attention| attention.id.clone())
                    .collect::<Vec<_>>();
                let message = RegistryMessage {
                    id: delivery_id.clone(),
                    task_id: None,
                    from_agent_id: "system:host-dispatcher".to_string(),
                    to_agent_id: Some(member.id.clone()),
                    channel: Some("host-triage".to_string()),
                    kind: RegistryMessageIntent::Message,
                    delivery_status: RegistryDeliveryStatus::Acknowledged,
                    content: host_dispatcher::build_headless_host_prompt(
                        &run.id,
                        &run.objective,
                        claimed,
                    ),
                    evidence_ids: Vec::new(),
                    created_at: now_string(),
                    delivery: None,
                    sender_kind: SenderKind::System,
                };
                match host_binding.binding {
                    harness_application::HostRuntimeKind::KimiAcp => {
                        let turn = harness_provider_kimi::run_kimi_host_turn(
                            &project_context.project_root,
                            thread_id,
                            &message.content,
                            Duration::from_millis(timeout_ms),
                        )
                        .map_err(|error| StoreError::Conflict(error.to_string()))?;
                        return host_dispatcher::DispatcherConsumerSuccess::new(
                            (turn.response_text, delivered_attention_ids),
                            turn.provider_receipt_id,
                        );
                    }
                    harness_application::HostRuntimeKind::ClaudeCli => {}
                }

                let outcome = run_claude_host_delivery(
                    store,
                    &member,
                    &runtime,
                    &message,
                    &delivery_id,
                    timeout_ms,
                    &project_context,
                )
                .map_err(|error| StoreError::Conflict(error.to_string()))?;
                if outcome.status != ProviderExecutionStatus::Succeeded {
                    return Err(StoreError::Conflict(format!(
                        "headless Host provider turn ended {:?}: {}",
                        outcome.status, outcome.summary
                    )));
                }
                let resumed_session = outcome
                    .native_session
                    .as_ref()
                    .map(|session| session.native_session_id.as_str())
                    .or(outcome.provider_thread_id.as_deref());
                if resumed_session != Some(thread_id) {
                    return Err(StoreError::Conflict(format!(
                        "headless Host resume identity drifted: expected {thread_id}, got {}",
                        resumed_session.unwrap_or("unavailable")
                    )));
                }
                let receipt = outcome
                    .provider_request_id
                    .or(outcome.provider_turn_id)
                    .map(|id| format!("{provider}:{thread_id}:{id}"))
                    .unwrap_or_else(|| format!("{provider}:{thread_id}:terminal"));
                host_dispatcher::DispatcherConsumerSuccess::new(
                    (
                        outcome.response_text.unwrap_or(outcome.summary),
                        delivered_attention_ids,
                    ),
                    receipt,
                )
            },
        )
    }));
    let released = store.release_host_binding_lease(&lease, current_unix_ms_u64());
    let dispatched = match dispatched {
        Ok(result) => result,
        Err(payload) => {
            if let Err(error) = released {
                eprintln!(
                    "headless Host consumer panicked and dispatcher lease release failed: {error}"
                );
            }
            resume_unwind(payload)
        }
    };
    let (summary, delivered_attention_ids) = match (dispatched, released) {
        (Ok(delivery), Ok(_)) => delivery,
        (Err(error), Ok(_)) => return Err(error.into()),
        (Ok(_), Err(error)) => return Err(error.into()),
        (Err(error), Err(release_error)) => {
            return Err(StoreError::Conflict(format!(
                "{error}; dispatcher lease release also failed: {release_error}"
            ))
            .into())
        }
    };
    append_team_run_event(
        store,
        &run.id,
        next_team_run_seq(store, &run.id)?,
        TeamRunEventSourceKind::Host,
        None,
        "host_dispatch",
        &run.id,
        "delivered",
        &format!(
            "headless Host delivered {} attention(s) to exact {}:{}",
            delivered_attention_ids.len(),
            provider,
            thread_id
        ),
    )?;
    Ok(serde_json::json!({
        "team_run_id": run.id,
        "dispatched": true,
        "host_surface": provider,
        "host_thread_id": thread_id,
        "attention_ids": delivered_attention_ids,
        "provider_summary": summary,
    }))
}
