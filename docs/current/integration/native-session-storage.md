# Provider-native session adapter contract

```text
status: implemented_v1_and_extension_contract
owner_role: provider-integration
canonical_for: native session binding, reading, resume, availability, and Dashboard projection
decision: ADR 0032
```

## Purpose

An Agent Team member should keep using Codex, Kimi, Claude Code, or another
agent's own session storage. Harness must coordinate that member without
becoming a second transcript database.

This contract defines the adapter seam between:

- Harness coordination truth (`Mission`, append-only `MissionLogEntry`,
  `AgentTeamRun`, `MemberRun`, Work, messages, interactions, outcomes, and
  artifact/check refs); and
- provider-native execution truth (chat, tools, commands, file events, turns,
  native children, and resume data).

## Implemented V1 surface and extension seam

V1 implements mode-aware binding, availability probing, exact-owner bounded
on-demand reads, and explicit provider-native resume through provider-specific
Rust functions. History is exposed only in the authenticated AgentWorkspace
projection; the old run-addressed HTTP readers are retired. It does not expose
one public Rust trait with the exact name below.

The following pseudocode also shows the intended extension seam. The historical
read is deliberately a fresh bounded read with no durable or recoverable
cursor. A unified adapter-level interrupt method is not implemented as one
generic interface today; live interruption remains mode-specific under ADR
0031.

```rust
trait NativeSessionAdapter {
    fn bind(&self, launch: LaunchReceipt) -> NativeSessionRef;
    fn probe(&self, session: &NativeSessionRef) -> NativeSessionAvailability;
    fn read(&self, session: &NativeSessionRef, limit: usize)
        -> NativeActivityPage;
    fn resume(&self, session: &NativeSessionRef, input: ResumeInput)
        -> NativeResumeReceipt;
    fn interrupt(&self, session: &NativeSessionRef, turn: Option<&str>)
        -> NativeControlReceipt;
}
```

`read` returns a projection, not Harness persistence:

```text
NativeActivityPage
  source_provider
  native_session_id
  availability
  source_snapshot_fingerprint   # response-local; not a cursor
  records[]
    kind = user_message | assistant_message | tool | command | file |
           approval_request | provider_child | turn_status | error
    native_id
    native_parent_id?
    status
    title / sanitized_summary
    occurred_at?
    artifact_ref?
```

No record type includes private chain-of-thought. Provider-specific fields stay
behind a drill-in/debug boundary rather than expanding the generic schema.

## Binding contract

`NativeSessionRef` is stored on `MemberRun` or via a one-to-one binding:

| Field | Meaning |
| --- | --- |
| `provider` | Codex, Kimi, Claude, or adapter id |
| `execution_mode` | `codex_exec`, `codex_app_server`, `kimi_acp`, etc. |
| `native_session_id` | Provider-owned thread/session id |
| `native_locator_kind` | Adapter resolver strategy; not necessarily a public absolute path |
| `provider_version` | Version that created/last opened the session |
| `adapter_contract_version` | Reader/resume contract reviewed for that version |
| `availability` | `available | stale | missing | incompatible` |
| `supports_resume` | Verified for this mode and version, not inferred from brand |
| `last_verified_at` | Latest successful probe |
| `parent_native_session_id` | Optional resume/fork lineage |

Secrets, auth tokens, raw environment, and private absolute paths are not
returned to ordinary Dashboard clients.

## Write boundary

Provider adapters may write only the Harness facts created by crossing a
coordination boundary:

| Provider occurrence | Harness write |
| --- | --- |
| tool/command/file/chat/turn event | none; native projection only |
| provider asks a user question | correlated request `Message` |
| Lead answers | correlated reply `Message` + provider receipt |
| provider requests more permission | fail closed against the frozen AgentSession ceiling |
| operator steers/interrupts/resumes | control request + provider acknowledgement |
| member submits owned Work for review | `WorkSubmitted` with result/evidence refs |
| member explains or coordinates with another actor | Work-linked canonical `Message` |
| member/Host declares an outcome | explicit outcome summary + refs |
| file/check/result supports acceptance | artifact/check reference, optionally hash |
| Host judges, replans, recovers, or closes out | append-only Mission Log entry with outcome and refs |

The same text may exist in both systems only when a Human/Lead deliberately
promotes it into a coordination object. Automatic copying is prohibited.

## Dashboard read flow

```text
GET Harness Team/Member projection
  -> Mission/MissionLogEntry/TeamRun/MemberRun/Work/WorkDelivery/messages/interactions/outcome

GET authenticated AgentWorkspace for exact AgentIdentity
  -> provider adapter probe
  -> provider-native bounded read (latest 300 displayable items)
  -> SessionEventProjection grouped by provider turn/episode

UI merge
  -> one chronological presentation
  -> source and durability badges
  -> native unavailable state does not erase Harness records
```

The backend performs native reads so provider paths and credentials do not leak
to browser code. The current response exposes `truncated` rather than a cursor;
refresh/reconnect rebuilds the projection directly from provider storage.

## Execution-root boundary

`store_root` is only the centralized Harness coordination store. A provider's
cwd is independently resolved as member `provider_cwd_hint`, TeamRun
`execution_root`, then selected Workspace `project_root`. For new raw-store
compatibility rows the process cwd is snapshotted as `execution_root` at create
time. The provider-native session locator records what is needed to find
the provider session; it does not turn `store_root` into a working directory.

This distinction is observable behavior, not naming trivia. Codex discovers
project `AGENTS.md` plus project/root skills and configuration from its launch
cwd; Claude and Kimi discover their project instruction/configuration context
from the corresponding project/worktree execution root. Tests must keep the
central store outside the project and assert that the provider is spawned in
the project/worktree. Otherwise a multi-project Host can execute with the wrong
instructions while writing apparently valid coordination rows to the right
store.

Immediately before spawn, `MemberRun.provider_environment_observation` records actual cwd,
Git HEAD/branch when available, and discovered instruction/skill directory
paths. It never contains the files' contents, config values, credentials,
environment dumps, transcript/tool streams, or thinking.

## Resume flow

```text
Lead chooses Resume
  -> Harness validates role, permission, budget, workspace, mode profile
  -> adapter probes NativeSessionRef and version compatibility
  -> provider-native resume operation
  -> native session continues owning the transcript
  -> Harness records resume request/ack and attempt lineage
```

`fresh` and `resume` are explicit choices. A failed resume does not silently
start a fresh session. If the provider creates a new session while resuming, the
new binding records the parent native session id.

## Provider matrix

| Mode | Native identity today | Native read truth | Restart resume | Operational boundary |
| --- | --- | --- | --- | --- |
| Codex `codex_exec` | real thread id captured | Codex rollout/state DB is native truth | `codex exec resume` remains available to bounded Workflow and legacy non-Team paths | workflow-only for new work; historical Team records remain readable but cannot start a new member |
| Codex `codex_app_server` | real thread id captured | app-server thread APIs plus Codex native store | `thread/resume` wired through explicit member resume binding | live provider activity is transient; native history is read on demand |
| Kimi `kimi_acp` | real ACP session id captured | `~/.kimi-code/sessions/**/session_<id>/agents/main/wire.jsonl` | reviewed through 0.36.1 prefer `session/resume`; both resume and the `session/load` compatibility fallback may replay history, which is drained before the next prompt | K3/max selection, generation-crossing same-session resume, next-round mail, bounded full-access receipts, and cooperative `session/cancel` notification are current; live activity remains transient and native history is read on demand |
| Claude `claude_agent_sdk` | real `system(init).session_id` captured | `~/.claude/projects/**/<session>.jsonl` | streaming mailbox, SDK interrupt/close, explicit resume binding, and SDK `listSessions` | Only Claude Team mode; `system(init).claude_code_version` owns the version claim; Desktop visibility is opt-in through `claude://resume?session=<id>`, and Desktop stays observation-only while Harness drives |
| Claude `claude_cli` | historical/workflow session id | `~/.claude/projects/**/<session>.jsonl` | bounded `--resume` only | Dynamic Workflow and historical reads only; rejected for new Agent Team members |

Unknown providers and unregistered execution modes have no executable Team
Member adapter and fail explicitly. A provider brand, installed binary, native
history reader, or Host integration alone is not evidence that a Team Member
execution mode is supported.

“Provider supports” never means “adapter supports.” Each row needs deterministic
and live acceptance against reviewed provider versions.

For persistent Team modes, a service restart does not create a new execution
truth. The next durable Team Supervisor generation reattaches the recorded
native session before claiming queued mail. An uncertain claim is reconciled
against a native receipt rather than replayed, and a latched Host Close forbids
resume through ordinary Team delivery.

## Failure and lifecycle states

- `missing`: provider cleanup or machine move removed the native session;
- `stale`: last read succeeded but current probe did not complete;
- `incompatible`: provider/format version is outside the reviewed adapter set;
- `available`: read path works for the bound session;
- `resume unsupported`: history may be readable although the mode cannot resume.

Harness retains Work responsibility/state, outcome, refs, and gates in all
states. UI must not invent native activity or resume from a Harness replay.

### Implemented Agent Team surfaces

- `MemberRun.native_session` carries the mode-aware locator and verified
  capability snapshot. New provider activity is not written to
  `member_actions.jsonl` or `team_run_events.jsonl`.
- Authenticated `AgentWorkspace.data.session_event_projection` resolves the
  canonical AgentSession and current NodeDaemon generation server-side, then
  returns a bounded thinking-free projection. Host-selected Member views
  structurally omit this field. The legacy
  `GET /v1/member-runs/{id}/native-activity` route returns `410 Gone` because it
  cannot prove the exact owner.
- A retry can bind a member to an earlier provider session with HTTP/MCP member
  field `resume_native_session_id` or CLI
  `--resume-member <member-name>:<native-session-id>`. Resume is never inferred
  from the newest local session.
- Codex Agent Team app-server uses `thread/resume`; Kimi ACP uses
  `session/load`. Workflow-side `codex_exec` may use `codex exec resume`, but
  Harness never falls back to it for a Team member. A provider rejection fails
  the member honestly instead of falling back to a fresh session.

## Completed migration sequence

1. **Contract and binding (complete):** schema/Rust `NativeSessionRef`, capability snapshot,
   availability, migration checks.
2. **Codex native reader/resume (complete):** exec and app-server independently; stop new
   Codex provider-derived action/event writes.
3. **Kimi and Claude readers/resume (complete):** verify installed provider storage and
   privacy first; stop NDJSON/stderr mirror writes.
4. **Dashboard backend projection (complete for V1):** provider source,
   availability, bounded activity, and an honest truncation signal. The UI
   binding belongs to the frontend Task; explicit resume selection remains on
   TeamRun retry/create CLI, MCP, and HTTP inputs.
5. **Removal (complete):** delete obsolete provider-event ledgers, transcript/stdout/JSONL
   fields, reducers, and old local mirrored data; no compatibility reader.
6. **Acceptance (DEV-20 implementation evidence):** deterministic provider
   conformance and exact-owner HTTP integration prove native reads, privacy,
   explicit unavailable state, and zero duplicate provider history. A mixed
   real-provider UI journey remains separate live acceptance.

## Remaining projection extensions

- Historical observations carry their display-safe semantic payload and opaque
  provider-native event ids where the provider exposes them. Filesystem paths
  and raw transcript rows stay server-side.
- The read endpoint returns a bounded on-demand window with `truncated`.
  Pagination is intentionally deferred; it must not introduce a durable or
  recoverable Harness projection cursor.
- Dashboard shows native availability and whether resume is supported, but the
  operator-facing resume/fresh choice is not yet a Member Focus control.
- These are projection/control-plane extensions, not permission to restore a
  Harness transcript or provider-event mirror.

## Completion checklist for every provider mode

- Native session id comes from the provider, not a synthetic fallback.
- Reader can reopen a completed session after Harness restart.
- Tool/command/file/chat records shown in Dashboard resolve to native ids once
  the generic projection adds native item identity.
- Resume either continues the native session or fails explicitly.
- Adapter version drift covers native storage and resume format.
- Provider-native session loss produces an honest unavailable state.
- Harness ledgers contain no mirrored transcript/tool/command/file activity.
- Thinking is absent from persistence, caches, export, and evidence.
