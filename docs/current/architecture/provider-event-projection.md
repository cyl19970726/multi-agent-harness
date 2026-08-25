# Canonical Provider Event Projection

Status: current contract through DEV-88.

## Authority boundary

Provider transcripts remain provider-owned. AgentFirm performs a bounded read
of a server-selected source and converts each supported native row into a
`ProviderObservation` during an on-demand request. The observation is a
disposable read-model value; it is never Message, Work,
CanonicalMessageDelivery, Evidence,
review, or Decision truth.

```text
provider source
  -> bounded provider decoder
  -> ProviderObservation (private/public/operator visibility fixed by server)
  -> disposable generation-fenced in-memory fold
       -> exact-owner SessionEventProjection
       -> allowlisted TeamRuntimeActivity
       -> validated recovery correlation (no command or evidence writes)
```

The source path is never returned. The envelope exposes only an opaque scoped
`provider-source:` locator in `native_source_ref`—not a Harness Evidence
reference—paired with a response-local content fingerprint that is also not
Evidence. The reader rejects
symlinks, root escape, invalid transient read positions, oversized lines, and
invalid UTF-8. Incremental process-local reads leave an incomplete last line
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
`/native-activity` routes are retired because they cannot prove the exact
Session owner. Opening or resuming a provider UI/Session is a separate explicit
control action and is not implied by reading this projection.

## Privacy projections

`SessionEventProjection` is available only to the exact owner AgentMember of
the current AgentSession generation. Team Host status does not bypass this
boundary. `TeamRuntimeActivity` is the only provider-derived shape eligible for
a future Team projection: it is a separately constructed allowlist containing
only interaction, runtime availability/interruption, and recovery summaries.
DEV-20 does not compose that shape into a selected Member's public RoleView.
Current Team pages continue to use canonical responsibility, Message, Work,
CanonicalMessageDelivery, and allowlisted runtime-command summaries.

`LiveProviderActivity` is a different channel: provider sinks send typed,
display-safe phase/tool/response progress into the serve process; its registry
holds at most 24 items per exact Execution Space + Project Binding +
AgentSession + MemberRun generation for 10 seconds. The SSE event name is
`live_provider_activity`, and the envelope is
`agentfirm.live_provider_activity_event.v1`. Delivery requires an authenticated
SSE subscription whose actor is the exact owner AgentMember. Same-project
Hosts, siblings, anonymous streams, cross-project streams, and later reconnects
receive no Member-private overlay. Terminal turn state clears the overlay
immediately; disconnect, TTL expiry, or process restart loses it.

Provider transports are not required to be SSE themselves. Codex app-server,
Claude SDK, Kimi ACP, Pi RPC, and DeepSeek Cordis events are normalized by
their runtime adapters; only the owner-authenticated Dashboard delivery uses
Harness SSE. NodeDaemon and `firm serve` are separate processes. When an exact
AgentMember owner opens an authenticated private SSE stream, serve registers a
process-memory callback for that owner only. Registration proves the existing
browser capability and current NodeDaemon instance; a forged actor, stale
daemon instance, anonymous process, or another Member cannot install or
replace that owner's endpoint. A later valid serve instance replaces only the
same owner's endpoint. Provider children explicitly do not inherit the HTTP
credential registry. Neither callback capabilities nor live items are durable.

The live scope carries `member_run_generation` and
`agent_session_generation` as independent fences. Reopen advances the Team
adapter's MemberRun generation while it may continue the same canonical
AgentSession generation and exact provider-native Session. An update or
terminal event from the pre-Reopen adapter is rejected at ingress and cannot
populate or clear the reopened generation's overlay. Historical reads continue
to resolve the exact native-session binding; they do not require those two
independent generations to be numerically equal or the recorded NodeDaemon
lease to remain active. A current lease fences execution effects, not read
authority. Exact owner, Execution Space, Project Binding, Team,
AgentSession/native-session binding, recorded provenance, and provider source
containment remain mandatory.

Codex live reasoning accepts only provider-declared `summaryTextDelta`. Kimi
thought text is discarded and becomes only a generic thinking phase; an ACP
reverse permission request becomes only a generic interaction-waiting phase.
Pi runs with provider thinking disabled. DeepSeek reasoning blocks and native
tool names remain private; only generic live phases and closed generic tool
lifecycle are projected. Claude Agent SDK thinking blocks are discarded
because the SDK does not label them as display-safe summaries. Unknown provider
tool names, arguments, paths, and status strings are also omitted. Raw
chain-of-thought is never saved, reconstructed, or forwarded.

Authored text, reasoning, tool input/output, environment details, paths, and
raw transcript rows are structurally absent from Team activity. Canonical
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

The volatile live channel is intentionally not equal-fidelity. `terminal`
clear is provider-neutral and emitted for every bounded cycle; phase rows are
only emitted when the reviewed native transport exposes them:

| Provider | thinking | response streaming | tool started | tool completed/failed | interaction waiting |
| --- | --- | --- | --- | --- | --- |
| Codex app-server | provider-declared summary only | yes | yes | completed | no |
| Claude Agent SDK | no (thinking dropped) | yes | yes | no | no |
| Kimi ACP | generic phase only | yes | yes | completed + failed | permission request |
| Pi RPC | no (thinking disabled) | no | yes | completed + failed | no reviewed live mapping |
| DeepSeek Harness | generic phase only | yes | yes | completed | no current runner emission |

This matrix is a capability disclosure, not a request to synthesize missing
events. Historical fidelity is separately governed by
`schemas/provider-events/adapters.v1.json` and the provider-native readers.

## Recovery

Command-caused effect certainty requires an exact RuntimeCommand binding.
Unknown native effect becomes `command_recovery_required` with `unknown`
certainty and `recovery_required` completeness. Malformed native rows produce
a redacted operator-only diagnostic. Missing or unprovable fields are never
treated as successful product facts.

## Executable contracts

- `schemas/provider-events/`: closed JSON Schemas, manifests, and fixtures.
- `crates/firm-provider-events/`: decoders, disposable fold, access policy, and
  bounded on-demand reader. It has no persistent store.
- Agent Workspace consumers bind directly to the versioned JSON Schemas. The
  browser consumes typed projections and never reinterprets native JSON; its
  TypeScript ownership remains in the frontend Task.
- `schemas/provider-events/session-event-projection.schema.json` is the
  historical response contract. `live-provider-activity.schema.json` and
  `live-provider-activity-event.schema.json` are the volatile snapshot/SSE
  contracts.
- `pnpm check:provider-events`: Rust/TypeScript/schema/manifest parity and
  conformance gate.
