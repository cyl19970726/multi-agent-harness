use super::*;

/// Provider capabilities declaration: what a platform can technically support.
///
/// This is distinct from member-level `ProviderLaunchProfile.capabilities` (intent: what
/// the member is *meant* to do). This declares what the *platform* can do
/// (streaming, resume, mid-turn approval, subagents, MCP, hooks).
///
/// See Pillar 3 and the capability declaration table in
/// docs/agent-integration-model.md for the current capability set per provider.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderCapabilities {
    /// Platform supports incremental event stream during a turn.
    pub streaming: bool,
    /// Platform supports session resume (`--session`, `--resume`, etc).
    pub resume: bool,
    /// Platform supports mid-turn tool approval/denial (approve/reject before execution).
    pub mid_turn_approval: bool,
    /// Platform supports native child threads / subagents.
    pub subagents: bool,
    /// Platform supports MCP server attachment.
    pub mcp: bool,
    /// Platform supports lifecycle hooks.
    pub hooks: bool,
    /// Platform supports a NATIVE structured-output / JSON-schema flag (codex
    /// `--output-schema`, claude `--json-schema`). When `false`, schema-mode
    /// nodes degrade to the prompt-coaxed text-extraction fallback rather than a
    /// special code path (goal-provider-neutral capability matrix: `schema`).
    /// Defaults to `false` for providers that don't declare it.
    #[serde(default)]
    pub schema: bool,
    /// Platform reports billed USD in its terminal frame (claude
    /// `result.total_cost_usd`; codex reports token usage only). When `false`,
    /// spend degrades to a token-based estimate or `null` (goal-provider-neutral
    /// capability matrix: `cost`). Defaults to `false`.
    #[serde(default)]
    pub cost: bool,
    /// Platform can run a leaf that is PHYSICALLY prevented from mutating the
    /// workspace — codex `--sandbox read-only`, claude a read-only tool allowlist
    /// (`Read,Grep,Glob`). When `false` the provider has NO read-only mode (kimi's
    /// headless `kimi -p` rejects every permission flag), so a read-only leaf must be
    /// isolated in a throwaway worktree to keep its writes off the live repo rather
    /// than trusted to stay read-only. Defaults to `false` = assume-unenforceable
    /// (the safe default: isolate an unknown provider's read-only leaves too).
    #[serde(default)]
    pub enforces_read_only: bool,
}

impl ProviderCapabilities {
    /// Codex exec capabilities per the capability declaration table in
    /// docs/agent-integration-model.md.
    pub fn codex_exec() -> Self {
        ProviderCapabilities {
            streaming: true,          // --json NDJSON
            resume: true,             // --session
            mid_turn_approval: false, // policy pre-approve only
            subagents: true,          // observed in Codex
            mcp: true,                // --config mcp_servers.*
            hooks: false,             // limited in exec mode
            schema: true,             // --output-schema <file>
            cost: false,              // token usage only, no total_cost_usd
            enforces_read_only: true, // --sandbox read-only
        }
    }

    /// Claude exec capabilities per the capability declaration table.
    pub fn claude_exec() -> Self {
        ProviderCapabilities {
            streaming: true,          // --output-format stream-json
            resume: true,             // --resume
            mid_turn_approval: false, // not documented for -p; Tier-3 only
            subagents: true,          // observed in Claude
            mcp: true,                // --mcp-config JSON
            hooks: false,             // not documented
            schema: true,             // --json-schema → result.structured_output
            cost: true,               // result.total_cost_usd
            enforces_read_only: true, // --allowedTools Read,Grep,Glob (no Edit/Write/Bash)
        }
    }

    /// Kimi exec capabilities (goal-provider-neutral S4) — a HONEST, partly
    /// UNKNOWN preset for a provider whose live CLI has not been verified.
    ///
    /// ASSUMES the `kimi` CLI is invoked like claude (stream-json NDJSON, a
    /// terminal `result` frame), so `streaming` is the only axis claimed `true`.
    /// Every other axis is marked `false` = DEGRADED-until-proven, NOT a positive
    /// claim of absence: resume/MCP/schema/cost/hooks all need to be confirmed
    /// against the real binary (see the goal's S3 spike) before being flipped on.
    /// Marking them `false` is the safe default — a missing axis degrades to the
    /// shared fallback (text-extract for schema, token-estimate for cost,
    /// leaf-only for resume) rather than a per-provider branch.
    pub fn kimi_exec() -> Self {
        ProviderCapabilities {
            streaming: true,          // assumed: --output-format stream-json
            resume: false,            // UNKNOWN: resumable session id unverified
            mid_turn_approval: false, // UNKNOWN
            subagents: false,         // UNKNOWN
            mcp: false,               // UNKNOWN
            hooks: false,             // UNKNOWN: no lifecycle hook bridge
            schema: false,            // UNKNOWN: degrade to text-extract fallback
            cost: false,              // UNKNOWN: degrade to token-estimate
            // VERIFIED false: `kimi -p` rejects every permission flag (-y/--auto/
            // --plan) and has no tool allowlist, so it has NO read-only mode. A
            // read-only kimi leaf must be worktree-isolated, not trusted (the live
            // CLI was confirmed to edit the shared tree from a read-only leaf).
            enforces_read_only: false,
        }
    }

    /// Check if all critical capabilities for basic streaming exec are present.
    pub fn supports_streaming_exec(&self) -> bool {
        self.streaming && !self.mid_turn_approval
    }
}

impl std::fmt::Display for ProviderCapabilities {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let features = [
            ("streaming", self.streaming),
            ("resume", self.resume),
            ("mid_turn_approval", self.mid_turn_approval),
            ("subagents", self.subagents),
            ("mcp", self.mcp),
            ("hooks", self.hooks),
            ("schema", self.schema),
            ("cost", self.cost),
            ("enforces_read_only", self.enforces_read_only),
        ];
        let enabled: Vec<&str> = features
            .iter()
            .filter_map(|(name, enabled)| if *enabled { Some(*name) } else { None })
            .collect();
        write!(f, "{{{}}}", enabled.join(", "))
    }
}
