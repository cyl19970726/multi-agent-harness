# Canonical Provider Event Projection

Status: current contract for DEV-20.

## Authority boundary

Provider transcripts remain provider-owned. AgentFirm incrementally reads a
server-selected source and converts each supported native row into a bounded
`ProviderObservation` during an on-demand read. The observation is a disposable
read-model value; it is never Message, Work, Delivery, Evidence, review, or
Decision truth.

```text
provider source
  -> bounded provider decoder
  -> ProviderObservation (private/public/operator visibility fixed by server)
  -> disposable generation-fenced in-memory fold
       -> exact-owner SessionEventProjection
       -> allowlisted TeamRuntimeActivity
       -> RuntimeCommand settle/recovery evidence
```

The source path is never returned. The envelope exposes only an opaque scoped
`native_source_ref` and a content fingerprint. The reader rejects symlinks,
root escape, cursor rollback, oversized lines, and invalid UTF-8. An incomplete
last line remains unconsumed until the provider finishes it.

## Identity and source authority

Every observation binds exact AgentIdentity, AgentSession id/generation, and
NodeDaemon id/generation from server context. Provider JSON cannot select those
fields, visibility, validated references, or RuntimeCommand authority.

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

## Privacy projections

`SessionEventProjection` is available only to the exact AgentIdentity owner of
the current AgentSession generation. Team Host status does not bypass this
boundary. A Team page consumes `TeamRuntimeActivity`, a separately constructed
allowlist containing only interaction, runtime availability/interruption, and
recovery summaries.

Authored text, reasoning, tool input/output, environment details, paths, and
raw transcript rows are structurally absent from Team activity. Canonical
Message, Delivery, Work, report, finding, failure, gate, and review facts remain
owned by their existing stores and are composed alongside runtime summaries by
the Team RoleView; provider observations never manufacture them.

## Provider fidelity

The versioned adapter manifest is executable governance. Codex, Claude, Kimi,
and Pi each declare native families, native identity support, and the exact
semantic kinds implemented by their decoder. Unsupported capabilities remain
absent rather than being inferred for parity. Fixture conformance covers all
four providers; only an actually available provider may be claimed as a live
journey.

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
- `pnpm check:provider-events`: Rust/TypeScript/schema/manifest parity and
  conformance gate.
