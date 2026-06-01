# Research

External-system architecture studies that inform our runtime and persistence
decisions. Each study is a faithful, source-cited writeup of how another
multi-agent system actually executes, persists, and coordinates agents. The
point is not to copy: it is to make our own substrate choices
([0018](../decisions/0018-exec-stream-primary-substrate.md),
[agent-integration-model.md](../agent-integration-model.md)) deliberate rather
than accidental, by seeing the design space others have explored.

## What is here

| Study | Topic | Doc |
| --- | --- | --- |
| Claude Code agent teams | Task-type taxonomy, in-process async substrate, file mailbox, session-resume model | [claude-code-agent-teams.md](claude-code-agent-teams.md) |
| Claude Code teams — diagrams | ASCII component / lifecycle / message-sequence / concurrency / persistence companion | [claude-code-agent-teams-diagrams.md](claude-code-agent-teams-diagrams.md) |
| Multica | DB-backed server + edge daemon, per-task subprocess + session-resume, squad routing | [multica-architecture.md](multica-architecture.md) |
| Multica — diagrams | ASCII deployment / lifecycle / end-to-end data-flow / slot / resume companion | [multica-architecture-diagrams.md](multica-architecture-diagrams.md) |
| Runtime/persistence decision | 3-way comparison (Claude Code / Multica / our harness) and the recommended model for us | [runtime-persistence-decision.md](runtime-persistence-decision.md) |
| Dynamic Workflow runtime design | Rust-native runtime to orchestrate codex+claude agents: the Rust-expression decision, CC-primitive mapping, Workflow object/run model, multi-provider scenario, WP plan | [dynamic-workflow-runtime-design.md](dynamic-workflow-runtime-design.md) |

The first two are descriptive (what an external system does). The third is
prescriptive (what we should do) and is the deliverable that ties the studies
back to [0018](../decisions/0018-exec-stream-primary-substrate.md). The fourth is
prescriptive too: a design for a Rust-native Dynamic Workflow runtime, modeled on
Claude Code Workflows, whose conceptual basis is the owner Dynamic Workflow
research report (cached at the gitignored path
`/.research-cache/dynamic-workflows/report.md`, never committed).

## How to add a study

1. Cache the source repo under `.research-cache/<name>/` and read it
   **read-only**. `.research-cache/` is gitignored and is **never committed** —
   it is a local convenience, not a deliverable. Scratch notes
   (`.harness-*.md`) are likewise local-only.
2. Write `docs/research/<system>-architecture.md` (or `<system>-<area>.md`).
   Lead with a one-paragraph answer to the question that motivated the study,
   then taxonomy / runtime model / persistence / coordination / "what we can
   learn". Cite source file paths inline (e.g. `daemon.go:1842`) so claims are
   checkable.
3. Match house style: concise prose, tables, ASCII diagrams, no marketing.
   Keep each file under 500 lines (`check:doc-size`).
4. Register the doc in [../registry.json](../registry.json) (copy a sibling
   entry; set a future `reviewAfter`) and link it from this README. Then run
   `npx pnpm@9.15.4 check` and confirm EXIT 0.

## Boundary

Research docs describe **external** systems and our reasoning about them. They
are not the contract: the binding substrate direction lives in
[0018](../decisions/0018-exec-stream-primary-substrate.md) and the integration
contract in [agent-integration-model.md](../agent-integration-model.md). When a
research finding changes our direction, it lands as an ADR update, and the
study is cited as evidence — not the other way around.
