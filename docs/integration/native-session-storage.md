# Provider-native session adapter contract

```text
status: target_contract
owner_role: provider-integration
canonical_for: native session binding, reading, resume, availability, and Dashboard projection
decision: ADR 0032
```

## Purpose

An Agent Team member should keep using Codex, Kimi, Claude Code, or another
agent's own session storage. Harness must coordinate that member without
becoming a second transcript database.

This contract defines the adapter seam between:

- Harness coordination truth (`Mission`, `Wave`, `AgentTeamRun`, `MemberRun`,
  assignments, interactions, outcomes, artifact/check refs, gates); and
- provider-native execution truth (chat, tools, commands, file events, turns,
  native children, and resume data).

## Target interfaces

Names below are architectural contracts, not claims that the Rust traits or
schemas already exist.

```rust
trait NativeSessionAdapter {
    fn bind(&self, launch: LaunchReceipt) -> NativeSessionRef;
    fn probe(&self, session: &NativeSessionRef) -> NativeSessionAvailability;
    fn read(&self, session: &NativeSessionRef, cursor: Option<NativeCursor>)
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
  cursor / next_cursor
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
| provider asks user/permission/plan question | `PendingInteraction` |
| Lead/Human/Policy answers | interaction resolution + control acknowledgement |
| operator steers/interrupts/resumes | control request + provider acknowledgement |
| member explicitly hands work to another actor | `TeamMessage(kind=handoff)` |
| member/Host declares an outcome | explicit outcome summary + refs |
| file/check/result supports acceptance | artifact/check reference, optionally hash |
| Wave is judged | Wave gate |

The same text may exist in both systems only when a Human/Lead deliberately
promotes it into a coordination object. Automatic copying is prohibited.

## Dashboard read flow

```text
GET Harness Team/Member projection
  -> Mission/Wave/TeamRun/MemberRun/assignment/interactions/outcome/gate

GET native activity for NativeSessionRef
  -> provider adapter probe
  -> provider-native read(cursor)
  -> sanitized NativeActivityProjection

UI merge
  -> one chronological presentation
  -> source and durability badges
  -> native unavailable state does not erase Harness records
```

The backend performs native reads so provider paths and credentials do not leak
to browser code. Cursor caches are bounded, deletable, and never acceptance
evidence. Refresh/reconnect can rebuild them.

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

| Mode | Native identity today | Native read truth | Restart resume | Current migration gap |
| --- | --- | --- | --- | --- |
| Codex `codex_exec` | real thread id captured | Codex rollout/state DB is native truth | `codex exec resume` wired through explicit member resume binding | live provider activity is transient; native history is read on demand |
| Codex `codex_app_server` | real thread id captured | app-server thread APIs plus Codex native store | `thread/resume` wired through explicit member resume binding | live provider activity is transient; native history is read on demand |
| Kimi `kimi_acp` | real ACP session id captured | `~/.kimi-code/sessions/**/session_<id>/agents/main/wire.jsonl` | ACP 0.27.0 advertises `loadSession` and `sessionCapabilities.resume`; `session/load` is wired | live provider activity is transient; native history is read on demand |
| Claude `claude_cli` | real `system(init).session_id` captured | `~/.claude/projects/**/<session>.jsonl` | `--resume` wired through explicit member resume binding | Agent Team activity is native-read; legacy Standing Agent delivery mirrors remain queued for retirement |

“Provider supports” never means “adapter supports.” Each row needs deterministic
and live acceptance against reviewed provider versions.

## Failure and lifecycle states

- `missing`: provider cleanup or machine move removed the native session;
- `stale`: last read succeeded but current probe did not complete;
- `incompatible`: provider/format version is outside the reviewed adapter set;
- `available`: read path works for the bound session;
- `resume unsupported`: history may be readable although the mode cannot resume.

Harness retains assignment, responsibility, outcome, refs, and gates in all
states. UI must not invent native activity or resume from a Harness replay.

### Implemented Agent Team surfaces

- `MemberRun.native_session` carries the mode-aware locator and verified
  capability snapshot. New provider activity is not written to
  `member_actions.jsonl` or `team_run_events.jsonl`.
- `GET /v1/member-runs/{id}/native-activity` resolves the provider-owned file
  server-side and returns a bounded, thinking-free display projection. Native
  paths never leave the backend and the response is never cached into a
  Harness ledger.
- A retry can bind a member to an earlier provider session with HTTP/MCP member
  field `resume_native_session_id` or CLI
  `--resume-member <member-name>:<native-session-id>`. Resume is never inferred
  from the newest local session.
- Codex `codex_exec` uses `codex exec resume`; Codex app-server uses
  `thread/resume`; Kimi ACP uses `session/load`. A provider rejection fails the
  member honestly instead of falling back to a fresh session.

## Implementation Waves

1. **Contract and binding:** schema/Rust `NativeSessionRef`, capability snapshot,
   availability, migration checks.
2. **Codex native reader/resume:** exec and app-server independently; stop new
   Codex provider-derived action/event writes.
3. **Kimi and Claude readers/resume:** verify installed provider storage and
   privacy first; stop NDJSON/stderr mirror writes.
4. **Dashboard joined projection:** source labels, missing/stale/incompatible
   states, cursor pagination, resume/fresh controls.
5. **Removal:** delete obsolete provider-event ledgers, transcript/stdout/JSONL
   fields, reducers, and old local mirrored data; no compatibility reader.
6. **Acceptance:** mixed-provider TeamRun proves assignments, interactions,
   outcomes and gate in Harness, native activity/resume at providers, and zero
   duplicate provider history.

## Acceptance checklist for every provider mode

- Native session id comes from the provider, not a synthetic fallback.
- Reader can reopen a completed session after Harness restart.
- Tool/command/file/chat records shown in Dashboard resolve to native ids.
- Resume either continues the native session or fails explicitly.
- Adapter version drift covers native storage and resume format.
- Provider-native session loss produces an honest unavailable state.
- Harness ledgers contain no mirrored transcript/tool/command/file activity.
- Thinking is absent from persistence, caches, export, and evidence.
