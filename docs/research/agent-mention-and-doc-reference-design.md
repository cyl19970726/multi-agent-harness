# @-mention Agent Assignment + Task<->Doc Reference — Design

A design study for two owner-facing capabilities on the harness Dashboard:
**(1)** treating agents as workspace members you `@`-mention on a Task or Goal
to make them executor or reviewer, and **(2)** letting a Task reference docs it
must consult (input), update (output), and trigger a full doc-sync against. This
is a **design doc only** — no implementation beyond illustrative snippets.

Two owner decisions frame the design and are **locked**:

1. **`@`-mention assignment REUSES existing objects.** It sets the existing
   `Task.assignee_agent_id` / `reviewer_agent_id` and sends a delivered
   `Message(kind=task)`. There is **no new `Assignment` or `Mention` object** and
   **no schema risk**: the `@` is UI sugar over objects that already carry the
   assignment-proof invariant.
2. **Design only this round.** Snippets are illustrative; nothing lands as code.

Doc-reference fields are modeled as **ADR-0017 additive-optional** extensions
(nullable / array, `additionalProperties:false` preserved, no `schema_version`,
`Option`/`Vec` + `#[serde(default)]` in Rust) per
[schemas.md](../schemas.md) and
[the Company OS concept model](../company-os/concept-model.md). Established assignment
doctrine is cited from [concept-model.md](../concept-model.md) and
[data-model.md](../data-model.md); harness building blocks are cited inline as
`file:line`. The doc-sync workflow ties to the runtime in
[dynamic-workflow-runtime-design.md](dynamic-workflow-runtime-design.md).

## 1. Vision — agents as workspace members you @-mention

The owner's framing: an Agent is a **durable workspace member**, like a person in
Notion. To put it to work you `@` it inside a Task or Goal document — `@`-ing an
Agent as **executor** makes it accountable for producing the output; `@`-ing one
as **reviewer** makes it accountable for checking the evidence. The Agent's own
detail page is its presence: it shows *where it is assigned* and *what it is
doing now*, and lets you message it directly.

This is **not a new assignment mechanism** — it is the human-facing surface of
the proof chain the docs already mandate. Assignment truth is a delivered
`Message(kind=task)`, never a bare field write
([concept-model.md](../concept-model.md) §"Task And Message", lines 124-139;
[data-model.md](../data-model.md) Source-Of-Truth table, line 39). The `@`-picker
is the gesture; under it, `assign_task` already sets the field **and** queues the
task message (`crates/harness-cli/src/main.rs:2724-2767`). The owner's mental
model ("@ a member like in Notion") maps one-to-one onto the existing
`design -> assignment message -> report -> evidence -> critic -> decision` order
([Company OS governance](../company-os/governance.md)).

## 2. The @-mention assignment model (reuse, no new object)

### What `@` does

`@`-ing an agent on a **Task** resolves to exactly the existing assign path:

| Surface gesture | Existing object effect | Building block |
| --- | --- | --- |
| `@agent` as executor | set `Task.assignee_agent_id`; `status -> assigned` | `assign_task` `main.rs:2742-2745` |
| (same gesture) | send delivered `Message(kind=task)` | `assign_task` `main.rs:2746-2765` |
| `@agent` as reviewer | set `Task.reviewer_agent_id`; on hand-off send `Message` on `review-request` | `request_task_review_value` `main.rs:2938-2971` |
| `@agent` on a **Goal** | set Goal owner / participating agent (owner accountable for definition) | concept-model roles, lines 104-110 |

`assignee_agent_id` / `reviewer_agent_id` are nullable top-level keys
(`schemas/task.schema.json:49-54`); no schema change is required for the core
gesture. Task `status` already includes `assigned` and `review`
(`task.schema.json:55-58`).

### The assignment-proof invariant must stay visible

The locked decision is that the `@` is a **projection**, not the truth. The UI
must therefore render the *delivered task Message*, not just the field, so the
owner can never be fooled by a field that was set without an instruction landing
— the exact failure mode [concept-model.md](../concept-model.md) line 139 warns
against, and Anti-Drift Invariant #2 ("a task cannot be considered assigned
without a prior `Message(kind=task)`", lines 260-263).

Concretely, the Task document's assignee chip carries a **delivery badge** driven
by the task Message's `delivery_status` (`schemas/message.schema.json:39-42`):

```
Assignee  @codex-1  [delivered 14:32]      <- Message(kind=task) delivered
Assignee  @codex-1  [queued, not delivered] <- field set, instruction pending
```

The `assign_task` core only **queues** the message (it does not call
`ensure_member_accepts_delivery`, `main.rs:2746-2765`), so "queued" is the honest
default state and the badge surfaces the gap rather than hiding it. (The
next-round autonomy path and review hand-off *do* deliver — `main.rs:1170-1191`,
`:2952-2953` — so the badge will read "delivered" there.)

### Notion-style rendering and picker

The Task document shows an **Owner / Assignee / Reviewer** property row, each
slot a **mention chip** (avatar + name + status dot + delivery badge). Editing a
slot opens a Notion-style **`@`-picker** populated from `snapshot.members` (agents
are top-level after #55, `team_ids` may be empty), with a composer to attach the
objective / acceptance criteria that ride in the task Message
([concept-model.md](../concept-model.md) line 130: "the task message should
include objective, acceptance criteria, owned paths, permissions, expected
evidence, and reviewer when relevant"). The picker reuses `POST
/v1/tasks/{id}/assign` (`assign_task_value` `main.rs:2872-2891`, accepts
`assignee` or `assignee_agent_id`).

### Optional convenience field (PROPOSED)

The existing objects fully cover assignment. The only thing not derivable is
*when the `@` chip was placed* if delivery later fails — but that is already
recoverable from the task Message `created_at` / `delivery_status`. So **no field
is required**. If a cheaper read-model is wanted, a single
`Task.mentioned_at` (`string|null`, PROPOSED-optional, ADR-0017 additive) could
cache the gesture timestamp; it is pure convenience and must never be read as
assignment proof. Recommendation: **skip it** unless the chip render proves slow.

## 3. Agent detail: "where assigned + what it is doing now"

Builds on the existing `MemberWorkbench` two-pane page
(`apps/agent-dashboard/src/surfaces/Surfaces.tsx:2871-2913`): `ConversationStream`
+ `MemberRail`. Three panels, each mapped to real snapshot data — no new backend.

**(a) ASSIGNMENTS — where this agent is on the hook.** Tasks where
`assignee_agent_id == agent.id` (executor) or `reviewer_agent_id == agent.id`
(reviewer), grouped by role, each row showing `status`, `branch_ref`, and an
acceptance-criteria slice. Derived by filtering the snapshot `tasks` array
client-side; `MemberRail` already resolves a single `current_task_id` to a Task
card with status badge / branch_ref / acceptance slice
(`Surfaces.tsx:3399-3444`) — ASSIGNMENTS generalizes that from one task to the
filtered set. Pure read-model (`readModel.ts` style, `selectedTask` fallback
`:262-266`).

**(b) DOING NOW — live presence.** Composed from:
- `current_task_id` / `current_proposal_id` (`agent-member.schema.json:149-154`),
  rendered today by `MemberRail` `:3399-3457`;
- the latest `ProviderSession` for this member — `status`
  (queued/running/succeeded/failed, `provider-session.schema.json:69-72`) +
  `prompt_summary` as the step line (`:86-88`), filtered by
  `sessionsByMember` (`readModel.ts:407-410`);
- recent `AgentEvent`s (`agent-event.schema.json:21-33`), e.g. `message_queued`
  (`main.rs:2655-2663`), `runtime-stale` (`main.rs:3821-3829`);
- `runtime_health` 4-probe panel (`RuntimeHealthPanel` `Surfaces.tsx:3740-3762`;
  null/unknown -> amber, never green `:3771-3787`);
- **running workflow steps referencing this agent** — `WorkflowStep` rows whose
  member is this agent, streamed live via WP2 SSE `workflow_step` frames.
  Backend already emits `sse::SseEventFrame::WorkflowRun/WorkflowStep`
  (`main.rs:2329-2340`) and carries `workflow_runs` / `workflow_steps` in the
  snapshot (`:6557-6558`), but the **frontend SSE handler does not yet consume
  those frame types** (`openEventStream` registers only agent_event / message /
  provider_session, `api.ts:95-106`). Wiring them in is the one frontend change
  this panel needs (WP-b).

**(c) CONVERSATION — message the member (real delivery).** The existing
send-message composer / `ConversationStream` (`Surfaces.tsx:2902-2908`), inbox /
outbox derived by `to_agent_id` / `from_agent_id` (`readModel.ts:271-276`).
Already built; reused unchanged.

Net effect: the Agent reads as a member with live presence — assignments,
heartbeat, and a way to talk to it — all from snapshot + SSE.

## 4. Task <-> Doc reference model (the owner's 3 questions)

Today a Task records only **paths** it may modify (`owned_paths`
`task.schema.json:75-81`; `git_metadata.owned_paths`; Proposal `changed_paths`)
and a free `scope_refs` string array (`task.schema.json:100-106`). There is **no
typed field for docs a task consults, docs it must update, or a doc-sync
trigger** (Input A §3; Input B §4 "Task->docs link gap"). The model below adds
two additive-optional arrays and reuses Evidence + the workflow runtime for the
rest.

**Q3 — what docs does a task CONSULT (input)?** Add
`Task.reference_doc_paths: string[]` (PROPOSED optional, ADR-0017 additive,
repo-relative doc paths). Rendered on the Task document as a **"References"**
section, each entry a link opened through the existing single-doc route
`GET /v1/docs?path=` (`main.rs:2467-2478`, allow-lists `docs/`) + the
dependency-free `Markdown` renderer in `DocSheet`
(`Surfaces.tsx:1274-1352`). The executor agent receives these paths **in its
delivery context** — folded into the task Message / launch-spec prompt so the
agent reads them before working. This is the harness analogue of `let-me-try
linked_guides` and rides alongside the existing `skill_refs` the launch spec
already carries.

**Q1 — what docs must a task UPDATE on completion (output)?** Add
`Task.updates_doc_paths: string[]` (PROPOSED optional, ADR-0017 additive). On
completion the agent produces, per updated doc, an
`Evidence { source_type: "doc", source_ref: "docs/...", task_id }` —
`source_type` is free-form (`evidence.schema.json:16-19`) and already carries
convention strings like `"git_worktree"` / `"historical work design"`
(`main.rs:1834,1513`), so `source_type: "doc"` is schema-legal with zero schema
change. The Task document shows a **"Docs to update"** checklist whose status is
derived: a path is *satisfied* once an `Evidence(source_type=doc)` keyed to this
`task_id` references it. This realizes the **completion-sync invariant** ("no
task is complete if it touched a guide but did not co-update it") through the
established proof chain — Message -> report -> Evidence -> Review -> Decision —
**never** a bare field flip (Invariants #2-#8). It references back into the real
`docs/` tree via the same `GET /v1/docs?path=` links.

**Q2 — when do we run a FULL doc-update pass?** A **doc-sync trigger**, not a
field. Three candidate triggers; recommend **goal-closeout auto + manual escape
hatch**:
- *Goal closeout (recommended primary).* The goal-close gate already exists and
  is the one CI-enforced invariant (Invariant #1). At closeout, fan out a
  `doc-sync` workflow over the union of `updates_doc_paths` across the goal's
  tasks. Closeout is the natural "the work is done, reconcile the docs" moment
  and records a reusable operating improvement when warranted.
- *Manual `workflow run --name doc-sync` (recommended escape hatch).* For
  ad-hoc reconciliation between closeouts.
- *Registry staleness (already exists, complementary).* `DocDescriptor.reviewAfter`
  / `reorgTrigger` + `check-doc-governance` flag stale docs
  (`scripts/check-doc-governance.mjs:116-120`) — a time-based net under the
  task-triggered pass, not a replacement.

`doc-sync` is a new built-in `WorkflowDef` registered in
`WorkflowRegistry::builtin()` (`workflow.rs:336-349`) — the same hook the design
study reserves for new workflows. Caveat: the current `WorkflowFn` signature
carries only a `topic: &str` and a fixed codex/claude member pair
(`workflow.rs:320-328`), so a per-doc fan-out must either widen the signature or
encode the target doc paths in the prompt (deferred detail, WP-e).

| Question | Mechanism | Reuse or PROPOSED | Building block |
| --- | --- | --- | --- |
| Q3 consult (input) | `reference_doc_paths` + delivery-context + References UI | PROPOSED field (additive); reuse `/v1/docs`, `DocSheet`, `skill_refs` | `task.schema.json:75-106`; `main.rs:2467-2478`; `Surfaces.tsx:1274-1352` |
| Q1 update (output) | `updates_doc_paths` + `Evidence(source_type=doc)` + checklist | PROPOSED field (additive); reuse Evidence (no schema change) | `evidence.schema.json:16-23`; `main.rs:1834` |
| Q2 full doc-update | `doc-sync` workflow on goal-closeout + manual run | reuse runtime registry + closeout gate; new `WorkflowDef` | `workflow.rs:336-349`; goal-close Invariant #1 |

All three additive fields preserve `additionalProperties:false`, are nullable /
defaulted, and add no `schema_version`, consistent with current schema
governance ([schemas.md](../schemas.md)).

## 5. How it ties together (one ASCII flow)

```
  operator opens Task doc
        |
        |  @-picker (members from snapshot)            <-- agents are members
        v
  @executor  -> set assignee_agent_id + DELIVER Message(kind=task)   [proof]
  @reviewer  -> set reviewer_agent_id (deliver on review hand-off)
        |
        |  reference_doc_paths folded into delivery context (Q3)
        v
  EXECUTOR AGENT works  ===========================================
        |   visible on its detail page "DOING NOW":
        |     current_task_id + ProviderSession(running) + AgentEvents
        |     + live workflow_step frames (SSE)
        v
  produces Evidence + Proposal(changed_paths)
        + Evidence(source_type=doc) for each updates_doc_paths entry (Q1)
        |
        v
  REVIEWER AGENT reviews  -> Review = evidence (not the decision)
        |
        v
  LEADER Decision (accept / revise / split / reject / waive)
        |
        v
  ... at GOAL CLOSEOUT (gate) ...
        |
        v
  doc-sync workflow fans out over the goal's updates_doc_paths (Q2)
        -> reconciles the project docs/ tree, emits reusable learning note
```

Agents appear as members at every hop: picked from `snapshot.members`, shown
working on their own detail page, and fanned out as workflow members at
closeout.

## 6. Sequenced WPs

Ordered by value; the no-schema, pure-frontend reuse lands first.

| WP | Scope | Kind | Size |
| --- | --- | --- | --- |
| **WP-a** | `@`-mention picker + chip + **delivery-badge** assignment-proof render on Task/Goal docs (reuse `assign_task` / `POST /v1/tasks/{id}/assign`) | pure frontend, reuse fields | S |
| **WP-b** | Agent-detail ASSIGNMENTS + DOING-NOW panels; wire `workflow_run` / `workflow_step` into the frontend SSE handler (`api.ts:95-106`) | frontend (one SSE wiring) | S-M |
| **WP-c** | `Task.reference_doc_paths` (additive schema) + executor delivery-context injection + Task-doc **References** UI | additive schema + frontend | M |
| **WP-d** | `Task.updates_doc_paths` (additive schema) + `Evidence(source_type=doc)` completion-sync + "Docs to update" checklist | additive schema + frontend (Evidence reused, no schema change) | M |
| **WP-e** | `doc-sync` built-in workflow (`WorkflowRegistry::builtin()`) + goal-closeout trigger + manual `workflow run`; may need `WorkflowFn` signature widening | runtime built-in | M-L |

WP-a and WP-b ship the owner's "@ a member, see what it's doing" vision with
**no schema risk** (reuse only). WP-c / WP-d add the two additive-optional fields.
WP-e is the largest and depends on the runtime study's open registry-signature
question; it lands last.

## 7. Open questions for the owner

1. **`reference_doc_paths` as a Task field vs derived from `owned_paths`?** A new
   typed field is explicit and directional (input vs output), but `owned_paths`
   already lists touched paths — should "consulted docs" be a separate declared
   list, or inferred from `owned_paths` entries under `docs/`? (Recommendation:
   separate field — `owned_paths` is a *write-permission* scope
   [agent-integration-model.md](../agent-integration-model.md) line 161, not an
   *input* declaration; conflating them loses direction.)
2. **doc-sync trigger: closeout-auto, manual workflow, or both?** Recommendation
   above is **both** (closeout-auto primary + manual escape hatch). Owner to
   confirm whether closeout should *block* on a clean doc-sync or merely *emit*
   one.
3. **Should `@reviewer` gate `status -> done`?** Tie to the existing review /
   closeout gates: a task moves to `review` only with evidence or a blocker, and
   the Leader records the decision ([concept-model.md](../concept-model.md)
   §"Task, Evidence, Proposal, And Decision", lines 147-155). Question: should a
   non-null `reviewer_agent_id` make a delivered review + Decision a **hard gate**
   on `done` (like Invariant #1 on goal-close), or stay a documented-but-unenforced
   target (Invariants #3-#4)? This is the same enforce-now-vs-document question
   the harness faces for Invariants #2-#8.
