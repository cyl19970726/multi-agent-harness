use super::*;

/// `native_locator_kind` is part of identity comparison and of the persisted
/// session read fingerprint. Three independent spellings used to exist, two of
/// which fell back to a placeholder ("provider_native", "provider_native_session")
/// for modes they did not list — a seeded `pi` pointer could therefore never
/// match the `pi_session` kind its own adapter produces.
#[test]
fn native_locator_kind_table_is_the_one_source_of_the_kind() {
    for (provider, execution_mode, expected) in [
        ("codex", "codex_app_server", "codex_rollout"),
        ("codex", "node_daemon_app_server", "codex_thread"),
        ("kimi", "kimi_acp", "kimi_code_session"),
        ("claude", "claude_agent_sdk", "claude_project_session"),
        (
            "deepseek_harness",
            "deepseek_sdk",
            "deepseek_harness_session",
        ),
        ("pi", "pi_rpc", "pi_session"),
    ] {
        assert_eq!(
            native_locator_kind_for_mode(provider, execution_mode),
            Some(expected),
            "{provider}/{execution_mode} must resolve to the kind its adapter emits"
        );
    }

    // Retired one-shot modes, `external_interactive`, and unregistered
    // providers produce no provider-native session, so they resolve to None and
    // every caller fails closed instead of inventing a placeholder kind.
    for (provider, execution_mode) in [
        ("codex", "codex_exec"),
        ("claude", "claude_cli"),
        ("codex", "external_interactive"),
        ("unregistered", "whatever"),
    ] {
        assert_eq!(
            native_locator_kind_for_mode(provider, execution_mode),
            None,
            "{provider}/{execution_mode} has no reviewed adapter and must not be guessed"
        );
    }
}

#[test]
fn locator_kind_by_provider_alone_refuses_a_provider_with_two_reviewed_modes() {
    assert_eq!(
        native_locator_kind_for_provider("kimi"),
        Some("kimi_code_session")
    );
    assert_eq!(native_locator_kind_for_provider("pi"), Some("pi_session"));
    assert_eq!(
        native_locator_kind_for_provider("codex"),
        None,
        "codex has both the Team runtime and the NodeDaemon-owned adapter; the mode must be resolved first"
    );
    assert_eq!(native_locator_kind_for_provider("unregistered"), None);
}

/// Every kind in the table is distinct: two adapters sharing one kind would
/// make their sessions compare as the same identity.
#[test]
fn every_reviewed_mode_has_its_own_locator_kind() {
    let mut kinds: Vec<&str> = NATIVE_LOCATOR_KINDS
        .iter()
        .map(|entry| entry.native_locator_kind)
        .collect();
    let total = kinds.len();
    kinds.sort_unstable();
    kinds.dedup();
    assert_eq!(
        kinds.len(),
        total,
        "locator kinds must be unique per adapter"
    );
}
