# Provider-native session adapter contract

```text
status: implemented_persisted_v3_contract
owner_role: provider-integration
canonical_for: native session binding, reading, resume, availability, and Dashboard projection
decision: ADR 0032
```

## Purpose

An Agent Team member should keep using Codex, Kimi, Claude Code, or another
agent's own session storage. Harness must coordinate that member without
becoming a second transcript database.

This contract defines the adapter seam between:

- Harness coordination truth (`AgentTeamRun`, `MemberRun`, `AgentSession`,
  Work/WorkDelivery, identity-first Messages and per-recipient delivery,
  RuntimeCommands, outcomes, artifact/check refs, and retired read-only legacy
  evidence); and
- provider-native execution truth (chat, tools, commands, file events, turns,
  native children, and resume data).

## Implemented V2 surface and extension seam

V2 implements mode-aware binding, availability probing, local-Operator paged
on-demand reads, and explicit provider-native resume through provider-specific
Rust functions. History is exposed only in the loopback AgentWorkspace
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
    exact provider-native event             # present once, unchanged
    ordered semantic fragments[]            # zero-copy navigation/display model
      kind = session_metadata | reasoning | assistant_response | tool_call_* |
             artifact_created | usage_reported | runtime_* | turn_* | diagnostic
    native id, parent/correlation, ordering, provider/session/daemon fences
```

The same-machine local Operator receives the complete provider-native
event, including user, reasoning, response, tool, command/file, and raw error
fields. Provider-specific fields stay inside expandable `native_event`; only
semantic kinds listed by the provider's executable adapter manifest become
fragments. Harness does not reinterpret the remaining fields as coordination
truth.

One persisted-event projector builds both the snapshot/reconnect record and
the appended SSE record from provider-owned storage. Provider callback events
are wake hints only: they are never independently classified or rendered as a
second transcript. One native event may yield several ordered fragments—for
example Claude reasoning, assistant text, and tool use—without duplicating the
raw provider event.

## Binding contract

`NativeSessionRef` is stored in three places today:

- canonical `MemberRun.native_session`
  (`crates/firm-core/src/agentfirm_api/identity_session.rs:524-525`);
- `AgentSession.native_session_ref` (same file, `:301-302`);
- the legacy `member_runs.jsonl` `ProviderRuntimeProjection.native_session`
  (`crates/firm-core/src/team_runtime.rs:901-902`).

The canonical `MemberRun.native_session` is the expectation both fences compare
against, and they live in different layers:

- **Member admission**, in the Store under one writer lock, validates the
  canonical MemberRun against the legacy runtime projection and fails closed
  with `MEMBER_ADMISSION_NATIVE_IDENTITY_MISMATCH` when the identity fields
  disagree, or `MEMBER_ADMISSION_NATIVE_PROJECTION_MISMATCH` when the full
  observation differs. Its own code calls one side `canonical` and the other
  the `runtime projection` (`crates/firm-store/src/store_team_admission.rs:14-47`).
- **NodeDaemon adoption**, in `firm-cli` rather than the Store, validates
  `AgentSession.native_session_ref` against a ref derived from the MemberRun and
  refuses with `AGENT_SESSION_RECOVERY_REQUIRED: <session> does not match
  MemberRun <member> native-session truth`
  (`crates/firm-cli/src/main_modules/member_orchestration.rs:158-172`).

Neither fence silently picks a winner; both fail closed. Neither is a single
Store-level check over all three copies.

Fields:

| Field | Meaning |
| --- | --- |
| `provider` | Codex, Kimi, Claude, or adapter id |
| `execution_mode` | `codex_exec`, `codex_app_server`, `kimi_acp`, etc. |
| `native_session_id` | Provider-owned thread/session id |
| `native_locator_kind` | Adapter resolver strategy; not necessarily a public absolute path |
| `provider_version` | Version that created/last opened the session |
| `adapter_contract_version` | Reader/resume contract reviewed for that version |
| `availability` | `available | stale | missing | incompatible | unknown` — `unknown` is a real wire value, and the legacy `team_runtime` enum uses it as its serde default when the field is absent (`crates/firm-core/src/agentfirm_api/identity_session.rs:66-74`, `crates/firm-core/src/team_runtime.rs:452-470`) |
| `supports_resume` | Verified for this mode and version, not inferred from brand |
| `last_verified_at` | Latest successful probe |
| `parent_native_session_id` | Optional resume/fork lineage |

Harness does not add resolver credentials, bootstrap tokens, raw process
environment, or hidden filesystem locators to the response. It also does not
inspect or redact the selected provider-native event: if a coding provider
itself persisted a value inside that event, the local Operator sees
the original value exactly as the provider's own Session UI would.

## Write boundary

The provider-native session store is the sole truth for one agent's transcript,
tool/command/file events, turn lifecycle, and resume state. Harness references
that session and does not keep a second provider event history.

Provider adapters may write only the Harness facts created by crossing a
coordination boundary:

| Provider occurrence | Harness write |
| --- | --- |
| tool/command/file/chat/turn event | none; native projection only |
| provider asks a user question | correlated request `Message` |
| Lead answers | correlated reply `Message` + provider receipt |
| provider requests more permission | fail closed against the frozen AgentSession ceiling |
| operator interrupts/resumes | control request + provider acknowledgement |
| member submits owned Work for review | `WorkSubmitted` with result/evidence refs |
| member explains or coordinates with another actor | Work-linked canonical `Message` |
| member/Host declares an outcome | explicit outcome summary + refs |
| file/check/result supports acceptance | artifact/check reference, optionally hash |
| Host judges, accepts, or requests changes | explicit Work review/acceptance with outcome and refs |

The same text may exist in both systems only when a Human/Lead deliberately
promotes it into a coordination object. Automatic copying is prohibited.

## Dashboard read flow

```text
GET Harness Team/Member projection
  -> TeamRun/MemberRun/AgentSession/Work/WorkDelivery/Messages/RuntimeCommands/outcome

GET AgentWorkspace from the same-machine loopback local Operator
  -> browser asks the selected Team's exact NodeDaemon service
  -> NodeDaemon validates viewer + Team/Session/daemon generations
  -> provider-native snapshot page (default 80, maximum 200 events)
  -> PersistedSessionProjection with source generation + watermark

SSE subscribe
  -> subscribe before reading
  -> emit persisted snapshot + source generation + watermark
  -> emit only persisted rows after the watermark
  -> rotation/replacement emits source reset, then a fresh snapshot

UI merge
  -> one chronological presentation
  -> source and durability badges
  -> native unavailable state does not erase Harness records
```

The merged Team/Workspace presentation is a
joined read model, not a transcript database: it is rebuilt from Harness
coordination rows plus bounded provider-native reads, and is never persisted
as a second history.

Only the NodeDaemon performs native reads, so provider locators do not become
browser, Dashboard-server, Control-Plane, or remote-gateway authority. Local
reads use the daemon's AF_UNIX control service. Remote reads use the existing
NodeGateway routed application envelope and the same daemon service; neither
response exposes an absolute path. `next_before_position`, source generation,
and watermark are disposable request boundaries, not durable cursors:
refresh/reconnect rebuilds the projection directly from provider storage. The
browser lazily requests earlier pages and virtualizes the rendered list; it
never changes or truncates an original event.
Typed `availability` and `unavailable_reason_code` distinguish an available
empty Session from a missing, unsupported, or failed reader; prose remains
display detail, not state.

Claude persists the verified `system(init).session_id` binding as soon as the
provider emits it, before the bounded cycle reaches a terminal result. A later
idle timeout therefore cannot erase an already verified provider-native Session
from the canonical MemberRun projection.

## Execution-root boundary

`store_root` is only the centralized Harness coordination store. A provider's
cwd is independently resolved as current
`MemberWorkspaceBinding.canonical_root`, then TeamRun `execution_root`, then
`ProjectBinding.project_root`. The resolved canonical cwd is frozen for the
AgentSession; no legacy Store location or provider observation may supply a
fallback authority. The provider-native session locator records what is needed
to find the provider session; it does not turn `store_root` into a working
directory.

One provider does keep session files under the coordination store, and that is
deliberate rather than a counter-example: Pi's managed session directory is
`<store_root>/pi_sessions/<member_run_id>/`
(`crates/firm-cli/src/main_modules/pi_runner_state.rs:96-98`). That is where
Harness asks Pi to write its provider-owned JSONL; it is not Pi's cwd, which
still resolves through the binding chain above.

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

| Mode | `native_locator_kind` | Native identity today | Native read truth | Restart resume | Operational boundary |
| --- | --- | --- | --- | --- | --- |
| Codex `codex_exec` | `codex_rollout` | real thread id captured | Codex rollout/state DB is native truth | `codex exec resume` remains available to explicit legacy non-Team compatibility paths | historical/compatibility only for new work; historical Team records remain readable but cannot start a new member |
| Codex `codex_app_server` | `codex_rollout` | real thread id captured | app-server thread APIs plus Codex native store | `thread/resume` wired through explicit member resume binding | live provider activity is transient; native history is read on demand |
| Kimi `kimi_acp` | `kimi_code_session` | real ACP session id captured | `~/.kimi-code/sessions/**/<workDirKey>/<sessionId>/agents/main/wire.jsonl`; the older `session_<sessionId>` directory layout is still accepted (`crates/firm-cli/src/native_session.rs:571-580`) | reviewed through 0.39.0 prefer `session/resume`; both resume and the `session/load` compatibility fallback may replay history, which is drained before the next prompt | K3/max selection, generation-crossing same-session resume, next-round mail, bounded full-access receipts, cooperative `session/cancel`, and narrow `session/close` plus process reap are current; live activity remains transient and native history is read on demand |
| Claude `claude_agent_sdk` | `claude_project_session` | real `system(init).session_id` captured | `~/.claude/projects/**/<session>.jsonl` | streaming mailbox, SDK interrupt/close, and the SDK `resume` option carrying the exact session id (`apps/claude-member-runner/src/member-runner.mjs:157`); `listSessions` is not on the shipped resume path | Only Claude Team mode; `system(init).claude_code_version` owns the version claim; Desktop visibility is opt-in through `claude://resume?session=<id>`, and Desktop stays observation-only while Harness drives |
| Claude `claude_cli` | `claude_project_session` | real one-shot session id | `~/.claude/projects/**/<session>.jsonl` | exact-session resume remains only in the separately fenced external Host/direct-delivery compatibility path | rejected for managed Host and Member runs; historical Team records remain read-only |
| Pi `pi_rpc` | `pi_session` | exact provider JSONL locator captured from provider state | exact regular JSONL under `<store_root>/pi_sessions/<member_run_id>/`, i.e. beneath the managed Execution Space root (`crates/firm-cli/src/main_modules/pi_runner_state.rs:96-98`) | exact provider session path is retained for explicit continuation | absolute paths are accepted only from the canonical NativeSessionRef and must remain beneath the resolved managed root |
| DeepSeek Harness `deepseek_sdk` | `deepseek_harness_session` | exact native `SessionId` | official `@deepseek-ai/dsh-session-persistence-jsonl` reader over the reviewed zstd store, invoked as a bounded Node child process (3 s deadline, 16 MiB stdout cap, `crates/firm-cli/src/native_session/deepseek.rs:10`, `:53-62`) | `ctx.agents.resume` with the exact SessionId, reached through the runner's injected `runtime.resume` wrapper (`apps/deepseek-member-runner/bin/deepseek-member-runner.mjs:25`) | Harness never reimplements or copies DSH zstd/packed persistence; the official package returns a bounded response-local logical JSONL view |

The `native_locator_kind` column is what each Team adapter's
`native_locator_kind()` emits
(`crates/firm-provider-codex/src/team_runtime.rs:664`,
`crates/firm-provider-kimi/src/team_runtime.rs:230`,
`crates/firm-provider-claude/src/lib.rs:311`,
`crates/firm-provider-deepseek/src/lib.rs:992`,
`crates/firm-provider-pi/src/team_runtime.rs:185-187`). The two provider-keyed
helpers that build a ref before an adapter is bound — for an explicit
`resume_native_session_id`, and for a provider-native ref — do not cover every
provider. Neither has a `pi` arm;
`crates/firm-cli/src/main_modules/team_run_setup.rs:190-195` also has no
`deepseek_harness` arm, while
`crates/firm-cli/src/main_modules/provider_native_identity.rs:44-50` does. An
uncovered provider falls through to `provider_native` /
`provider_native_session` instead of the adapter's own kind.

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

These are read-availability observations on `NativeSessionRef`. They are not
AgentSession lifecycle values: `closed` is an `AgentSessionStatus`, written only
by a settled `StopSession` RuntimeCommand and terminal once written
(ADR 0071). A Team Close leaves the session `idle` and its native history
`available`.

Harness retains Work responsibility/state, outcome, refs, and gates in all
states. UI must not invent native activity or resume from a Harness replay.

### Implemented Agent Team surfaces

- `MemberRun.native_session` carries the mode-aware locator and verified
  capability snapshot. New provider activity is not written to
  `member_actions.jsonl` or `team_run_events.jsonl`.
- `AgentWorkspace.data.persisted_session_projection` resolves the canonical
  AgentSession and recorded NodeDaemon generation, then asks that daemon for
  complete provider-native events in response-local pages. The same-machine
  local Operator may select any locally bound Host or Member. A remote viewer
  must be the exact Session-owning AgentMember or the exact active Host and is
  carried through the current NodeGateway route; ordinary sibling credentials
  receive coordination data but no native Session content.
- The retired v2 Session projection and volatile provider payload overlay are
  not current API fields. Provider callbacks are no-payload wake hints; the
  Dashboard renders only persisted v3 snapshot/append/source-reset records.
  The legacy
  `GET /v1/member-runs/{id}/native-activity` route returns `410 Gone` because it
  cannot prove the canonical Team and AgentSession scope.
- Stop/Detach or daemon release does not erase readable provider-native
  history. Current NodeDaemon/Supervisor authority remains mandatory for every
  Resume, Interrupt, Close, delivery, and other provider effect. Team Close
  quiesces the AgentSession to `idle`; a provider `StopSession` closes it, and
  the next adoption pass then mints a new AgentSession row carrying the same
  native session id (ADR 0071).
- A retry can bind a member to an earlier provider session with HTTP/CLI member
  field `resume_native_session_id` or CLI
  `--resume-member <member-name>:<native-session-id>`. Resume is never inferred
  from the newest local session.
- Codex Agent Team app-server uses `thread/resume`; Kimi ACP prefers
  `session/resume` and uses `session/load` only as the reviewed
  method-not-found compatibility path. An explicit legacy non-Team path may use
  `codex exec resume`, but
  Harness never falls back to it for a Team member. A provider rejection fails
  the member honestly instead of falling back to a fresh session.

## Completed migration sequence

1. **Contract and binding (complete):** schema/Rust `NativeSessionRef`, capability snapshot,
   availability, migration checks.
2. **Codex native reader/resume (complete):** exec and app-server independently; stop new
   Codex provider-derived action/event writes.
3. **Kimi and Claude readers/resume (complete):** verify installed provider storage and
   privacy first; stop NDJSON/stderr mirror writes.
4. **NodeDaemon persisted projection (complete):** local and routed remote read,
   bounded older pages, snapshot-first SSE, watermark append, and typed source
   reset. Dashboard consumption and compatibility-overlay deletion belong to
   the frontend cutover Task; explicit resume selection remains on TeamRun
   retry/create CLI and HTTP inputs.
5. **Removal (complete):** delete obsolete provider-event ledgers, transcript/stdout/JSONL
   fields, reducers, and old local mirrored data; no compatibility reader.
6. **Acceptance (DEV-20 implementation evidence):** deterministic provider
   conformance and local-Operator HTTP integration prove native reads, scope,
   explicit unavailable state, and zero duplicate provider history. A mixed
   real-provider UI journey remains separate live acceptance.

## Remaining projection extensions

- Provider-native formats remain provider-specific inside `native_event`; a
  future richer renderer may add provider-aware presentation without filtering
  or copying the source.
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
- Thinking is absent from Harness persistence, caches, export, and evidence;
  provider-persisted thinking remains readable from the native Session.
