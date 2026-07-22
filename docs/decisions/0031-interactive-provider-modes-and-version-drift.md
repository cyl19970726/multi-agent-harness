# ADR 0031 — Interactive provider modes and adapter version drift

```text
status: accepted_and_implemented
date: 2026-07-21
scope: Agent Team Member chat, steering, interruption, and provider upgrades
```

ADR [0032](0032-provider-native-session-is-execution-truth.md) additionally
requires every interactive mode to use its provider-native session store for
history and resume rather than a Harness transcript mirror.

## Context

An Agent Team Member is not only a one-shot executor. Operators and the Lead
need to send follow-up messages, steer active work, answer provider requests,
and interrupt a turn without fabricating terminal state. Providers expose
different control surfaces, and those surfaces change across releases.

Codex `exec --json` is a good non-interactive batch stream, but it cannot accept
same-turn input. Current Codex app-server exposes persistent threads,
`turn/steer`, `turn/interrupt`, streamed items, approvals, and thread resume.
Kimi ACP exposes its own session prompt, reverse requests, and cancellation
protocol. These are distinct execution modes, not interchangeable provider
labels.

## Decision

### Chat and control semantics

The shared product actions are:

| Product action | Active interactive turn | Idle interactive session | Non-interactive turn |
| --- | --- | --- | --- |
| Send message | steer current turn | start follow-up turn | queue for next round |
| Interrupt | provider interrupt, then await terminal acknowledgement | no-op | unsupported unless a real process handle is controlled |
| Stop member | interrupt active turn, close adapter runtime, then mark stopped | close runtime, then mark stopped | only after observed process termination |
| Provider question | durable PendingInteraction routed to Lead/Human/Policy | same | explicit blocker/follow-up only |

The Dashboard composer must show which result occurred: **Steered now**,
**Started follow-up**, or **Queued for next round**. An interrupt control enters
an **Interrupting** state and becomes terminal only after provider confirmation
or an explicit recovery attestation.

### Codex mode selection

- `codex_exec` remains the batch/read-only mode for bounded one-shot work.
- `codex_app_server` is the interactive Agent Team Member mode for chat,
  same-turn steer, approvals, and interrupt. Its provider thread id is the
  native-session binding. Restart-time `thread/resume` is implemented through
  an explicit resume binding; capability state remains mode/version specific.
- The two modes have separate ProviderIntegrationProfiles and acceptance gates.
- `codex_exec` honestly reports `interaction_mode=unsupported` and
  `supports_cancel=false`; `codex_app_server` reports only the controls its
  live adapter now exercises.

### Version drift governance

Every execution-mode profile records:

- detected provider version;
- adapter contract version;
- exact provider versions reviewed against that contract;
- adapter review date;
- compatibility status and explanation.

`harness member providers` probes installed versions. `--fail-on-review` is the
CI/periodic-audit gate. A new unreviewed version becomes `review_required`; it
does not silently become compatible or incompatible. Review must regenerate
provider schemas/capability snapshots and run mode-specific deterministic and
live acceptance before adding the new version to the reviewed set.

Dashboard exposes the same compatibility state on MemberRun. A later strict
production policy may block `review_required`; default development mode warns
so provider releases do not unexpectedly make local development unusable.

## Consequences

- Provider name no longer determines chat or interruption capability; execution
  mode does.
- Agent Team UI can remain shared while buttons are capability-driven.
- Codex app-server is an explicit selectable execution mode, not a hidden
  fallback from `codex_exec`.
- Release monitoring becomes reproducible and suitable for scheduled checks.
- Provider protocol vocabulary alone never proves Harness lifecycle control.
- Version review also covers native-store discovery/read/resume compatibility;
  a stream parser passing is not enough.

## Acceptance

- installed Codex and Kimi versions probe as `current` when they match reviewed
  versions;
- a fake/new version produces `review_required` and `--fail-on-review` fails;
- MemberRun snapshots and Dashboard expose compatibility state;
- deterministic acceptance proves `turn/steer`, `turn/interrupt`, Kimi
  `session/cancel`, and streamed activity against generated schemas from the
  installed Codex version;
- provider request routing is durable through PendingInteraction;
- restart-time Codex `thread/resume`/`exec resume` and Kimi `session/load` use
  explicit `NativeSessionRef` bindings and fail rather than silently opening a
  fresh session.
