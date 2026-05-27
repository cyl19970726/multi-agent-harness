# 0010: Harness Store Is Canonical

## Decision

The canonical coordination state is the harness store plus versioned repo
artifacts.

## Consequences

Provider transcripts, hooks, PRs, dashboards, and logs are evidence sources.
Provider state must be reduced into harness messages, events, evidence,
proposals, or decisions before it is used for acceptance.
