use super::*;

/// Every adapter's locator kind IS a table entry, enforced by the type system:
/// `TeamRuntimeAdapter::NATIVE_LOCATOR` is a `NativeLocatorKindEntry`, and the
/// trait's provided `native_locator_kind()` returns that entry's kind. An
/// adapter cannot answer with a literal.
///
/// This test covers what the type system cannot: that every adapter's declared
/// entry is the one the table registers for its own (provider, execution_mode),
/// and that all six entries are accounted for. Two `debug_assert_eq!`s used to
/// stand in for this, covering 2 of 6 and asserting against the very literals
/// the table exists to eliminate; they are deleted.
#[test]
fn every_adapter_declares_the_table_entry_for_its_own_mode() {
    use harness_core::native_locator::*;
    use harness_runtime_contract::TeamRuntimeAdapter;

    let declared =
        [
            <harness_provider_codex::CodexTeamRuntime<
                '_,
                crate::codex_app_server::CodexAppServerClient,
            > as TeamRuntimeAdapter>::NATIVE_LOCATOR,
            <harness_provider_kimi::KimiTeamRuntime<'_> as TeamRuntimeAdapter>::NATIVE_LOCATOR,
            <harness_provider_claude::ClaudeTeamRuntime as TeamRuntimeAdapter>::NATIVE_LOCATOR,
            <harness_provider_deepseek::DeepSeekTeamRuntime as TeamRuntimeAdapter>::NATIVE_LOCATOR,
            <harness_provider_pi::PiTeamRuntime as TeamRuntimeAdapter>::NATIVE_LOCATOR,
            // The NodeDaemon-owned Codex session adapter is not a TeamRuntimeAdapter;
            // it reads its entry directly at provider_adapter.rs.
            CODEX_NODE_DAEMON_APP_SERVER,
        ];
    assert_eq!(
        declared.len(),
        NATIVE_LOCATOR_KINDS.len(),
        "every table entry must be accounted for by this test"
    );
    for entry in declared {
        assert_eq!(
            native_locator_kind_for_mode(entry.provider, entry.execution_mode),
            Some(entry.native_locator_kind),
            "{}/{} declares an entry the table does not register",
            entry.provider,
            entry.execution_mode
        );
    }
}

/// The table and the closed runtime registry are two independent
/// `(provider, execution_mode)` lists. Nothing cross-checked them, so a sixth
/// provider could be registered as a runtime and silently fall into the new
/// fail-closed seeding path.
#[test]
fn every_registered_team_runtime_mode_resolves_in_the_locator_table() {
    for descriptor in harness_application::PROVIDERS.iter() {
        assert!(
            harness_core::native_locator_kind_for_mode(
                descriptor.provider,
                descriptor.team.execution_mode
            )
            .is_some(),
            "{}/{} is a registered Team runtime with no locator-kind entry; seeding a resume for it would now fail closed",
            descriptor.provider,
            descriptor.team.execution_mode
        );
    }
}

/// Seeding a resume pointer for a provider/mode with no reviewed adapter fails
/// closed with an honest refusal instead of inventing a placeholder kind.
///
/// This path is production-reachable: `validate_team_member_execution_mode`
/// consults the runtime registry only when `execution_mode` is `Some`, so an
/// unregistered provider with `execution_mode: None` reaches
/// `build_member_run_for_team` through the `unsupported_team_member` profile.
#[test]
fn seeding_a_resume_for_an_unreviewed_mode_fails_closed() {
    let cwd = std::fs::canonicalize(std::env::current_dir().expect("current dir"))
        .expect("canonical current dir");
    let member = TeamMemberSpec {
        agent_member_id: "agent-unreviewed".into(),
        name: "Unreviewed".into(),
        role: "builder".into(),
        provider: "not-a-registered-provider".into(),
        execution_mode: None,
        model: None,
        effort: None,
        service_tier: None,
        provider_cwd_hint: None,
        owned_paths: Vec::new(),
        resume_native_session_id: Some("thread-from-nowhere".into()),
        initial_work: None,
    };
    let refused = build_member_run_for_team(None, "team-run-unreviewed", &member, cwd.to_str())
        .expect_err("an unreviewed provider/mode cannot seed a provider-native pointer");
    assert!(
        refused
            .to_string()
            .contains("RESUME_NATIVE_SESSION_UNSUPPORTED"),
        "{refused}"
    );

    // Without a resume seed the same member still builds: the refusal is about
    // naming a provider-native session, not about the provider existing.
    let mut without_seed = member.clone();
    without_seed.resume_native_session_id = None;
    let built = build_member_run_for_team(None, "team-run-unreviewed", &without_seed, cwd.to_str())
        .expect("an unreviewed provider without a resume seed is unaffected");
    assert_eq!(built.native_session, None);
}

/// `provider_native_session_ref` refuses the same way, and names the mode.
#[test]
fn provider_native_session_ref_refuses_an_unreviewed_provider() {
    let refused = provider_native_session_ref("not-a-registered-provider", "thread-from-nowhere")
        .expect_err("an unreviewed provider has no locator kind to claim");
    assert!(
        refused
            .to_string()
            .contains("NATIVE_LOCATOR_KIND_UNREVIEWED"),
        "{refused}"
    );
    for provider in ["codex", "kimi", "claude", "deepseek_harness"] {
        let built = provider_native_session_ref(provider, "thread-1")
            .unwrap_or_else(|error| panic!("{provider} is reviewed and must build: {error}"));
        assert_eq!(
            Some(built.native_locator_kind.as_str()),
            harness_core::native_locator_kind_for_mode(provider, &built.execution_mode),
            "{provider} must carry the kind its own adapter will produce"
        );
    }
}

/// The five reviewed Team modes still seed a resume pointer, and each carries
/// the kind its adapter produces — the property that was broken for pi and
/// deepseek_harness, whose seeds carried `provider_native`.
#[test]
fn every_reviewed_mode_seeds_the_kind_its_adapter_produces() {
    let cwd = std::fs::canonicalize(std::env::current_dir().expect("current dir"))
        .expect("canonical current dir");
    for (provider, execution_mode, expected_kind) in [
        ("codex", "codex_app_server", "codex_rollout"),
        ("kimi", "kimi_acp", "kimi_code_session"),
        ("claude", "claude_agent_sdk", "claude_project_session"),
        (
            "deepseek_harness",
            "deepseek_sdk",
            "deepseek_harness_session",
        ),
        ("pi", "pi_rpc", "pi_session"),
    ] {
        let member = TeamMemberSpec {
            agent_member_id: format!("agent-{provider}"),
            name: format!("Member{provider}"),
            role: "builder".into(),
            provider: provider.into(),
            execution_mode: Some(execution_mode.into()),
            model: None,
            effort: None,
            service_tier: None,
            provider_cwd_hint: None,
            owned_paths: Vec::new(),
            resume_native_session_id: Some(format!("thread-{provider}")),
            initial_work: None,
        };
        let run = build_member_run_for_team(None, "team-run-reviewed", &member, cwd.to_str())
            .unwrap_or_else(|error| panic!("{provider}/{execution_mode} must seed: {error}"));
        let seeded = run
            .native_session
            .expect("a reviewed mode seeds its resume pointer");
        assert_eq!(
            seeded.native_locator_kind, expected_kind,
            "{provider}/{execution_mode} seeded the wrong locator kind"
        );
        assert_eq!(seeded.execution_mode, execution_mode);
    }
}

/// `external_interactive` is the user's own already-open session: Harness never
/// locates it, so it must not reach the locator table at all. Today that is
/// guaranteed only by `validate_team_member_execution_mode` running before
/// `build_member_run_for_team`; nothing pinned that ordering.
#[test]
fn external_interactive_is_rejected_before_it_can_reach_the_locator_table() {
    let member = TeamMemberSpec {
        agent_member_id: "agent-external".into(),
        name: "ExternalHost".into(),
        role: "host".into(),
        provider: "codex".into(),
        execution_mode: Some("external_interactive".into()),
        model: None,
        effort: None,
        service_tier: None,
        provider_cwd_hint: None,
        owned_paths: Vec::new(),
        resume_native_session_id: Some("thread-user-owned".into()),
        initial_work: None,
    };
    let refused = validate_team_member_execution_mode(&member)
        .expect_err("an external interactive session is never resumed by Harness");
    assert!(
        !refused
            .to_string()
            .contains("RESUME_NATIVE_SESSION_UNSUPPORTED"),
        "the external_interactive refusal must keep its own reason, not the locator one: {refused}"
    );
    assert_eq!(
        harness_core::native_locator_kind_for_mode("codex", "external_interactive"),
        None,
        "external_interactive has no locator kind, so a leak past the guard also fails closed"
    );
}
