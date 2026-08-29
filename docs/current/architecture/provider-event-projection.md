# Canonical Provider-Native Session Projection

Status: current after DEV-136.

## One transcript source

Provider-native persisted Session data is the only transcript authority. Codex,
Claude, Kimi, Pi, and DeepSeek each have one reviewed persisted reader that
projects a complete provider-owned row into
`agentfirm.provider_native_event_record.v3`. Harness never reconstructs a
transcript from runtime callbacks, RoleViews, RuntimeCommands, Messages, Work,
or coordination events, and never stores a copy of provider content.

```text
provider-owned Session source
  -> exact NodeDaemon persisted reader
  -> complete physical row + stable row locator
  -> ProviderNativeEventRecord v3
  -> ordered semantic fragments + expandable exact native_event
  -> snapshot / append / source-reset SSE
  -> Dashboard virtualized Session timeline
```

The record identity derives only from `source_generation` + `row_locator`.
NodeDaemon id/generation is absent from the row and exists only in the outer
read authority, so a daemon handoff cannot rename provider history. Ordering
key, content fingerprint, generation-scoped cursor, snapshot watermark, and
typed source reset are response-local read facts, never Harness Evidence.

## Semantic boundary

The persisted vocabulary contains Session metadata, reasoning, assistant
response, tool calls/results, command and file events, artifacts, usage, turn
terminal states, malformed rows, and unclassified native rows. Provider-native
user input remains present in the exact expandable native row whenever the
source carries it; it is not promoted to a semantic fragment until the reviewed
manifest explicitly claims that mapping. Unsupported semantic claims remain
absent while the full reviewed native row remains expandable.

Runtime, transport, interaction, command recovery, effect certainty, Work,
Message delivery, and Host acceptance are coordination facts and never appear
as transcript fragments. The Dashboard may place those projections beside the
Session chronology, but it must label and render them as a separate plane.

Text availability is independent from completeness. `unavailable` means the
provider-owned source did not expose the text and forbids a synthesized
placeholder. Complete content is not semantically filtered, summarized, or
truncated by Harness. Pagination, lazy loading, and virtualization bound UI
cost without changing the provider row.

## Exact read authority

Only the exact machine-scoped NodeDaemon resolves and opens a provider-native
source. A local caller uses its AF_UNIX control socket. A remote caller uses the
existing NodeGateway `native_session_read` application kind and
`collaboration.native_session_read` capability; the target applies the request
through the same NodeDaemon service. Browser, Dashboard server, Control Plane,
and gateway never receive an absolute source path and never open provider files.

Every read revalidates the exact Execution Space, Project Binding, Team,
TeamRun, AgentSession/runtime generation, native-session fingerprint, and
current NodeDaemon lease. A same-machine loopback Operator has explicit read
capability. Remote content is restricted to the exact Session-owning
AgentMember or exact active Team Host. Reading never grants mutation authority.

## Snapshot-first stream

The Dashboard subscribes before its first persisted read. The server emits:

- `native_session_snapshot`: recent complete rows and a watermark;
- `native_session_append`: complete rows strictly after that watermark;
- `native_session_source_reset`: typed replacement/rotation/format-fence reset,
  followed by a fresh snapshot.

Reconnect always starts from a fresh snapshot. Older pages carry the same
source generation. An incomplete physical tail is reported and never consumed.
No cursor, watermark, row, or fold is written to a Harness Store.

Provider callbacks are allowed only as no-payload wake hints that cause another
persisted read. They cannot contribute a display record. The former v2 live
decoder, in-memory activity registry, SessionEventProjection, TeamRuntimeActivity,
and `live_provider_activity` browser payload are retired current surfaces.

## Provider fidelity

`schemas/provider-events/persisted-adapters.v3.json` is the executable provider
capability disclosure. Each provider declares only source families, format
fences, pagination/tail behavior, reachability, and semantic fragments proven
by its persisted reader. Unsupported capabilities are absent rather than
inferred for parity. Real persisted fixtures and Rust/JSON Schema validation
must agree for all five providers.

The browser renders the same v3 records for first open, live append, reconnect,
older-page loading, and later reopen. A provider row cannot change semantic
meaning depending on which transport delivered the notification.

## Executable contracts

- `schemas/provider-events/provider-native-event-record-v3.schema.json`
- `schemas/provider-events/persisted-session-page-v1.schema.json`
- `schemas/provider-events/persisted-adapter-manifest-v1.schema.json`
- `schemas/provider-events/persisted-adapters.v3.json`
- `crates/firm-provider-events/src/persisted/`
- `crates/firm-cli/src/provider_event_persisted.rs`
- `apps/agent-dashboard/src/surfaces/AgentConversationWorkspace.tsx`
- `pnpm check:provider-events`
