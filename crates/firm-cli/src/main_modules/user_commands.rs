use super::*;

pub(super) fn member_trust_command(
    store: &HarnessStore,
    resolved: &ResolvedStore,
    args: &[String],
) -> CliResult<()> {
    require_subcommand(args, "member-trust mutate --actor-kind <kind> --actor-id <id> --idempotency-key <key> --expected-version <n> --json <TrustCommand JSON>")?;
    if args[0] != "mutate" {
        return Err(CliError::Usage(format!(
            "unknown member-trust command: {}",
            args[0]
        )));
    }
    let execution_space_id = resolved
        .execution_space_context
        .as_ref()
        .map(|space| space.id.clone())
        .ok_or_else(|| {
            CliError::Usage("member-trust mutations require an explicit Execution Space".into())
        })?;
    let actor_kind_raw = required(args, "--actor-kind")?;
    let actor_kind = agentfirm_api::parse_actor_kind(&actor_kind_raw).ok_or_else(|| {
        CliError::Usage("--actor-kind must be human|agent_member|external|service".into())
    })?;
    let actor_id = required(args, "--actor-id")?;
    let idempotency_key = required(args, "--idempotency-key")?;
    let expected_version = required(args, "--expected-version")?
        .parse::<u64>()
        .map_err(|_| CliError::Usage("--expected-version must be an unsigned integer".into()))?;
    let command = serde_json::from_str::<agentfirm_api::TrustCommand>(&required(args, "--json")?)?;
    let authority_actor = match (
        value(args, "--authority-kind"),
        value(args, "--authority-id"),
    ) {
        (None, None) => None,
        (Some(kind), Some(id)) => Some(harness_core::agentfirm_api::ActorRef {
            kind: agentfirm_api::parse_actor_kind(&kind)
                .ok_or_else(|| CliError::Usage("--authority-kind is invalid".into()))?,
            id,
        }),
        _ => {
            return Err(CliError::Usage(
                "--authority-kind and --authority-id must be provided together".into(),
            ))
        }
    };
    let auth = agentfirm_api::AuthenticatedMutation {
        execution_space_id,
        actor: harness_core::agentfirm_api::ActorRef {
            kind: actor_kind,
            id: actor_id,
        },
        authorized_authority_actors: authority_actor.into_iter().collect(),
        idempotency_key,
        expected_version,
        request_fingerprint: None,
    };
    match agentfirm_api::TrustApplication::new(store).execute(auth, command) {
        Ok(result) => print_json(&result),
        Err(StoreError::Conflict(encoded)) => Err(CliError::Usage(encoded)),
        Err(error) => Err(error.into()),
    }
}

pub(super) fn retired_command(command: &str) -> bool {
    matches!(
        command,
        "goal"
            | "phase"
            | "task"
            | "proposal"
            | "git"
            | "review"
            | "gap"
            | "goal-design"
            | "goal-evaluation"
            | "goal-case"
            | "vision"
            | "decision"
            | "autonomy"
            | "board"
            | "codex"
    )
}

pub(super) fn retired_surface_error(command: &str) -> CliError {
    CliError::Usage(format!(
        "`harness {command}` was retired with the Goal/GoalPhase/Task Graph coordination stack; use Agent Team or Host execution. Historical data remains available only through `harness legacy-goal-task export|verify`."
    ))
}

pub(super) fn company_command(_store: &HarnessStore, args: &[String]) -> CliResult<()> {
    // DOC-108 Stage B: the entire `harness company` tree is retired. The
    // Company Store registry and its Docs/Organization/Approval/Finance
    // surfaces are historical, export/verify-only through
    // `harness legacy-company-os export|verify`.
    let subcommand = args.first().map(String::as_str).unwrap_or("help");
    Err(retired_company_error(subcommand))
}
#[cfg(test)]
pub(super) fn append_jsonl_value(path: &Path, value: &serde_json::Value) -> CliResult<()> {
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    serde_json::to_writer(&mut file, value)?;
    file.write_all(b"\n")?;
    file.flush()?;
    Ok(())
}

pub(super) fn count_non_empty_lines(path: &Path) -> std::io::Result<u64> {
    let text = fs::read_to_string(path)?;
    Ok(text.lines().filter(|line| !line.trim().is_empty()).count() as u64)
}

/// Global Work CLI (DOC-106): the same one read projection served at
/// `/v1/views/global-work`, computed in process over every local Execution
/// Space store. Read-only; there is no aggregate writer and no Company ledger.
pub(super) fn global_work_command(args: &[String]) -> CliResult<()> {
    require_subcommand(args, "work list")?;
    match args[0].as_str() {
        "list" => {
            let execution_spaces = execution_space::firm_home()
                .ok()
                .and_then(|firm_home| execution_space::list_spaces(&firm_home).ok())
                .unwrap_or_default()
                .into_iter()
                .map(|space| (space.id, HarnessStore::new(space.store_root)))
                .collect::<Vec<_>>();
            let mut target = "/v1/views/global-work".to_string();
            let mut params = Vec::new();
            for (flag, key) in [
                ("--team-id", "team_id"),
                ("--mission-id", "mission_id"),
                ("--node-id", "node_id"),
                ("--host-id", "host_id"),
                ("--member-id", "member_id"),
                ("--assignee-membership-id", "assignee_membership_id"),
                ("--assignee-kind", "assignee_kind"),
                ("--phase", "phase"),
                ("--condition", "condition"),
                ("--resolution", "resolution"),
                ("--priority", "priority"),
                ("--module-id", "module_id"),
            ] {
                for selected in many(args, flag) {
                    params.push(format!("{key}={selected}"));
                }
            }
            if let Some(limit) = value(args, "--limit") {
                params.push(format!("limit={limit}"));
            }
            if let Some(cursor) = value(args, "--cursor") {
                params.push(format!("cursor={cursor}"));
            }
            if !params.is_empty() {
                target.push('?');
                target.push_str(&params.join("&"));
            }
            let view = role_views_api::global_work_view_json(&execution_spaces, &target)
                .map_err(CliError::Usage)?;
            print_json(&serde_json::json!({
                "ok": true,
                "result": view,
                "command": "harness work list",
                "boundaries": {
                    "authority": "Work/WorkOperation kernel",
                    "global_work_kind": "cross_execution_space_read_projection",
                    "global_work_creates_second_object": false,
                    "replaces": "company work list|query and /v1/views/company-work",
                    "mutation_route": "team-run work assign --membership-id (CAS) and team-run work commands",
                }
            }))
        }
        other => Err(CliError::Usage(format!(
            "unknown work command: {other}; usage: harness work list [--team-id|--assignee-membership-id|--assignee-kind|--member-id|--phase|--condition|--resolution|--priority|--module-id|--limit|--cursor]"
        ))),
    }
}
/// Read-only export/verification boundary for the retired Goal/Task ledgers.
pub(super) fn legacy_goal_task_command(args: &mut Vec<String>) -> CliResult<()> {
    if args.first().map(String::as_str) != Some("legacy-goal-task") {
        return Err(CliError::Usage(
            "usage: harness legacy-goal-task export|verify".into(),
        ));
    }
    args.remove(0);
    require_subcommand(args, "legacy-goal-task export|verify")?;
    match args[0].as_str() {
        "export" => {
            if args.iter().any(|arg| arg == "--store") {
                return Err(CliError::Usage(
                    "legacy-goal-task export requires --project; --store is not allowed".into(),
                ));
            }
            let project_flag_count = args.iter().filter(|arg| *arg == "--project").count();
            if project_flag_count != 1 {
                return Err(CliError::Usage(
                    "legacy-goal-task export requires exactly one --project <id|path>".into(),
                ));
            }
            let selector = take_flag_value(args, "--project").ok_or_else(|| {
                CliError::Usage("--project requires an id or existing project path".into())
            })?;
            let firm_home = project::firm_home().map_err(project_err)?;
            let context = resolve_project_selector(&firm_home, &selector).ok_or_else(|| {
                CliError::Usage(format!(
                    "project selector did not resolve; refusing fallback: {selector}"
                ))
            })?;
            let output = PathBuf::from(required(args, "--output")?);
            let summary = legacy_export::export_archive(
                &context.store_root,
                Some(context.id.as_str()),
                Some(&context.project_root),
                &output,
            )
            .map_err(CliError::Usage)?;
            print_json(&summary)?;
        }
        "verify" => {
            let archive = PathBuf::from(required(args, "--archive")?);
            let summary = legacy_export::verify_archive(&archive).map_err(CliError::Usage)?;
            print_json(&summary)?;
        }
        other => {
            return Err(CliError::Usage(format!(
                "unknown legacy-goal-task command: {other}"
            )))
        }
    }
    Ok(())
}

/// Read-only export/verification boundary for the retired Company OS record
/// surface (DOC-108 Stage A). Export enumerates every record store under the
/// resolved Firm home — Company Stores, Execution Space stores, project and
/// repo-local compatibility stores, machine node stores — so store selectors
/// are rejected rather than silently narrowing the enumeration. Verification
/// is fully offline and resolves no live store.
pub(super) fn legacy_company_os_command(args: &mut Vec<String>) -> CliResult<()> {
    if args.first().map(String::as_str) != Some("legacy-company-os") {
        return Err(CliError::Usage(
            "usage: harness legacy-company-os export|verify".into(),
        ));
    }
    args.remove(0);
    require_subcommand(args, "legacy-company-os export|verify")?;
    match args[0].as_str() {
        "export" => {
            for forbidden in ["--store", "--project", "--space", "--company"] {
                if args.iter().any(|arg| arg == forbidden) {
                    return Err(CliError::Usage(format!(
                        "legacy-company-os export enumerates the whole Firm home; {forbidden} is not allowed"
                    )));
                }
            }
            let firm_home = match take_flag_value(args, "--firm-home") {
                Some(raw) => PathBuf::from(raw),
                None => project::firm_home().map_err(project_err)?,
            };
            let output = PathBuf::from(required(args, "--output")?);
            let summary =
                legacy_company_os::export_archive(&firm_home, &output).map_err(CliError::Usage)?;
            print_json(&summary)?;
        }
        "verify" => {
            let archive = PathBuf::from(required(args, "--archive")?);
            let summary = legacy_company_os::verify_archive(&archive).map_err(CliError::Usage)?;
            print_json(&summary)?;
        }
        other => {
            return Err(CliError::Usage(format!(
                "unknown legacy-company-os command: {other}"
            )))
        }
    }
    Ok(())
}

/// `harness governance <check|init|describe>` — the project-portable doc/skill
/// governance gate, native in the binary (no node/pnpm). It runs over a project
/// root (cwd by default, `--root <path>` to override) using
/// `<root>/.governance.toml` (or a light default when absent), and
/// exits non-zero when a blocking gate fails — the same contract the legacy
/// `pnpm check:links/doc-size/skills/doc-governance` chain had.
pub(super) fn governance_command(args: &[String]) -> CliResult<()> {
    require_subcommand(args, "governance check|init|describe")?;
    let root = args
        .iter()
        .position(|a| a == "--root")
        .and_then(|i| args.get(i + 1))
        .map(PathBuf::from)
        .map(Ok)
        .unwrap_or_else(std::env::current_dir)?;
    let json = args.iter().any(|a| a == "--json");

    match args[0].as_str() {
        "check" => {
            let config =
                harness_governance::GovernanceConfig::load(&root).map_err(CliError::Usage)?;
            let report = harness_governance::run_check(&root, &config);
            print_governance_report(&report, json);
            if !report.passed() {
                std::process::exit(1);
            }
        }
        "init" => {
            let config = harness_governance::GovernanceConfig::default_firm();
            let path = root.join(".governance.toml");
            if path.exists() {
                return Err(CliError::Usage(format!(
                    "{} already exists",
                    path.display()
                )));
            }
            let toml = config.to_toml().map_err(CliError::Usage)?;
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(&path, toml)?;
            println!("wrote {}", path.display());
        }
        "describe" => {
            let config =
                harness_governance::GovernanceConfig::load(&root).map_err(CliError::Usage)?;
            print!("{}", config.to_toml().map_err(CliError::Usage)?);
        }
        other => {
            return Err(CliError::Usage(format!(
                "unknown governance subcommand: {other}"
            )))
        }
    }
    Ok(())
}

/// Print a governance report mirroring the legacy gates: per gate, warnings to
/// stderr (`console.warn`), then either the success summary (stdout) or the
/// failures (stderr). `--json` emits a machine-readable summary instead.
pub(super) fn print_governance_report(report: &harness_governance::GovernanceReport, json: bool) {
    if json {
        let gates: Vec<serde_json::Value> = report
            .gates
            .iter()
            .map(|g| {
                serde_json::json!({
                    "gate": g.kind,
                    "severity": g.severity,
                    "passed": g.failures.is_empty(),
                    "failures": g.failures,
                    "warnings": g.warnings,
                })
            })
            .collect();
        let out = serde_json::json!({ "passed": report.passed(), "gates": gates });
        println!("{}", serde_json::to_string_pretty(&out).unwrap_or_default());
        return;
    }
    for gate in &report.gates {
        for w in &gate.warnings {
            eprintln!("{w}");
        }
        if gate.failures.is_empty() {
            if !gate.summary.is_empty() {
                println!("{}", gate.summary);
            }
        } else {
            for f in &gate.failures {
                eprintln!("{f}");
            }
        }
    }
}

/// Record an operator-authorized compatibility decision for one exact adapter
/// tuple. This command probes and writes metadata only: it never installs,
/// builds, upgrades, or edits a provider or its adapter source.
pub(super) fn provider_command(
    store: &HarnessStore,
    resolved: &ResolvedStore,
    args: &[String],
) -> CliResult<()> {
    if matches!(args.first().map(String::as_str), Some("help" | "--help")) {
        print_provider_help();
        return Ok(());
    }
    require_subcommand(args, "provider admit")?;
    match args[0].as_str() {
        "admit" => provider_admit_command(store, resolved, &args[1..]),
        other => Err(CliError::Usage(format!(
            "unknown provider command: {other}; usage: harness [--project <id|path>] provider admit --provider <name> --execution-mode <mode> --provider-version <version> --adapter-contract-version <version> --evidence <ref> [--policy strict|advisory] [--actor <id>] [--json]"
        ))),
    }
}

pub(super) fn provider_admit_command(
    store: &HarnessStore,
    resolved: &ResolvedStore,
    args: &[String],
) -> CliResult<()> {
    provider_admit_command_with_probe(store, resolved, args, team_member_provider_version_output)
}

pub(super) fn provider_admit_command_with_probe<F>(
    store: &HarnessStore,
    resolved: &ResolvedStore,
    args: &[String],
    probe_provider_version: F,
) -> CliResult<()>
where
    F: Fn(&str) -> Result<String, String>,
{
    let provider = required(args, "--provider")?;
    let execution_mode = required(args, "--execution-mode")?;
    let provider_version = value(args, "--provider-version")
        .or_else(|| value(args, "--version"))
        .ok_or_else(|| CliError::Usage("--provider-version (or --version) is required".into()))?;
    let adapter_contract_version = required(args, "--adapter-contract-version")?;
    let evidence_refs = many(args, "--evidence");
    if evidence_refs.is_empty() || evidence_refs.iter().any(|value| value.trim().is_empty()) {
        return Err(CliError::Usage(
            "at least one non-empty --evidence reference is required".into(),
        ));
    }
    let policy = match value(args, "--policy").as_deref().unwrap_or("strict") {
        "strict" => ProviderCompatibilityAdmissionPolicy::Strict,
        "advisory" => ProviderCompatibilityAdmissionPolicy::Advisory,
        other => {
            return Err(CliError::Usage(format!(
                "unknown provider admission policy {other}; expected strict or advisory"
            )))
        }
    };

    if resolved.execution_space_context.is_some() && !resolved.project_selection_explicit {
        return Err(CliError::Usage(
            "provider admission into an Execution Space requires an explicit global `--project <id|path>` flag; FIRM_PROJECT, ACTIVE_PROJECT, and the space default binding are ambient selectors and cannot authorize this scoped write; no admission was written"
                .to_string(),
        ));
    }

    let mut profile = team_member_provider_profile_for_mode(&provider, Some(&execution_mode));
    if profile.execution_mode != execution_mode {
        return Err(CliError::Usage(format!(
            "execution mode {execution_mode} is not registered for provider {provider}"
        )));
    }
    let registered_contract = profile.adapter_contract_version.as_deref().ok_or_else(|| {
        CliError::Usage(format!(
            "provider {provider} mode {execution_mode} has no adapter contract to admit"
        ))
    })?;
    if registered_contract != adapter_contract_version {
        return Err(CliError::Usage(format!(
            "adapter contract mismatch: requested {adapter_contract_version}, registered {registered_contract}"
        )));
    }
    let detected = probe_provider_version(&provider).map_err(|error| {
        CliError::Usage(format!(
            "provider version probe failed; no admission was written: {error}"
        ))
    })?;
    if detected != provider_version {
        return Err(CliError::Usage(format!(
            "provider version mismatch: requested {provider_version}, installed {detected}; no admission was written"
        )));
    }
    apply_provider_version(&mut profile, Some(detected));
    if profile.compatibility_status != ProviderCompatibilityStatus::ReviewRequired {
        return Err(CliError::Usage(format!(
            "provider tuple is {}; only an actually observed review-required tuple may be admitted; no admission was written",
            serde_snake_label(&profile.compatibility_status)
        )));
    }

    let (project_id, store_id) = resolved.provider_compatibility_scope().ok_or_else(|| {
        CliError::Usage(
            "provider admission requires an explicitly resolved Project Binding and canonical store identity"
                .to_string(),
        )
    })?;
    let admission = ProviderCompatibilityAdmission {
        id: generated_id("provider-admission"),
        project_id,
        store_id,
        provider,
        execution_mode,
        provider_version,
        adapter_contract_version,
        policy,
        actor: value(args, "--actor").unwrap_or_else(|| "operator".to_string()),
        evidence_refs,
        admitted_at: now_string(),
        lifecycle: ProviderCompatibilityAdmissionLifecycle::Active,
        predecessor_admission_id: None,
        reason: None,
    };
    let ensured = store.ensure_provider_compatibility_admission(&admission)?;
    let admission = ensured.admission;
    if has_flag(args, "--json") {
        print_json(&serde_json::json!({
            "command": "harness provider admit",
            "ok": true,
            "created": ensured.created,
            "reused": !ensured.created,
            "source": "operational_admission",
            "source_review_modified": false,
            "provider_source_modified": false,
            "admission": admission,
        }))
    } else {
        println!(
            "{} {} {} {} {} (policy={}, record={}); adapter source review remains unchanged",
            if ensured.created {
                "admitted"
            } else {
                "reused"
            },
            admission.provider,
            admission.execution_mode,
            admission.provider_version,
            admission.adapter_contract_version,
            serde_snake_label(&admission.policy),
            admission.id,
        );
        Ok(())
    }
}

/// Server-owned provider admission seam for the closed Operator action.
/// The browser selects only a registered provider/mode; installed version,
/// adapter contract, scope and evidence are all observed and bound here.
pub(super) fn admit_provider_from_operator_action(
    store: &HarnessStore,
    execution_space_id: &str,
    node_id: &str,
    provider: &str,
    execution_mode: &str,
    idempotency_key: &str,
) -> Result<(ProviderCompatibilityAdmission, bool), String> {
    let admission_id = format!("provider-admission:{idempotency_key}");
    if let Some(existing) = store
        .latest_provider_compatibility_admissions()
        .map_err(|error| error.to_string())?
        .into_iter()
        .find(|existing| existing.id == admission_id)
    {
        let (project_id, store_id) = store
            .provider_compatibility_scope()
            .ok_or_else(|| "canonical provider compatibility scope is unavailable".to_string())?;
        if existing.project_id == project_id
            && existing.store_id == store_id
            && existing.provider == provider
            && existing.execution_mode == execution_mode
            && existing.actor == node_id
            && existing.evidence_refs.iter().any(|evidence| {
                evidence == &format!("server-scope:{execution_space_id}:{node_id}:{project_id}")
            })
        {
            return Ok((existing, true));
        }
        return Err("idempotency key is already bound to a different provider admission".into());
    }
    let registration = store
        .latest_node_project_registrations()
        .map_err(|error| error.to_string())?
        .into_iter()
        .find(|registration| {
            registration.node_id == node_id
                && registration.execution_space_id == execution_space_id
                && registration.status == harness_core::NodeProjectRegistrationStatus::Active
        })
        .ok_or_else(|| {
            "exact Node/project/Execution Space registration is not active".to_string()
        })?;
    let (project_id, store_id) = store
        .provider_compatibility_scope()
        .ok_or_else(|| "canonical provider compatibility scope is unavailable".to_string())?;
    if registration.project_binding_id != project_id {
        return Err("Node registration does not match the canonical project scope".into());
    }
    let (detected, adapter_contract_version) =
        operator_provider_admission_probe(provider, execution_mode)?;
    let admission = ProviderCompatibilityAdmission {
        id: admission_id,
        project_id: project_id.to_string(),
        store_id: store_id.to_string(),
        provider: provider.to_string(),
        execution_mode: execution_mode.to_string(),
        provider_version: detected.clone(),
        adapter_contract_version: adapter_contract_version.clone(),
        policy: ProviderCompatibilityAdmissionPolicy::Strict,
        actor: node_id.to_string(),
        evidence_refs: vec![
            format!("server-probe:provider-version:{provider}:{detected}"),
            format!("server-registry:adapter-contract:{adapter_contract_version}"),
            format!("server-scope:{execution_space_id}:{node_id}:{project_id}"),
        ],
        admitted_at: now_string(),
        lifecycle: ProviderCompatibilityAdmissionLifecycle::Active,
        predecessor_admission_id: None,
        reason: None,
    };
    let ensured = store
        .ensure_provider_compatibility_admission(&admission)
        .map_err(|error| error.to_string())?;
    Ok((ensured.admission, !ensured.created))
}

/// Observe whether an installed provider tuple is presently eligible for an
/// operational admission. RoleView projection and action execution share this
/// exact probe so the UI cannot advertise an action that the server already
/// knows it must reject.
pub(crate) fn operator_provider_admission_probe(
    provider: &str,
    execution_mode: &str,
) -> Result<(String, String), String> {
    let mut profile = team_member_provider_profile_for_mode(provider, Some(execution_mode));
    if profile.execution_mode != execution_mode {
        return Err(format!(
            "execution mode {execution_mode} is not registered for {provider}"
        ));
    }
    let adapter_contract_version = profile.adapter_contract_version.clone().ok_or_else(|| {
        format!("provider {provider}/{execution_mode} has no registered adapter contract")
    })?;
    let detected = team_member_provider_version_output(provider)?;
    apply_provider_version(&mut profile, Some(detected.clone()));
    if profile.compatibility_status != ProviderCompatibilityStatus::ReviewRequired {
        return Err(format!(
            "observed provider tuple is {}; admission is available only for review_required tuples",
            serde_snake_label(&profile.compatibility_status)
        ));
    }
    Ok((detected, adapter_contract_version))
}

pub(super) fn member_command(store: &HarnessStore, args: &[String]) -> CliResult<()> {
    require_subcommand(
        args,
        "member providers|preflight|inbox|message|work|runtime",
    )?;
    match args[0].as_str() {
        // Runtime availability of each provider ACCOUNT, reported separately
        // from adapter compatibility. See `member providers` for the latter.
        "preflight" => member_preflight_command(store, &args[1..])?,
        "register" | "list" => {
            return Err(CliError::Usage(
                "member identity registration moved to `member-trust mutate`; legacy registry routes are retired"
                    .into(),
            ));
        }
        // The provider-neutral capability matrix (goal-provider-neutral acceptance
        // #4): every REGISTERED provider with the capabilities its adapter
        // declares (streaming / resume / schema / cost / …). Derived from the
        // canonical application catalog — adding a provider cannot silently
        // omit its Team/Host/compatibility/historical classification.
        "providers" => {
            let fail_on_review = args.iter().any(|arg| arg == "--fail-on-review");
            let mut needs_review = false;
            let providers = harness_application::PROVIDERS
                .iter()
                .map(|descriptor| {
                    let compatibility = compatibility_delivery_binding(descriptor.provider);
                    let detected = team_member_provider_version_output(descriptor.provider);
                    let mut profile = team_member_provider_profile(descriptor.provider);
                    apply_provider_version(&mut profile, detected.as_ref().ok().cloned());
                    let resolution = resolve_provider_compatibility(
                        store,
                        &profile,
                        detected.as_ref().err().map(String::as_str),
                    );
                    let core_runtime_capabilities_active =
                        ["open_or_resume", "start_cycle", "observe"]
                            .into_iter()
                            .all(|capability| {
                                has_active_verified_provider_capability(&profile, capability)
                            });
                    needs_review |= resolution
                        .as_ref()
                        .map_or(true, |value| !value.allowed || value.needs_review);
                    needs_review |= !core_runtime_capabilities_active;
                    Ok(serde_json::json!({
                        "provider": descriptor.provider,
                        "catalog": descriptor,
                        "direct_delivery_compatibility_capabilities": compatibility.map(|binding| binding.capabilities()),
                        "team_member_profile": profile,
                        "operational_compatibility": resolution.ok(),
                        "version_probe_error": detected.err(),
                        // Executable per-intent capability report (DOC-89):
                        // null until the provider's binding migrates to the
                        // provider-neutral adapter.
                        "runtime_capability_bindings": crate::runtime_adapter::capability_bindings_for(descriptor.provider),
                        "core_runtime_capability_admission": if core_runtime_capabilities_active {"active"} else {"review_required"},
                    }))
                })
                .collect::<CliResult<Vec<_>>>()?;
            print_json(&providers)?;
            if fail_on_review && needs_review {
                return Err(CliError::Usage(
                    "one or more provider adapters require review; inspect the JSON report"
                        .to_string(),
                ));
            }
        }
        "inbox" => bound_member_inbox_command(store, &args[1..])?,
        "message" => bound_member_message_command(store, &args[1..])?,
        "work" => bound_member_work_command(store, &args[1..])?,
        "runtime" => bound_member_runtime_command(store, &args[1..])?,
        other => return Err(CliError::Usage(format!("unknown member command: {other}"))),
    }
    Ok(())
}

/// Provider lifecycle control from the exact Supervisor-bound AgentMember.
/// The caller token authenticates the source identity; the canonical Role
/// Action policy then permits only exact-self control or the Team Host acting
/// on another active MemberRun. The CLI never reaches the Store directly.
pub(super) fn bound_member_runtime_command(store: &HarnessStore, args: &[String]) -> CliResult<()> {
    require_subcommand(
        args,
        "member runtime interrupt [--member-run-id <target>] --expected-version <n> --reason <text>",
    )?;
    let context = bound_member_role_context()?;
    match args[0].as_str() {
        "interrupt" => {
            let target_member_run_id =
                value(args, "--member-run-id").unwrap_or_else(|| context.member_run_id.clone());
            let expected_version = required(args, "--expected-version")?
                .parse::<u64>()
                .map_err(|_| {
                    CliError::Usage("--expected-version must be an unsigned integer".into())
                })?;
            let reason = required(args, "--reason")?;
            execute_bound_member_role_action(
                store,
                &context,
                &format!("/v1/agentfirm/member-runs/{target_member_run_id}/interrupt"),
                expected_version,
                value(args, "--idempotency-key")
                    .unwrap_or_else(|| generated_id("member-runtime-interrupt")),
                serde_json::json!({
                    "action": "interrupt_member_run",
                    "reason": reason,
                }),
                None,
            )
        }
        other => Err(CliError::Usage(format!(
            "unknown member runtime command: {other}"
        ))),
    }
}

#[derive(Debug)]
pub(super) struct BoundMemberRoleContext {
    pub(super) team_run_id: String,
    pub(super) member_run_id: String,
    pub(super) capability_token: String,
}

/// Resolve a member-originated Role Action from the runtime envelope injected
/// by the owning Supervisor. These values only route the request; none is
/// trusted as actor identity. The live Supervisor validates the unpersisted
/// capability token and rebuilds the actor, Session, Team and lease scope from
/// its process-local registration immediately before the canonical action.
pub(super) fn bound_member_role_context() -> CliResult<BoundMemberRoleContext> {
    let member_run_id = env::var("FIRM_MEMBER_RUN_ID")
        .or_else(|_| env::var("HARNESS_MEMBER_RUN_ID"))
        .map_err(|_| {
            CliError::Usage(
                "member Role Actions require the Supervisor-bound FIRM_MEMBER_RUN_ID runtime environment"
                    .into(),
            )
        })?;
    let team_run_id = env::var("FIRM_TEAM_RUN_ID")
        .or_else(|_| env::var("HARNESS_TEAM_RUN_ID"))
        .map_err(|_| {
            CliError::Usage(
                "member Role Actions require the Supervisor-bound FIRM_TEAM_RUN_ID runtime environment"
                    .into(),
            )
        })?;
    let capability_token = env::var("FIRM_MEMBER_ROLE_ACTION_TOKEN")
        .or_else(|_| env::var("DSH_FIRM_MEMBER_ROLE_ACTION_TOKEN"))
        .map_err(|_| {
            CliError::Usage(
                "member Role Actions require the live Supervisor capability token".into(),
            )
        })?;
    Ok(BoundMemberRoleContext {
        team_run_id,
        member_run_id,
        capability_token,
    })
}

pub(super) fn execute_bound_member_role_action(
    store: &HarnessStore,
    context: &BoundMemberRoleContext,
    path: &str,
    expected_version: u64,
    idempotency_key: String,
    body: serde_json::Value,
    confirmed_action: Option<String>,
) -> CliResult<()> {
    let result = dispatch_live_member_control(
        store,
        LiveMemberControlRequest::RoleAction {
            team_run_id: context.team_run_id.clone(),
            member_run_id: context.member_run_id.clone(),
            capability_token: context.capability_token.clone(),
            path: path.to_string(),
            expected_version,
            idempotency_key,
            body,
            confirmed_action,
        },
    )?;
    print_json(&result)
}

pub(super) fn bound_member_inbox_command(store: &HarnessStore, args: &[String]) -> CliResult<()> {
    let context = bound_member_role_context()?;
    let member_run_id = context.member_run_id.clone();
    let messages = dispatch_live_member_control(
        store,
        LiveMemberControlRequest::ReadInbox {
            team_run_id: context.team_run_id,
            member_run_id: context.member_run_id,
            capability_token: context.capability_token,
            include_all: has_flag(args, "--all"),
        },
    )?;
    let messages = serde_json::from_value::<Vec<TeamMessageProjection>>(messages)?;
    if has_flag(args, "--json") {
        print_json(&messages)?;
    } else {
        for message in &messages {
            let delivery = message
                .deliveries
                .iter()
                .find(|delivery| delivery.member_id == member_run_id)
                .map(|delivery| serde_snake_label(&delivery.status))
                .unwrap_or_else(|| "unknown".to_string());
            println!(
                "{}\t{}\tfrom={}\t{}\t{}",
                message.id,
                team_message_kind_label(&message.kind),
                message.sender_runtime_id,
                delivery,
                message.body.lines().next().unwrap_or_default()
            );
        }
    }
    Ok(())
}

pub(super) fn bound_member_message_command(store: &HarnessStore, args: &[String]) -> CliResult<()> {
    require_subcommand(
        args,
        &crate::collaboration::member_operating_contract::member_message_subcommand_usage(),
    )?;
    let context = bound_member_role_context()?;
    let run = latest_team_run(store, &context.team_run_id)?;
    let team = store
        .latest_teams()?
        .remove(&run.agent_team_id)
        .ok_or_else(|| CliError::Usage("TeamRun references a missing AgentTeam".into()))?;
    let team_revision = derive_team_revisions(&store.teams()?)
        .get(&team.id)
        .copied()
        .unwrap_or_default();
    let body = required(args, "--body")?;
    if body.trim().is_empty() {
        return Err(CliError::Usage("--body must be non-empty Markdown".into()));
    }
    let work_id = value(args, "--work-id");
    let evidence_refs = many(args, "--evidence-ref");
    let response_required = has_flag(args, "--response-required");
    let (operation, intent) = match args[0].as_str() {
        "send" => {
            let recipient_ids = many(args, "--recipient-agent-id");
            if recipient_ids.is_empty() {
                return Err(CliError::Usage(
                    "member message send requires at least one --recipient-agent-id".into(),
                ));
            }
            (
                "send",
                serde_json::json!({
                    "action": "send_message",
                    "recipient_ids": recipient_ids,
                    "body": body,
                    "work_id": work_id,
                    "evidence_refs": evidence_refs,
                    "response_required": response_required,
                }),
            )
        }
        "reply" => {
            let recipient_ids = many(args, "--recipient-agent-id");
            if recipient_ids.is_empty() {
                return Err(CliError::Usage(
                    "member message reply requires at least one --recipient-agent-id".into(),
                ));
            }
            (
                "reply",
                serde_json::json!({
                    "action": "reply_message",
                    "recipient_ids": recipient_ids,
                    "body": body,
                    "correlation_id": required(args, "--correlation-id")?,
                    "causation_id": required(args, "--causation-id")?,
                    "work_id": work_id,
                    "evidence_refs": evidence_refs,
                    "response_required": response_required,
                }),
            )
        }
        "request-decision" => (
            "request-decision",
            serde_json::json!({
                "action": "request_decision",
                "body": body,
                "work_id": work_id,
                "evidence_refs": evidence_refs,
            }),
        ),
        other => {
            return Err(CliError::Usage(format!(
                "unknown member message command: {other}; expected send|reply|request-decision"
            )))
        }
    };
    let path = format!(
        "/v1/agentfirm/team-runs/{}/messages/{operation}",
        context.team_run_id
    );
    execute_bound_member_role_action(
        store,
        &context,
        &path,
        team_revision,
        value(args, "--idempotency-key").unwrap_or_else(|| generated_id("member-message")),
        intent,
        None,
    )
}

pub(super) fn bound_member_work_command(store: &HarnessStore, args: &[String]) -> CliResult<()> {
    require_subcommand(
        args,
        "member work create|assign|claim|start|block|resume|release|submit|accept --expected-version <n> ...",
    )?;
    let context = bound_member_role_context()?;
    let expected_version = required(args, "--expected-version")?
        .parse::<u64>()
        .map_err(|_| CliError::Usage("--expected-version must be an unsigned integer".into()))?;
    if args[0] == "create" {
        return execute_bound_member_role_action(
            store,
            &context,
            &format!("/v1/agentfirm/team-runs/{}/works", context.team_run_id),
            expected_version,
            value(args, "--idempotency-key").unwrap_or_else(|| generated_id("member-work")),
            serde_json::json!({
                "action": "create_work",
                "work_id": required(args, "--work-id")?,
                "title": required(args, "--title")?,
                "context_markdown": value(args, "--context").unwrap_or_default(),
                "completion_criteria_markdown": required(args, "--completion-criteria")?,
                "claim_mode": value(args, "--claim-mode").unwrap_or_else(|| "host_assign".into()),
                "priority": value(args, "--priority").unwrap_or_else(|| "normal".into()),
            }),
            None,
        );
    }
    let work_id = required(args, "--work-id")?;
    let (operation, intent) = match args[0].as_str() {
        "assign" => (
            "assign",
            serde_json::json!({
                "action": "assign_work",
                "member_run_id": required(args, "--member-run-id")?,
            }),
        ),
        "claim" => ("claim", serde_json::json!({"action": "claim_work"})),
        "start" => ("start", serde_json::json!({"action": "start_work"})),
        "block" => (
            "block",
            serde_json::json!({
                "action": "block_work",
                "reason": required(args, "--reason")?,
            }),
        ),
        "resume" => (
            "resume",
            serde_json::json!({
                "action": "unblock_work",
                "resolution": required(args, "--resolution")?,
            }),
        ),
        "release" => ("release", serde_json::json!({"action": "release_work"})),
        "submit" => (
            "submit",
            serde_json::json!({
                "action": "submit_work",
                "result_summary": required(args, "--result-summary")?,
                "artifact_refs": many(args, "--artifact-ref"),
                "check_refs": many(args, "--check-ref"),
                "base_revision": value(args, "--base-revision"),
                "candidate_revision": required(args, "--candidate-revision")?,
            }),
        ),
        "accept" => ("accept", serde_json::json!({"action": "accept_work"})),
        other => {
            return Err(CliError::Usage(format!(
                "unknown member work command: {other}; expected create|assign|claim|start|block|resume|release|submit|accept"
            )))
        }
    };
    let path = if operation == "accept" {
        let run = latest_team_run(store, &context.team_run_id)?;
        format!(
            "/v1/agentfirm/teams/{}/works/{work_id}/accept",
            run.agent_team_id
        )
    } else {
        format!(
            "/v1/agentfirm/team-runs/{}/works/{work_id}/{operation}",
            context.team_run_id
        )
    };
    execute_bound_member_role_action(
        store,
        &context,
        &path,
        expected_version,
        value(args, "--idempotency-key").unwrap_or_else(|| generated_id("member-work")),
        intent,
        (operation == "accept").then_some("accept".to_string()),
    )
}

pub(super) fn org_command(store: &HarnessStore, args: &[String]) -> CliResult<()> {
    require_subcommand(args, "org member|bootstrap-lead|host|cutover-audit")?;
    match args[0].as_str() {
        "member" | "bootstrap-lead" => return Err(CliError::Usage(
            "organization member identity uses canonical member-trust mutate; legacy convergence/bootstrap ledgers were removed".into(),
        )),
        "host" => {
            let rest = &args[1..];
            let team_id = value(rest, "--team")
                .or_else(|| value(rest, "--team-id"))
                .ok_or_else(|| CliError::Usage("missing required option --team".to_string()))?;
            let team = store
                .latest_teams()?
                .remove(&team_id)
                .ok_or_else(|| CliError::Usage(format!("AgentTeam not found: {team_id}")))?;
            print_json(&serde_json::json!({
                "team_id": team.id,
                "host_agent_id": team.host_agent_id,
                "source": "agent_team"
            }))?;
        }
        "cutover-audit" => {
            let teams = store.latest_teams()?;
            let members = store
                .all_trust_agent_members()?
                .into_iter()
                .map(|member| (member.id.clone(), member))
                .collect::<BTreeMap<_, _>>();
            let missions = store.latest_missions()?;
            let nodes = store.latest_execution_nodes()?;
            let mut mission_ids = BTreeSet::new();
            for team in teams.values() {
                team.validate()
                    .map_err(|error| CliError::Usage(error.to_string()))?;
                // Mission linkage is optional legacy provenance (DEV-35):
                // mission-less Teams are the current default and skip the
                // historical Mission-reference audit entirely.
                if let Some(mission_id) = team
                    .legacy_mission_id
                    .as_deref()
                    .filter(|mission_id| !mission_id.trim().is_empty())
                {
                    if !mission_ids.insert(mission_id) {
                        return Err(CliError::Usage(format!(
                            "multiple AgentTeams reference Mission {mission_id}"
                        )));
                    }
                    if !missions.iter().any(|mission| mission.id == mission_id) {
                        return Err(CliError::Usage(format!(
                            "AgentTeam {} references missing Mission {}",
                            team.id, mission_id
                        )));
                    }
                }
                if !nodes.iter().any(|node| node.id == team.node_id) {
                    return Err(CliError::Usage(format!(
                        "AgentTeam {} references missing ExecutionNode {}",
                        team.id, team.node_id
                    )));
                }
                if !members.contains_key(&team.host_agent_id) {
                    return Err(CliError::Usage(format!(
                        "AgentTeam {} references missing Host Agent {}",
                        team.id, team.host_agent_id
                    )));
                }
            }
            print_json(&serde_json::json!({
                "ready": true,
                "team_count": teams.len(),
                "agent_member_count": members.len(),
                "authority": "host_agent_id",
                "flat_team_model": true
            }))?;
        }
        other => return Err(CliError::Usage(format!("unknown org command: {other}"))),
    }
    Ok(())
}
