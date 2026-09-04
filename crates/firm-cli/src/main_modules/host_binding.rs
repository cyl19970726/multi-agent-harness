use super::*;

pub(super) const HOST_BINDING_LEASE_DEFAULT_TTL_MS: u64 = 30_000;
pub(super) const HOST_BINDING_LEASE_MIN_TTL_MS: u64 = 5_000;
pub(super) const HOST_BINDING_LEASE_MAX_TTL_MS: u64 = 300_000;

pub(super) fn require_external_interactive_host_binding(
    run: &AgentTeamRun,
    surface: &str,
    thread_id: &str,
) -> CliResult<()> {
    if run.host_control_mode != HostControlMode::ExternalInteractive
        || canonical_surface(&run.host_surface) != canonical_surface(surface)
        || run.host_thread_id.as_deref() != Some(thread_id)
    {
        return Err(CliError::Usage(format!(
            "UNAUTHORIZED_ACTOR: --surface and --thread-id must identify the exact external_interactive Host binding for TeamRun {}",
            run.id
        )));
    }
    Ok(())
}

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

#[allow(dead_code)]
pub(super) fn dispatch_headless_host_once(
    _store: &HarnessStore,
    _resolved: &ResolvedStore,
    _args: &[String],
) -> CliResult<serde_json::Value> {
    Err(CliError::Usage(
        "EXTERNAL_HOST_IS_PULL_ONLY: Harness cannot drive an external Host provider turn or prove provider receipt; read and acknowledge the Host inbox explicitly"
            .to_string(),
    ))
}
