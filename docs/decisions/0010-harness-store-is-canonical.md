# 0010: Harness Store Is Canonical

## Status

Accepted for coordination state. Amended by
[ADR 0032](0032-provider-native-session-is-execution-truth.md): the provider's
native session, rather than a Harness mirror, is canonical for one agent's
transcript, tool activity, and resume state.

## Decision

The canonical coordination state is the Harness store plus versioned repo
artifacts. Provider-native session state is referenced, not copied.

## Consequences

Provider transcripts, hooks, PRs, dashboards, and logs cannot establish
assignment, authority, approval, or Wave acceptance. Acceptance uses explicit
Harness outcomes, artifact/check references, and gates while execution claims
remain verifiable in the provider-native session. Provider events do not need
to be duplicated into Harness before they can support such a claim.
