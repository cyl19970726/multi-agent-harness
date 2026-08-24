# Harness CLI Map

status: stable  
owner: lead-operations  
last reviewed: 2026-08-17

This map records the current `target/debug/firm` command surface. It separates
implemented CLI from API/store-backed capability that does not yet have a
dedicated CLI. The legacy Company OS command tree (`company ...`) and the
Mission writers were retired by DOC-108; they fail with an explicit retired
error and are documented here only as retired compatibility.

## Status labels

- **Implemented**: routed by `crates/firm-cli/src/main.rs` and callable from
  the compiled `harness` binary.
- **Partial**: supported by store/API/UI/scripts, but the CLI is incomplete or
  only covers metadata/control slices.
- **Missing / next**: expected product surface with no dedicated CLI command yet.
- **Retired compatibility**: retained only to read, export, or verify
  historical data.

## Top-level command map

| Area | Commands | Status | Notes |
| --- | --- | --- | --- |
| Execution Space / Project Binding routing | `init`, `space init/list/current/switch/show/migrate-from-project`, `project add/list/current/switch/remove/show/migrate` | Implemented | `--space` selects Agent Team/Workflow coordination storage; `--project` independently selects provider cwd, instructions/Skills, Git/worktree and permission boundaries. Raw store overrides and project-derived execution stores are compatibility paths only. |
| Agent Team | `team create/list/show/rename/add-member/remove-member/activate-member/activate/deactivate/trash/restore`, `team message send/inbox/claim` | Implemented | Defines one durable flat Team with required Host AgentMember and immutable ExecutionNode placement; Mission provenance is optional and legacy-only. Peer-Team messaging sends into the shared Team Inbox over the canonical Message/subscription/delivery fabric. |
| Durable Agent Organization identity | `org host`, `org cutover-audit` | Implemented foundation | Durable AgentMember is separate from MemberRun/native Session. `cutover-audit` refuses ambiguous Host authority and audits legacy Mission provenance only when present. |
| Agent Team run | `team-run create/list/status/work/recover/host-inbox/dispatch-host/bind-host/host-lease-status/renew-host-lease/release-host-lease/inbox/add-member/rename-member/interrupt-member/close-member/reopen-member/deactivate-member/start/send/answer-message/events/wait/board-summary/complete/cancel` | Implemented | Runtime control plane for shared Works, persistent MemberRuns, WorkDelivery, identity-first Message, and per-recipient CanonicalMessageDelivery. Provider questions and answers are correlated Message kinds. Legacy `ack`/TeamMessage delivery mutations are not current authority. Permissions are frozen at AgentSession start; no second permission workflow is exposed. Interrupt stops only the current provider turn. Close freezes coordination and releases a managed runtime; Reopen resumes the same MemberRun/native session; Deactivate retires permanently. Bound agents use `member runtime interrupt`; the exact Host may target another active MemberRun, while an ordinary Member may target only itself. |
| Agent Team member detail | `member-run show --id <member-run-id> [--json]`; `member-run open-native --id <member-run-id> [--print-only] [--json]` | Implemented | Joins one MemberRun, TeamRun, owned/eligible Works, WorkDelivery, Inbox/Outbox, actions, Workspace and provider-native session locator without copying the provider transcript. `open-native` is provider-neutral fail-closed routing. |
| Members/providers | `member providers/preflight/inbox/message/work` | Implemented | Two separate axes. `member providers` reviews ADAPTER compatibility against the installed provider version. `member preflight` reports execution-mode-specific ACCOUNT capacity as a sibling of compatibility, never merged into it. See [integration/provider-capacity.md](../integration/provider-capacity.md). |
| AgentMember execution trust | `member-trust mutate` plus TeamRun supervisor delivery | Implemented kernel | Canonical AgentMember, MemberRun, Message, delivery, Report, Finding, Evaluation, Gate, and Work acceptance mutations share one operation ledger and explicit authority context. The retired `agent` registry routes are not an execution authority. |
| Global Work read | `work list` | Implemented | The read-only Global Work aggregate filters the one Work authority by accountable Team, assignee TeamMembership, phase, condition, resolution, priority, and owner; preserves exact ids and revisions. |
| Dynamic Workflow | Former `workflow` command family | Retired | No current command route. Historical records are available only through legacy archive export, verify, and restore-read. |
| Dashboard | `dashboard snapshot`, `dashboard doctor` | Implemented | Operator projection and read-only convergence checks. |
| Serve/API | `serve [--addr] [--once]`, `mcp`, `node init/list/show/drain/retire/project`, `daemon start/status/stop/serve`, `hook record` | Implemented | One machine-scoped NodeDaemon supervises every admitted local TeamRun across registered Execution Spaces. All public start surfaces delegate to it and fail explicitly when unavailable; there is no per-run fallback. `hook record` is a compatibility ingress that validates the bound AgentMember and discards the provider frame. |
| Mission legacy reads | `mission list`, `mission show`, `mission log show` | Retired compatibility | Read-only legacy reads over historical rows (DOC-108). `mission create/update-context/close/log append` and the `/v1/missions*` POST routes and `mission_*` MCP writers fail with an explicit DOC-108 retired-write error. |
| Legacy Wave archive | `legacy wave list/show/history` | Retired compatibility | `wave create/update/advance/gate` and HTTP writes return the ADR 0051 retirement error. MCP publishes no `wave_*` capability at all. Existing `waves.jsonl` rows remain readable without becoming current planning context; no data migration. |
| Legacy Company OS export | `legacy-company-os export`, `legacy-company-os verify` | Retired compatibility | Stage A export/verify of every Company Store, Execution Space, compatibility, and machine store. The `company ...` command tree and the `/v1/company-os/*` routes are retired (410 tombstones); historical data is export/verify-only. |
| Historical migration | `legacy-goal-task export`, `legacy-goal-task verify` | Retired compatibility | Export/verify only. |
| Retired command families | old `goal`, `phase`, `task`, proposal/review/design surfaces | Retired compatibility | These fail explicitly and must not be used for new work. |

## Skills and install status

Active repository skills:

- `skills/collaborate-as-agent-team-member` (mirrored into the Star Harness plugin)
- `skills/shared-references` (cross-reference, mirrored)
- `skills/bootstrap-project-workflow`

The retired Company OS operator skills and the Mission-orchestration skill were
archived to `archive/skills/` with the DOC-108 cutover and must not be
installed or treated as current operating contracts. Skills are
distribution/operator guidance. They are not the authority for product
architecture; canonical truth remains schemas, store/API, CLI routing, UI,
tests and ADRs.

## Current validation commands

| Check | What it proves |
| --- | --- |
| `pnpm check` | Umbrella: JSON validity, schema fixtures, provider events, collaboration foundation, member execution trust, runtime message fabric, remote fabric, tool descriptors, native-session boundary, plugin contract, cross-layer consistency, role views, dashboard. |
| `pnpm acceptance:legacy-retirement` | Deterministic Agent Team, MCP, Kimi ACP adapter, and Dashboard contracts plus the retired Mission/Wave legacy reads and retired-write errors. |
| `firm governance check` | Documentation registry/link/retired-surface governance. |

## Recommended CLI roadmap

1. **Global Work depth**: pagination and richer filters on the read-only
   aggregate as the RoleView contract grows.
2. **External source observation**: sync/projection-first connectors that
   attach external software-delivery evidence to TeamWork without becoming
   commercial truth.
