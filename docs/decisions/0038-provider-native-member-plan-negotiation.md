# ADR 0038: Provider-Native Member Plan Negotiation

```text
status: superseded by ADR 0039
date: 2026-07-24
extends: ADR 0032 provider-native execution truth; ADR 0037 member autonomy
```

> Historical decision only. ADR 0039 removes the dedicated Plan Mode and Plan
> Gate. Planning is now ordinary correlated Host/Member conversation, with an
> optional Markdown artifact and no provider-specific approval state.

## Context

Codex, Kimi, Claude Code, and future providers expose planning in different
ways. Codex app-server has collaboration modes, structured plan events, and a
thread Goal. Kimi ACP has a session mode/config option and plan updates. Other
modes may only support a prompt-level planning convention.

Treating those protocols as the product model would make Agent Team behavior
provider-specific. Ignoring them would discard useful read-only planning,
native session continuity, and visible plan progress.

The product needs one small negotiation:

```text
Member plan proposal
  -> Host challenge
  -> Member revised plan
  -> Host approval
  -> execution
```

It must not add a universal Goal object, Task Graph, conditional delivery, or a
second provider transcript.

## Decision

### Assignment is the durable Goal

The current correlated `assignment` is the only durable Member work identity.
The Dashboard derives “Member Goal” from that Assignment, completion criteria,
owned paths, status, blockers, and controls.

When a provider supports a native Goal, the adapter projects the Assignment
objective into that provider session. It is a session-local execution aid, not
a Harness product object and not an alternative source of ownership.

For approval-gated planning, that native Goal remains `paused` while the
Member and Host debate the plan. It becomes `active` only after correlated
`plan_approval`. This prevents provider Goal continuation from crossing the
Host approval boundary.

### Plan negotiation is one Assignment message chain

Planning is optional and uses four provider-neutral `TeamMessage` kinds:

| Kind | Sender → recipient | Causation |
| --- | --- | --- |
| `plan_request` | Host → Assignment owner | Assignment message |
| `plan_proposal` | owner → Host | latest request or feedback |
| `plan_feedback` | Host → owner | latest proposal |
| `plan_approval` | Host → owner | latest proposal |

One Assignment correlation may contain:

```text
plan_request
  -> plan_proposal
  -> (plan_feedback -> plan_proposal)*
  -> plan_approval
```

Harness rejects a proposer other than the Assignment owner, a reviewer other
than Host, broken causation, approval of a superseded proposal, and further
plan messages after approval.

Plan revisions keep the same `MemberRun`, Assignment correlation, Workspace,
and provider-native session. They do not create Waves. Host may update or
advance a Wave separately when the overall plan, roster, responsibility, risk,
or decision boundary changes.

### Provider adapters expose capability honestly

Each `ProviderIntegrationProfile` snapshots:

- `plan_mode = native | emulated | unsupported | unknown`
- `goal_mode = native | emulated | unsupported | unknown`

`native` means the selected execution mode has a verified provider protocol.
`emulated` means Harness can preserve the negotiation contract but the provider
does not expose an equivalent native state. `unsupported` must fail rather than
pretend execution was held behind a read-only planning boundary.

Current adapter design:

| Provider mode | Plan | Goal projection |
| --- | --- | --- |
| Codex app-server | native collaboration Plan mode and plan events | native thread Goal |
| Kimi ACP | native session mode/config and plan updates | Assignment prompt projection |
| Codex one-shot exec | unsupported for approval-gated planning | unsupported |
| Claude CLI | emulated until deterministic mode coverage exists | emulated |

This table describes adapter coverage, not permission to upgrade a provider.
Version review remains governed separately.

For Codex app-server, native Plan/default selection is carried by the
experimental `turn/start.params.collaborationMode` object with a complete
preset (`mode` plus `settings.model`; built-in instructions are selected with
`settings.developer_instructions = null`). It is not a CLI config override.
The adapter sends the mode on every turn so resuming a thread cannot
accidentally retain the previous planning boundary.

### Native state is observed, then promoted explicitly

Provider plan updates remain transient native-session state while the planning
turn runs. The adapter promotes only the Member's explicit submitted Markdown
plan into durable `plan_proposal`.

When a provider exposes both a structured progress checklist and an explicit
final Markdown proposal, the explicit proposal is canonical. The structured
plan is only a fallback when no final proposal exists.

Harness does not persist raw provider plan streams, chat, tool calls,
reasoning, or transcript events. The provider-native session remains execution
truth.

If the provider itself pauses on an approval or exit-plan request, that pause is
a `PendingInteraction` linked to the same Member and Assignment. A provider
`completed` event never means Host approved the plan. The canonical semantic
decision is still `plan_approval`.

### Approval gates execution, not communication

After `plan_request`, the member runs in a native or honestly emulated
plan-only mode and becomes `reviewing`. After submitting a proposal it becomes
`waiting`.

- `plan_feedback` resumes planning in the same session and produces a revision.
- `plan_approval` switches the same session to execution mode.
- TeamRun cancellation stops the wait.
- Question, answer, peer coordination, and Host observation remain available.

Adapters correlate provider lifecycle notifications to the active provider
turn. A delayed completion from a Goal update or earlier plan revision must
never complete the current revision.

The Host may debate assumptions in prose. Harness does not reduce a plan to
workflow steps or claim that plan item completion proves semantic success.

## UX Contract

Team War Room has a first-class Plan Review surface, separate from Lead Inbox
and provider `PendingInteraction`:

- one card per assigned member;
- native/emulated/unsupported capability;
- proposal revision and current negotiation state;
- Host challenge and approval controls;
- delivery, correlation, and causation remain inspectable.

Member Focus shows the current Assignment Goal followed by Execution Plan,
revision history, latest Host challenge, approval, and provider capability.
Provider-native plan progress may appear live but is not replayable evidence.

## Consequences

- Providers can join through a small adapter contract without dictating the
  product object model.
- Host keeps final responsibility while members retain plan autonomy.
- Plans can be argued and revised without replacing the member or session.
- The Dashboard distinguishes planning, waiting for Host, approved, and
  executing without reading private reasoning.
- Simple Assignments can skip planning entirely.

## Acceptance

The deterministic and live acceptance paths must prove:

1. invalid sender, correlation, causation, revision, and approval order fail;
2. Codex app-server receives its Assignment as native Goal, plans in Plan mode,
   records `collaborationMode=plan` in the native planning turn context,
   resumes the same thread for revisions, records `default` for execution, and
   executes only after approval;
3. Kimi ACP switches to `plan`, exposes a proposal, stays in the same session,
   then switches to `default` after approval;
4. unsupported modes do not fabricate a plan-only boundary;
5. CLI, HTTP, MCP, and Dashboard reconstruct the same message chain; and
6. no raw provider plan/thinking/transcript stream enters Harness storage.
