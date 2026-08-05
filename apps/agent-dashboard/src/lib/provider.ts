/**
 * Provider display helpers. The store carries raw provider ids (`kimi`,
 * `codex`, `claude`); UI surfaces show a human product name instead. Unknown
 * ids render verbatim so future adapters (e.g. `pi`) appear without changes.
 */
const PROVIDER_DISPLAY_NAMES: Record<string, string> = {
  kimi: "Kimi Code",
  codex: "Codex",
  claude: "Claude Code",
  pi: "Pi",
};

/**
 * The registered persistent bidirectional provider modes (mirrors the backend
 * `validate_team_member_execution_mode` registry). Every member/team creation
 * form sources its provider options from this single constant, so adding a
 * provider is one registry entry, not a per-form edit. The display map above
 * stays the only hardcoded presentation layer.
 */
export const TEAM_MEMBER_PROVIDER_MODES: Array<{ provider: string; label: string; mode: string }> = [
  { provider: "kimi", label: "Kimi Code", mode: "kimi_acp" },
  { provider: "codex", label: "Codex", mode: "codex_app_server" },
  { provider: "claude", label: "Claude Code", mode: "claude_agent_sdk" },
  { provider: "pi", label: "Pi", mode: "pi_rpc" },
];

export function providerDisplayName(provider?: string | null): string {
  if (!provider) return "provider unset";
  return PROVIDER_DISPLAY_NAMES[provider.toLowerCase()] ?? provider;
}

/** One-line execution stack for compact cards: "Kimi Code · kimi_acp · model". */
export function providerStackLine(
  provider?: string | null,
  executionMode?: string | null,
  model?: string | null,
): string {
  return [providerDisplayName(provider), executionMode, model].filter(Boolean).join(" · ");
}

/**
 * Model label backed by provider control verification. The effective value
 * wins; a requested-but-unconfirmed value carries its status; a bare stored
 * model is explicitly unverified. Shared by the focus hero and every card so
 * the same member never shows two different model truths.
 */
export function memberModelLabel(member: {
  provider_controls?: {
    model?: { effective?: string | null; requested?: string | null; status?: string | null } | null;
  } | null;
  model?: string | null;
}): string | undefined {
  const control = member.provider_controls?.model;
  if (control?.effective) return control.effective;
  if (control?.requested) return `${control.requested} (${control.status ?? "requested"})`;
  if (member.model) return `${member.model} (unverified)`;
  return undefined;
}

/**
 * Same-turn Steer is a live-control capability. Today only the Codex app
 * server mode implements it; keeping the predicate and its user-facing
 * reasons in one place lets every surface gate consistently.
 */
export function liveSteerCapability(member: {
  provider_profile?: { execution_mode?: string | null } | null;
  status?: string | null;
}): { allowed: boolean; reason?: string } {
  const mode = member.provider_profile?.execution_mode;
  if (mode !== "codex_app_server") {
    return { allowed: false, reason: `${mode ?? "This provider mode"} does not support same-turn Steer.` };
  }
  if (member.status !== "running") {
    return { allowed: false, reason: "Steer is available only while this Codex member has an active turn." };
  }
  return { allowed: true };
}
