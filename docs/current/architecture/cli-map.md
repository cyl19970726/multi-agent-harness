# Harness CLI Map

status: stable  
owner: lead-operations  
last reviewed: 2026-08-09

This map records the current `target/debug/firm` command surface. It separates
implemented CLI from API/store-backed capability that does not yet have a
dedicated CLI.

## Status labels

- **Implemented**: routed by `crates/firm-cli/src/main.rs` and callable from
  the compiled `harness` binary.
- **Partial**: supported by store/API/UI/scripts, but the CLI is incomplete or
  only covers metadata/control slices.
- **Missing / next**: expected product surface with no dedicated CLI command yet.
- **Retired compatibility**: retained only to export or verify historical data.

## Top-level command map

| Area | Commands | Status | Notes |
| --- | --- | --- | --- |
| Execution Space / Project Binding routing | `init`, `space init/list/current/switch/show/migrate-from-project`, `project add/list/current/switch/remove/show/migrate` | Implemented | `--space` selects Mission/Wave/Agent Team/Workflow storage; `--project` independently selects provider cwd, instructions/Skills, Git/worktree and permission boundaries. Raw store overrides and project-derived execution stores are compatibility paths only. |
| Company Store routing | `company init/list/current/switch/show/migrate-from-project`, global `--company <id>` for `company ...`, `HARNESS_COMPANY` | Implemented | ADR 0042 Phase 2 first slice. `firm company ...` uses the selected Company Store when explicit/current Company exists; execution commands still use Project routing. |
| Mission | `mission create/list/show/update-context/close`, `mission log append/show` | Implemented | Current durable intent surface. One Team is created through `team create --mission-id ...`; retired link/unlink and Mission-scoped create-team commands are absent. `mission log append --mission-id <id> --kind judgment\|replan\|recovery\|closeout_evidence --body <markdown>` and `mission log show --mission-id <id> [--tail <n>]` are the ADR 0051 append-only Mission Log that absorbed Wave as the Host's judgment record. |
| Wave | `wave list/show/history` | Implemented (historical reads only) | `wave create/update/advance/gate` retired by the ADR 0051 Mission Log cutover — the CLI, HTTP (`/v1/waves`...), and MCP (`wave_create`...) surfaces all return the same retirement error pointing at `mission log append`. Existing Wave rows remain readable as historical context; no data migration. |
| Agent Team | `team create/list/show/rename/add-member/remove-member/close/archive` | Implemented | Defines one flat Team for one Mission with required Host Agent and immutable ExecutionNode placement. |
| Durable Agent Organization identity | `org member create/converge/list/show`, `org bootstrap-lead`, `org host`, `org cutover-audit` | Implemented foundation | ADR 0052 CLI/store slice. Durable AgentMember is separate from MemberRun/native Session. `converge` is explicit and deterministic; `cutover-audit` refuses compatibility-only, missing, or conflicting Host authority. HTTP/MCP/UI and full Organization/Work cutover remain pending. |
| Agent Team run | `team-run create/list/status/work/inbox/host-inbox/dispatch-host/bind-host/host-lease-status/renew-host-lease/release-host-lease/ack/reconcile-delivery/add-member/rename-member/close-member/reopen-member/deactivate-member/start/send/resolve-interaction/events/complete/cancel` | Implemented | Runtime control plane for shared Works, persistent MemberRuns, WorkDelivery, and typed conversation. `bind-host` durably records the exact Host surface/thread independently from `HostBindingLease`; an Interactive Codex lease requires an exact parsed `session_meta.payload.id` in the canonical default `<HOME>/.codex/sessions` rollout store. Filename matches, `CODEX_THREAD_ID`, and caller-controlled `CODEX_HOME` are not validation evidence. This same-user filesystem check proves rollout existence only—not live attachment, exclusive ownership, authentication, or successful wake. Unsupported or unverifiable Claude/Kimi/manual identity remains bound but unleased with an actionable warning. Lease status/renew/release expose TTL and exact owner/lease/generation fencing. A live Interactive lease suppresses dispatcher scheduling. `dispatch-host` is the bounded one-shot execution seam: it takes an exact Dispatcher lease, atomically claims the actual eligible attention batch, resumes the exact bound provider session with a read-only triage prompt, records a real provider receipt, requeues failures, and releases its lease. Kimi permission requests fail closed and Claude reapplies read-only mode; Codex headless dispatch fails closed because exact-session resume cannot currently reapply a sandbox. Persistent multi-TeamRun polling is isolated to #415 and must reuse this seam; terminal accept/merge/cancel remains interactive Host work. `team-run work` exposes list/show/create/assign/claim/start/block/submit/request-changes/accept/cancel. Close freezes coordination and releases a managed runtime; Reopen resumes the same MemberRun/native session; Deactivate retires permanently. |
| Agent Team member detail | `member-run show --id <member-run-id> [--json]`; `member-run open-native --id <member-run-id> [--print-only] [--json]` | Implemented | Joins one MemberRun, TeamRun, owned/eligible Works, WorkDelivery, Inbox/Outbox, actions, PendingInteractions, Workspace and provider-native session locator without copying the provider transcript. `open-native` is provider-neutral fail-closed routing; the currently verified UI target is explicit Claude Agent SDK → Claude Desktop import on macOS. |
| Members/providers | `member register/list/providers/preflight` | Implemented | Two separate axes. `member providers` reviews ADAPTER compatibility against the installed provider version. `member preflight [--provider <name>] [--execution-mode <mode>] [--canary] [--timeout-s <n>] [--fail-on-unavailable]` reports execution-mode-specific ACCOUNT capacity (state, account/source boundary, observed_at, reset_at, evidence source, confidence) as a sibling of compatibility, never merged into it. See [integration/provider-capacity.md](../integration/provider-capacity.md). |
| Standing Agent runtime | `agent create/list/show/start/health/send/route-inbox/deliver/retry-delivery/reconcile-delivery/gateway/close` | Implemented | Current standing-agent operational CLI. `route-inbox` idempotently joins stable Agent-addressed mail to one active AgentTeam MemberRun through `AgentMessageRoute`; it does not collapse Agent identity, Team participation, or Company OS authority. |
| Dynamic Workflow | `workflow list/run/run-script/get-output/patch/gc-worktrees/reap-workers/reap` | Implemented | WorkflowRun/WorkflowStep remain their own execution truth. |
| Dashboard | `dashboard snapshot` | Implemented | Produces operator projection. |
| Serve/API | `serve [--addr] [--once]`, `mcp`, `node init/list/show/drain/retire/project`, `daemon start/status/stop/serve`, `hook record` | Implemented | One machine-scoped NodeDaemon supervises every admitted local TeamRun across registered Execution Spaces. All public start surfaces delegate to it and fail explicitly when unavailable; there is no per-run fallback. |
| Historical migration | `legacy-goal-task export`, `legacy-goal-task verify` | Retired compatibility | Export/verify only. Current planning must use Mission plus the Mission Log; Wave remains readable only as historical context. |
| Retired command families | old `goal`, `phase`, `task`, proposal/review/design surfaces | Retired compatibility | These fail explicitly and must not be used for new work. |

## Company OS CLI map

### Company Store

`firm company init/list/current/switch/show` manages the explicit Company
Store identity introduced by ADR 0042.

| Capability | Commands | Status | Notes |
| --- | --- | --- | --- |
| Registry | `company init --id <company-id> [--name <name>]`, `company list`, `company current`, `company show [company-id]`, `company switch <company-id>` | Implemented | Stores live under `<HARNESS_HOME>/companies/<id>/`; `ACTIVE_COMPANY` and `companies/registry.json` track the current Company. |
| Company OS routing | `harness --company <id> company ...`, `HARNESS_COMPANY=<id> firm company ...`, or active Company from `company switch/init` | Implemented | Applies only to `firm company ...`; Mission/Wave, Agent Team, Workflow, provider cwd, and Project selection remain separate. |
| Migration from project-derived stores | `company migrate-from-project --from-project <project-id\|path> --id <company-id> [--name <name>] [--force]`, `company migrate-from-project ... --verify-only`, `company migrations` | Implemented | Copies and verifies only the explicit active Company Store ledger allowlist, appends a Company Store migration record, and writes an advisory source marker. Retired WorkItem, Assignment, and cutover ledgers are disposable history: they are not copied and are not verification inputs. `--verify-only` applies the same allowlist to an existing destination. No Mission/Wave, Agent Team, Workflow, provider session, prompt, or runtime ledger is copied. |

### Docs

`firm company docs ...` is the most complete Company OS CLI surface. Per
the ADR 0054 retirement plan (`docs/current/company-os/ai-first-docs-spec.md` §13),
the Block-era document/template/block command tree was deleted at retirement
stage R3; the AI-first Docs v2 page surface is current for page/document work.

| Capability | Commands | Status | Notes |
| --- | --- | --- | --- |
| Pages v2 (ADR 0054) | `page create`, `page read`, `page write`, `page append`, `page search`, `page rename`, `page move`, `page archive` | Implemented | AI-first page model: whole-page immutable revisions with sha256 digests, `expected_revision` optimistic concurrency, idempotent replay by action id, scoped reads (`outline/section/range/keyword` + `simple/with-ids/full`), Markdown<->block serialization, `--after -1\|heading:<text>` anchors, write-time missing-embed warnings, metadata maintenance (`rename`, `move` with parent-cycle rejection, `archive` behind `--confirm` with dry-run default) through the same revision mechanism. Serve API: `/v1/company-os/docs-v2/pages*` (token-gated writes, revision history, live entity_embed resolution). Dashboard: `?surface=docs-v2` (store-live, zero fixture path). |
| Read/projection | `query`, `search`, `traverse`, `refs`, `related`, `snapshot`, `diff`, `change-report`, `health` | Implemented | Agent-readable and human-readable projections over native Docs records. |
| Source sync | `source sync` | Implemented | Syncs external source state into Docs TypedRecords and idempotent `Document → source_for → TypedRecord` Relations with explicit boundaries. GitHub webhook transport is still a separate future adapter. |
| Module setup | `module create` | Implemented | Creates BusinessModule plus default/fallback View through governed API path. |
| Custom page metadata | `page-definition create`, `page scaffold`, `page verify`, `page publish` | Partial | Defines/scaffolds/verifies/publishes custom-page records and refs. It does not yet generate a complete production page from an arbitrary business brief. |
| Typed records | `typed-record append`, `typed-record update`, `typed-record validate` | Implemented | Core structured memory primitive. |
| Views | `view create`, `view update` | Partial | Basic view records exist. Advanced view editing, calendar/chart views and complex field configuration are still missing. |
| Relations | `relation link`, `relation unlink`, `relation relink`, `relation repair-missing` | Implemented | Native relation maintenance across Docs and adjacent systems. `repair-missing` is definition/module-scoped, dry-run-first, confirmation-gated, and idempotent. |

### Work

`firm company work ...` is a read-only aggregate and Milestone surface. Native
Work mutations belong to `firm team-run work ...`.

| Capability | Commands | Status | Notes |
| --- | --- | --- | --- |
| Company Work read | `company work list`, `company work query` | Implemented | Filters native Work by Team, TeamRun, phase, condition, resolution, and owner; preserves exact ids and revisions. |
| Native Work intake | `team-run work create` | Implemented | Creates the only executable Work object inside an explicit TeamRun. |
| Native responsibility | `team-run work assign`, `claim`, `release`, `retarget` | Implemented | Mutates native Work with optimistic revision checks. |
| Native lifecycle | `team-run work start`, `block`, `resume`, `submit`, `review`, `request-changes`, `accept`, `cancel` | Implemented | Uses phase/condition/resolution and immutable report/gate/decision evidence. |
| Milestone management | `company work milestone list`, `show`, `create`, `update`, `close` | Implemented | Stores native Work ids in `work_refs`; Milestones never own Work lifecycle. |
| Retired Company task mutations | `company work create`, `update`, `assign`, `transition`, `close` | Removed | Rejected with an actionable route to `team-run work`. |
| Approval request/decision | `firm company approval request`, `decide`, `list`, `show` | Implemented | Approval is a shared Company OS CLI group, not nested under Work. Requests/decisions dispatch governed Actions. |

### Organization

| Capability | Current surface | Status | Notes |
| --- | --- | --- | --- |
| Actors | `org actor list`, `show`, `create-human`, `create-agent`, `update-status` | Implemented | Native Human and Standing Agent authoring is available. Writes use Human-admin administrative governance. |
| Org units | `org unit list`, `show`, `create`, `update-status` | Implemented | Native OrgUnit authoring is available. |
| Membership/reporting | `org membership list`, `assign`, `update-status` | Implemented | Native Membership assignment is available. Move/retire are represented as status updates for now. |
| Execution relation | `org link-execution --authority <human> --actor <standing-agent> --agent-member <id> --execution-space <id> [--replace]`, `org unlink-execution --authority <human> --actor <standing-agent> [--expect-agent-member <id>]` (also as `org actor link-execution`/`unlink-execution`) | Implemented | Links an EXISTING StandingAgent to an EXISTING AgentMember. Both ids are explicit; equal ids never bind implicitly. `--execution-space` is required and has no fallback because `firm company ...` resolves the Company Store and never reaches the `--space` selector (ADR 0042); the named space is opened read-only to validate the AgentMember. Only `execution_agent_member_ref` and `updated_at` change; the write is a governed administrative re-append, never a raw JSONL edit. Re-running the same pair is a no-op (`changed:false`), and repointing requires `--replace`. `--authority` is validated as an active Human `company_os.admin` on every invocation, including no-ops. |
| Permissions/capabilities | actor create/update-status fields | Partial | Create commands can write permission/capability refs. Dedicated grant/revoke/proposal workflow is still missing. |
| HR / business-agent lifecycle | `create-agent`, `membership assign`, `update-status` plus skill/docs contract | Partial | Basic lifecycle exists. Proposal/approval/promotion workflow remains next. |

### Finance

| Capability | Current surface | Status | Notes |
| --- | --- | --- | --- |
| Commitments | `finance commitment list`, `show`, `propose`, `transition` | Implemented | `propose` is R2; `transition` uses `commitment.append` and enforces existing approval policy. |
| Payments | `finance payment list`, `show`, `record` | Implemented | `record` creates evidence-backed prepared Payments through governed `payment.append`; settlement transition remains a future command. |
| Approval-linked spend | `company approval ...` + `company finance ...` | Implemented | Approval request/decision can be linked to Commitment and Payment commands. |
| Budget/invoice/refund/reporting | docs/skill contract only | Missing / next | Need first-class finance CLI and acceptance tests. |

### External gateways

| Capability | Current surface | Status | Notes |
| --- | --- | --- | --- |
| Social platform readiness | `company gateway social readiness [--platform xiaohongshu|douyin|wechat_channels] [--adb adb] [--device <serial>]` | Implemented / read-only core bootstrap | Observes Android package/focus readiness for Xiaohongshu, Douyin, and WeChat Channels slots. It returns data suitable for `social_platform_account` records but does not write Store truth, log in, publish, delete, pay, or export messages. |
| GitHub source observation | `company docs source sync` | Implemented for local worktree observation | Writes external software source TypedRecords. First GitHub connector should be sync/projection-first and may call existing `gh`/Git rather than adding GitHub-specific core CLI. GitHub webhook/API delivery remains next. |
| GitHub issue/PR/check connector | connector sync using existing `gh`/GitHub API/webhook; Company OS receives delivery refs and views | Planned / first connector priority | Sync issues, PRs, reviews, checks, branches, and source snapshots into TeamWork delivery panels, Agent detail development queues, and Docs source mapping views. No new MCP/plugin CLI is required for the first slice. |
| WeCom merchant gateway | docs/TeamWorks only | Planned | Needs schema/API/CLI/Agent inbox implementation. |
| Social plugin actions/connectors/views | plugin Skill + MCP or plugin-owned CLI adapter; Company OS receives governed Actions/TypedRecords/Relations/TeamWorks/evidence | Planned | Upload, title/body/topic fill, publish submit, comment/private-message sync, profile management, paid-promotion preparation, account sync, metrics sync, and view extensions should live in platform plugins rather than `firm-cli` core. |
| Social publication / metric evidence | Docs TypedRecords + TeamWorks now; plugin connector commands next | Partial | Current Store can model accounts, campaigns, post plans, publications, external message threads, and metric snapshots. Dedicated publish/message/metrics/plugin commands must remain policy-gated and write back through Company OS records. |

### Company OS API resources with no equivalent dedicated CLI

The local API can read/write more resources than the CLI exposes directly:

- `action-policies`
- `action-commands`
- `audit-events`

These should not all become raw append commands. The preferred next step is
ergonomic governed commands that preserve ownership boundaries:

- raw action policy / command / audit inspection helpers
- future budget / invoice / refund resources

## Skills and install status

Repository skills exist for the intended Company OS operator roles:

- `skills/company-docs-operator`
- `skills/company-work-operator`
- `skills/company-org-operator`
- `skills/parked-company-finance-operator-20260805` (retired per ADR 0053)
- `skills/company-module-designer`
- `skills/company-page-builder`
- `skills/company-business-project-bootstrap`

The skill suite is checked by `scripts/check-company-os-skill-suite.mjs` and
`acceptance:skill-install`. Skills are distribution/operator guidance. They are
not the authority for product architecture; canonical truth remains schemas,
store/API, CLI routing, UI, tests and ADRs.

## Current validation commands

| Check | What it proves |
| --- | --- |
| `pnpm check:company-os` | Docs CLI smoke, Work CLI smoke, skill suite, Company OS UI/runtime/navigation/trademark checks and docs governance. |
| `pnpm check:docs-v2-live` | Store-live Docs v2 API and page store contracts (revision writes, scoped reads, legacy projection). |
| `node scripts/check-company-os-work-cli-smoke.mjs` | Store-live Work CLI can create/assign/transition/close TeamWorks. |
| `node scripts/check-company-os-operator-cli-smoke.mjs` | Store-live Org, Milestone, Approval, Commitment and Payment operator CLI commands work together. |
| `pnpm acceptance:mission-wave` | Deterministic Mission/Wave, TeamRun, MCP and dashboard contracts. |
| `firm governance check` | Documentation registry/link/retired-surface governance. |

## Recommended CLI roadmap

1. **Org governance workflow**: add proposal/approval/promotion commands for
   permission grant/revoke, agent creation, reporting changes and retirement.
2. **Finance lifecycle depth**: add explicit payment transition/settlement,
   budget, invoice, refund and finance report commands.
3. **Work taxonomy helpers**: add WorkType/business-line helpers when they
   become first-class records instead of fields.
4. **Custom page CLI**: promote page metadata into a full page-build loop:
   design contract -> component scaffold -> store-live fixture -> screenshot
   capture -> compare -> publish.
5. **External source CLI**: add GitHub/webhook adapter commands that update
   Docs source projections without letting GitHub become commercial truth.
6. **SQL read-model CLI**: when ADR 0035 is implemented, add rebuild/status/query
   commands for the derived read/index layer. JSONL ledgers remain canonical.
