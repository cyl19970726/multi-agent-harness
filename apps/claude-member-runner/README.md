# Claude Member Runner

A persistent Agent Team member backed by the [Claude Agent SDK](https://code.claude.com/docs/en/agent-sdk).
One process per `MemberRun`, one provider-native Claude session that stays alive
until the Host accepts the handoff.

> Status: **skeleton, `review_required`.** Lifecycle, gates, and the stdio
> protocol are covered by deterministic tests against a fake SDK. Nothing here
> has run against a real provider yet, so per AGENTS.md this execution mode is
> not a reviewed-compatible adapter.

## Why this exists

`run_claude_team_member` in `harness-cli` spawns `claude -p "<envelope>"` once
per delivery. A member therefore stops existing between deliveries, and
`docs/integration/claude.md` honestly records the consequences: no interrupt, no
mid-turn control channel, no harness-managed hooks.

That is the Claude-side form of the same defect the Rust loop has:

```rust
let queued = ledger.queued_messages_for(&member.id)?;
if queued.is_empty() { break; }   // member terminates on a momentarily empty queue
```

A message arriving after that instant has no recipient. It stays `queued`
forever. This is the runtime half of the unmet clause in ADR 0037 — *"Member …
owns its plan, Workspace, session … until the Team Lead accepts its handoff
through a `review_result`"* — and it is why acceptance items 5 and 6 have no
test anywhere in the repo.

## Mapping to the model

| Contract | Agent SDK primitive |
| --- | --- |
| Member persists across turns and Waves | `query({ prompt: AsyncIterable })` streaming input |
| Mailbox delivery into a live member | `Mailbox.push()` feeding that iterable |
| Interrupt with a real acknowledgement | `query.interrupt()` → `still_queued` |
| Steer | `query.setPermissionMode()` / `setModel()` |
| `native_session_id` binding | `system/init` → `session_id` |
| Member registry (no second roster) | `tagSession(id, "<team_run_id>:<member_run_id>")` |
| Member discovery | `listSessions()` filtered by that tag |
| Member detail page activity | `getSessionMessages(id)` — read on demand, never mirrored |
| Retry without polluting the original | `forkSession: true` |
| owned-paths enforcement (ADR 0033) | `PreToolUse` deny |
| Plan gate (ADR 0038) | `PreToolUse` deny until `plan_approval` |
| `evidence_refs` (Issue #232) | `PostToolUse` observation |

`tagSession` is the part worth noticing: the member roster lives in the
provider's own session registry, so Harness stores the binding and nothing else.
That is stricter adherence to ADR 0032 than the current `MemberRun.native_session`
locator, not looser.

## Protocol

NDJSON on stdio, one process per member. `harness-cli` writes commands, reads
events. stderr is diagnostics and is never parsed.

Commands: `start`, `deliver`, `interrupt`, `set_permission_mode`, `close`.
Events: `member_started`, `session_bound`, `assistant_message`, `turn_complete`,
`turn_idle`, `delivered`, `interrupted`, `permission_mode_changed`,
`plan_approved`, `plan_gate_blocked`, `owned_path_violation`,
`registry_write_failed`, `member_closed`, `runner_error`.

**`turn_complete` is not a lifecycle event.** Only an explicit `close` produces
`member_closed`. Collapsing those two is the bug this runner exists to remove.

`start` payload:

```jsonc
{
  "teamRunId": "trun-…", "memberRunId": "mrun-…",
  "memberName": "RuntimeBuilder", "roleLabel": "Runtime owner",
  "cwd": "/abs/path/to/worktree",          // member execution root
  "ownedPaths": ["crates/harness-cli"],    // [] = no restriction
  "allowedTools": ["Read", "Edit", "Bash"],
  "disallowedTools": [],
  "permissionMode": "default",
  "settingSources": ["project", "user"],   // loads the project's CLAUDE.md + skills
  "planRequired": true,                    // ADR 0038 approval gate
  "resumeSessionId": null, "forkSession": false
}
```

## Run it

```bash
# Deterministic lifecycle tests — no credentials, no SDK install needed
node --test "apps/claude-member-runner/test/*.test.mjs"

# Dry run of the whole protocol against the fake SDK
printf '%s\n' \
  '{"command":"start","payload":{"teamRunId":"t","memberRunId":"m","memberName":"Demo","cwd":"/tmp/p"}}' \
  '{"command":"deliver","payload":{"id":"m1","kind":"assignment","from_member_id":"host","body":"hi"}}' \
  '{"command":"close","payload":{"reason":"review_result_accepted"}}' \
  | node apps/claude-member-runner/bin/claude-member-runner.mjs --fake
```

Live execution additionally needs `pnpm add @anthropic-ai/claude-agent-sdk` and
valid provider credentials. Neither is done here: adding a provider dependency
and re-authenticating are Human decisions under AGENTS.md.

## Not done yet

- No Rust caller. `run_claude_team_member` still runs the `-p` loop; wiring it
  to spawn this process is the next change.
- No live canary. Until one runs, `claude_agent_sdk` stays `review_required` and
  must not be presented as a reviewed-compatible mode.
- `getSessionMessages` projection for the Member detail page is not wired here —
  it belongs on the read path (`GET /v1/member-runs/{id}/native-activity`).
- Auth policy: the Agent SDK docs state Anthropic does not permit third-party
  products to offer claude.ai login or rate limits without prior approval.
  Self-hosted dogfooding on the operator's own login is a different case from
  distributing the harness; that decision is not a code change.
