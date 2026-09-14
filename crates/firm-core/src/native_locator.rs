//! The one table that names the provider-native locator kind for every
//! reviewed Agent Team execution mode.
//!
//! `native_locator_kind` is not a label. It is part of
//! [`NativeSessionRef::same_identity_as`](crate::NativeSessionRef::same_identity_as)
//! and part of the persisted-session read fingerprint, so a seeded pointer that
//! guesses a different kind than its adapter will produce is a pointer that
//! fails identity comparison the moment the provider reports back. Before this
//! table there were three independent spellings: each adapter's own
//! `native_locator_kind()`, a `provider_native_session_ref` match with no `pi`
//! arm defaulting to `"provider_native_session"`, and a `--resume-member` match
//! covering only codex/kimi/claude defaulting to `"provider_native"`.
//!
//! Every producer now reads THIS table, and a compile-linked parity test in
//! `firm-cli` asserts that each adapter's `native_locator_kind()` equals the
//! entry recorded here.

/// One reviewed (provider, execution_mode) pair and the locator kind its
/// adapter produces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NativeLocatorKindEntry {
    pub provider: &'static str,
    pub execution_mode: &'static str,
    pub native_locator_kind: &'static str,
}

/// The Codex Team runtime adapter.
pub const CODEX_APP_SERVER: NativeLocatorKindEntry = NativeLocatorKindEntry {
    provider: "codex",
    execution_mode: "codex_app_server",
    native_locator_kind: "codex_rollout",
};

/// The NodeDaemon-owned Codex session adapter — a different adapter from the
/// Team-runtime one above. It keeps its own kind because collapsing the two
/// would silently change the identity of every row already written under
/// either name.
pub const CODEX_NODE_DAEMON_APP_SERVER: NativeLocatorKindEntry = NativeLocatorKindEntry {
    provider: "codex",
    execution_mode: "node_daemon_app_server",
    native_locator_kind: "codex_thread",
};

/// The Kimi Team runtime adapter.
pub const KIMI_ACP: NativeLocatorKindEntry = NativeLocatorKindEntry {
    provider: "kimi",
    execution_mode: "kimi_acp",
    native_locator_kind: "kimi_code_session",
};

/// The Claude Team runtime adapter.
pub const CLAUDE_AGENT_SDK: NativeLocatorKindEntry = NativeLocatorKindEntry {
    provider: "claude",
    execution_mode: "claude_agent_sdk",
    native_locator_kind: "claude_project_session",
};

/// The DeepSeek Harness Team runtime adapter.
pub const DEEPSEEK_SDK: NativeLocatorKindEntry = NativeLocatorKindEntry {
    provider: "deepseek_harness",
    execution_mode: "deepseek_sdk",
    native_locator_kind: "deepseek_harness_session",
};

/// The Pi Team runtime adapter.
pub const PI_RPC: NativeLocatorKindEntry = NativeLocatorKindEntry {
    provider: "pi",
    execution_mode: "pi_rpc",
    native_locator_kind: "pi_session",
};

/// Every reviewed (provider, execution_mode) pair. Each adapter's own
/// `native_locator_kind()` returns its entry above, so the kind exists as ONE
/// literal that both the adapter and every seeding path read.
pub const NATIVE_LOCATOR_KINDS: &[NativeLocatorKindEntry] = &[
    CODEX_APP_SERVER,
    CODEX_NODE_DAEMON_APP_SERVER,
    KIMI_ACP,
    CLAUDE_AGENT_SDK,
    DEEPSEEK_SDK,
    PI_RPC,
];

/// The locator kind this exact (provider, execution_mode) adapter produces.
///
/// `None` means no reviewed adapter produces a provider-native session for that
/// pair — a retired one-shot mode, `external_interactive` (whose session is the
/// user's own and is never located by Harness), or an unregistered provider.
/// Callers must fail closed on `None` rather than substitute a placeholder:
/// a placeholder kind is exactly what made a seeded `pi` pointer unmatchable.
pub fn native_locator_kind_for_mode(provider: &str, execution_mode: &str) -> Option<&'static str> {
    NATIVE_LOCATOR_KINDS
        .iter()
        .find(|entry| entry.provider == provider && entry.execution_mode == execution_mode)
        .map(|entry| entry.native_locator_kind)
}

/// The locator kind for a provider that has exactly one reviewed mode.
///
/// Codex has two (`codex_app_server` and the NodeDaemon-owned
/// `node_daemon_app_server`), so it resolves to `None` here on purpose: a
/// caller that knows only "codex" does not know which adapter will answer and
/// must resolve the mode first.
pub fn native_locator_kind_for_provider(provider: &str) -> Option<&'static str> {
    let mut matches = NATIVE_LOCATOR_KINDS
        .iter()
        .filter(|entry| entry.provider == provider);
    let first = matches.next()?;
    match matches.next() {
        None => Some(first.native_locator_kind),
        Some(_) => None,
    }
}
