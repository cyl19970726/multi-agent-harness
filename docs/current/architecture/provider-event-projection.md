# Canonical Provider Event Projection

Status: current contract through DEV-88.

## Authority boundary

Provider transcripts remain provider-owned. AgentFirm performs a paged read
of a server-selected source and converts every complete native row into a
`ProviderObservation` during an on-demand request. The observation is a
disposable read-model value; it is never Message, Work,
CanonicalMessageDelivery, Evidence,
review, or Decision truth.

```text
provider source
  -> page scan in provider order (no content filtering or event truncation)
  -> ProviderObservation + exact response-local native_event
  -> disposable generation-fenced in-memory fold
       -> local-Operator SessionEventProjection
       -> allowlisted TeamRuntimeActivity
       -> validated recovery correlation (no command or evidence writes)
```

The source path is never returned. The envelope exposes only an opaque scoped
`provider-source:` locator in `native_source_ref`—not a Harness Evidence
reference—paired with a response-local content fingerprint that is also not
Evidence. The reader rejects
symlinks, root escape, invalid transient read positions, and invalid UTF-8. One
event is never shortened to meet a page budget. Incremental process-local reads leave an incomplete last line
unconsumed; the on-demand latest projection omits it and reports truncation.

## Identity and source authority

Every observation binds the exact AgentMember (recorded under its same-ID deprecated AgentIdentity compatibility naming), AgentSession id/generation, and
NodeDaemon id/generation from server context. Provider JSON cannot select those
fields, visibility, collaboration/Evidence references, or RuntimeCommand
authority. The V1 observation schema carries no collaboration-reference field.

Duplicate native rows within one on-demand read are idempotent. Any changed
envelope under the same observation identity conflicts, even when the native
content fingerprint is unchanged. Late rows are folded by provider ordering
position. The response exposes a `source_snapshot_fingerprint` only to describe
that response; it is not a cursor, replay token, stable history ID, or evidence
reference.

AgentFirm writes no transcript mirror, fold, observation history, or transcript
position. A process restart discards every in-memory fold and reads the
provider-owned source again. The provider-native Session remains the sole
history and correctness authority.

The historical projection is returned only inside the authenticated
`AgentWorkspace.data.session_event_projection`. The deprecated run-addressed
`/native-activity` routes are retired because they cannot prove the canonical
Team and AgentSession scope. Opening or resuming a provider UI/Session is a separate explicit
control action and is not implied by reading this projection.

## Trusted local read boundary

`SessionEventProjection` is available to the same-machine loopback Dashboard
Operator for every locally bound AgentSession. A local read does not require a
per-Agent secret or Provider credential: the loopback connection plus the
selected canonical Execution Space/Project/Team/AgentSession binding form the
read boundary. Remote RoleView credentials remain coordination credentials;
they do not grant provider-native Session content. Remote transcript browsing
is not part of this contract.

This grants no mutation capability. Work, Message, Accept, Close, Reopen, and
every provider effect still travel through authenticated `firm` CLI/Supervisor
authority and exact generation fences. Cross-machine transport continues to
use NodeDaemon/gateway machine identity.

`LiveProviderActivity` is a volatile channel: provider sinks send the complete
provider event plus typed navigation metadata into the serve process; its registry
holds at most 24 items per exact Execution Space + Project Binding +
AgentSession + MemberRun generation for 10 seconds. The SSE event name is
`live_provider_activity`, and the envelope is
`agentfirm.live_provider_activity_event.v1`. Delivery uses the same local-
Operator read policy as history. The browser subscribes to one
selected Team and AgentMember; SSE fanout remains scoped to that exact
Execution Space, Project Binding, and AgentMember. Terminal turn state clears the overlay
immediately; disconnect, TTL expiry, or process restart loses it.

Provider transports are not required to be SSE themselves. Codex app-server,
Claude SDK, Kimi ACP, Pi RPC, and DeepSeek Cordis events are normalized by
their runtime adapters; Dashboard delivery uses Harness SSE. NodeDaemon and
`firm serve` are separate processes. When the local Operator opens a stream,
serve registers a machine-local callback for the selected AgentMember. The
Unix control socket, exact current NodeDaemon instance, loopback callback
authority, serve-instance capability, and callback token fence registration;
the HTTP reader was locally admitted before registration. A stale daemon instance or
non-loopback callback cannot install an endpoint. Provider children do not
inherit callback capabilities. Neither callback capabilities nor live items
are durable.

The live scope carries `member_run_generation` and
`agent_session_generation` as independent fences. Reopen advances the Team
adapter's MemberRun generation while it may continue the same canonical
AgentSession generation and exact provider-native Session. An update or
terminal event from the pre-Reopen adapter is rejected at ingress and cannot
populate or clear the reopened generation's overlay. Historical reads continue
to resolve the exact native-session binding; they do not require those two
independent generations to be numerically equal or the recorded NodeDaemon
lease to remain active. A current lease fences execution effects, not read
authority. Local-Operator read context, Execution Space, Project Binding, Team,
AgentSession/native-session binding, recorded provenance, and provider source
containment remain mandatory.

The local Operator receives the provider event as it exists at the
native boundary: user input, thinking/reasoning, assistant response, tool
call/result, command/file event, and raw provider error are not semantically
filtered or summarized. This content remains response-local/volatile and is
never written into a Harness Store. Page size, lazy loading, and UI
virtualization bound resource use without changing or truncating the original
event. Live SSE and reopened provider history use the same raw-event display
model.

Canonical
Message, CanonicalMessageDelivery, Work, report, finding, failure, gate, review, and
RuntimeCommand summaries remain owned by their existing stores and are
composed by the Team RoleView; provider observations never manufacture them.
Provider turn completion may append only a generic coordination fact such as
"round completed with authored output; transcript remains provider-native".
It never copies the provider-authored response into `MemberAction` or a
TeamRun summary, and provider-emitted source references never become Harness
Evidence without an explicit canonical Evidence write.
The same rule applies to legacy Message delivery adapters: their response text
may be consumed by the current in-process caller, but it does not manufacture a
`provider-report` Message or a delivery-session Evidence row. Provider hooks
are validated-and-discarded compatibility ingress; NodeDaemon/AgentSession and
canonical Message/CanonicalMessageDelivery writers remain the only message and
delivery authority.

## Provider fidelity

The versioned adapter manifest is executable governance. Codex, Claude, Kimi,
Pi, and DeepSeek Harness each declare native families, native identity support, and the exact
semantic kinds implemented by their decoder. Unsupported capabilities remain
absent rather than being inferred for parity. Fixture conformance covers all
five providers; only an actually available provider may be claimed as a live
journey.

`terminal` clear is provider-neutral and emitted for every bounded cycle.
Native event families remain provider-specific and are displayed honestly;
typed navigation hints are emitted only when the reviewed transport exposes
them:

| Provider | thinking | response streaming | tool started | tool completed/failed | interaction waiting |
| --- | --- | --- | --- | --- | --- |
| Codex app-server | yes | yes | yes | completed | no |
| Claude Agent SDK | when emitted | yes | yes | provider-native | no |
| Kimi ACP | yes | yes | yes | completed + failed | permission request |
| Pi RPC | when emitted | provider-native | yes | completed + failed | provider-native |
| DeepSeek Harness | yes | yes | yes | completed | provider-native |

This matrix is a capability disclosure, not a request to synthesize missing
events. Historical fidelity is separately governed by
`schemas/provider-events/adapters.v1.json` and the provider-native readers.

## Recovery

Command-caused effect certainty requires an exact RuntimeCommand binding.
Unknown native effect becomes `command_recovery_required` with `unknown`
certainty and `recovery_required` completeness. Malformed complete rows remain
visible as exact raw provider input with an operator diagnostic classification.
Missing or unprovable fields are never
treated as successful product facts.

## Executable contracts

- `schemas/provider-events/`: closed JSON Schemas, manifests, and fixtures.
- `crates/firm-provider-events/`: decoders, disposable fold, access policy, and
  paged on-demand reader. It has no persistent store.
- Agent Workspace consumers bind directly to the versioned JSON Schemas. The
  browser displays typed navigation metadata and the exact `native_event`; its
  TypeScript ownership remains in the frontend Task.
- `schemas/provider-events/session-event-projection.schema.json` is the
  historical response contract. `live-provider-activity.schema.json` and
  `live-provider-activity-event.schema.json` are the volatile snapshot/SSE
  contracts.
- `pnpm check:provider-events`: Rust/TypeScript/schema/manifest parity and
  conformance gate.
