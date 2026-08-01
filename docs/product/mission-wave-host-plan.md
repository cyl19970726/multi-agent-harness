# Mission, Host Plan Waves, And Agent Teams

```text
status: canonical
owner_role: product
architecture: ADR 0034 + ADR 0037
```

## Product Promise

Mission/Wave gives a capable Host Agent a durable external memory without
turning that memory into a rigid scheduler.

- **Mission** says what we are trying to accomplish and why.
- **Wave** records the Host's current plan, judgment, and important changes.
- **Agent Team** is an independent reusable group led by the Host that created
  and coordinates it.
- **Assignment messages** say who is doing what.
- **Provider-native sessions** prove what each member actually executed.
- **Agent Members** own end-to-end assignments and may use native subagents
  without giving away responsibility.

## Example

Mission context:

```markdown
# Ship Star Harness host integration

Deliver a repeatable Codex-first integration with a live Dashboard. Preserve
provider-native sessions and use Kimi only for targeted review.

## Success
- Mission, Wave, Team and Member views remain navigable.
- Chat, pending interaction, steer, interrupt and resume are honest.
- All acceptance checks pass from latest master.
```

Wave 1 context:

```markdown
# Wave 1 — Establish the baseline

The Host will validate the current build and start the linked Platform Team.
WorkspaceFixer owns MCP registration and Dashboard startup. InteractionReviewer
checks question/approval behavior.

| Member | Role | Responsibility | Deliverable |
| --- | --- | --- | --- |
| WorkspaceFixer | Primary builder | Build and launch from latest master | Run evidence |
| InteractionReviewer | Reviewer | Exercise interaction edge cases | Review report |

## Host judgment
Start both lanes concurrently. Integration may proceed after the build lane
passes; the interaction lane may carry into the next Wave.
```

When the build lane completes but review is still running, the Host creates:

```markdown
# Wave 2 — Integrate and keep review running

The baseline is reproducible. Merge the build evidence now. Keep
InteractionReviewer on the same MemberRun and native session; its assignment
continues from Wave 1.

Add RepairFixer only if the live interaction check finds a defect.
```

No runtime is moved into Wave 2. The Wave only records the changed Host plan.
The existing assignment correlation and provider session continue.

## Required Behaviors

### Mission

- Stores Markdown `context`.
- Links `agent_team_ids[]`.
- Can link/unlink an independent team without mutating that team.
- Shows linked teams and active runs as relations.
- Closes with an explicit Host outcome; team lifecycle is unchanged.

### Wave

- Stores Markdown `context`, `revision`, `updated_by`, and append-only history.
- Supports update and explicit advance. Use a revision for a small adjustment
  inside the same judgment boundary. Advance and create the next Wave when the
  plan, member composition, responsibility, risk, or decision boundary changes
  materially.
- Does not require all assignments or TeamRuns to finish before advance.
- May cite assignments, members, artifacts, checks, or team runs in prose.
- Optional legacy executor fields remain read-only-compatible, not required on
  the new authoring path.

### Agent Team

- Stable definition with editable name, description, Team Lead, status, and
  member identities.
- The Host Agent that creates and coordinates a team is its **Team Lead**.
  `owner_agent_id` is the compatibility wire field for that identity; `host`
  means the current Host Agent.
- The Team Lead owns formation, assignments, member interaction, composition
  changes, integration, and acceptance. It is not an ordinary MemberRun and is
  not counted in the member roster unless it explicitly joins as an executing
  member.
- Can be standalone or linked to Missions.
- A Mission-scoped TeamRun uses `mission_id` and `agent_team_id`; `wave_id` is
  absent in the primary path.
- Members can continue, join, be renamed, or be explicitly closed across
  Waves. Interrupt stops only the current turn; Close ends one runtime
  generation, and explicit Reopen resumes the same MemberRun/native session.
  Controls require the selected provider mode's real acknowledgement.
- Closing preserves MemberRun coordination and its native-session locator.
  Wave advance and TeamRun completion never close a member implicitly.

### Messaging

- Assignment ownership uses a correlation id.
- Team messages carry typed Host, Member, stable Agent, Operator, or Service
  sender and recipients; display names never define authorship.
- Question, answer, progress, blocker, handoff, review, and control messages
  preserve the correlation.
- Members may send direct peer messages inside the same TeamRun. Routine peer
  collaboration is visible to the Lead but does not require Lead approval.
- Member-to-Host messages are delivered when the control plane receives them.
  Host-to-member and peer messages queue for the recipient's next available
  round.
- A handoff does not automatically dispatch dependent work. The Host reads it
  and explicitly sends the next Assignment or review.
- `origin_wave_id` is optional navigation metadata.
- Host and members can query inbox/status projections without reading provider
  transcripts.
- One current Team Supervisor generation owns provider delivery and live
  controls. Claim, provider receipt, recipient ACK, semantic reply, and Host
  acceptance remain distinct.

### Member autonomy

- Member Goal is derived from the active Assignment, completion standard,
  owned paths, progress/blocker state, and latest Steer. There is no `Goal`
  object.
- A member owns its lane until the Lead sends an accepting `review_result`.
- Provider-native subagents are internal implementation detail. They inherit
  the member's permissions and evidence obligations and do not become
  `MemberRun`s.
- Use another Member when a lane needs its own durable identity, Workspace,
  mailbox, native session, or independent acceptance.
- Steer is live only when the execution mode supports real current-turn
  injection. Unsupported/unavailable Steer fails; the caller may separately
  choose an ordinary queued Message for the next round.
- Provider-pausing questions and approvals are `PendingInteraction`; ordinary
  team coordination is `TeamMessage`.

## UX Contract

Keep the approved Mission Canvas layout. Make targeted semantic changes:

- Mission context is the durable right-rail brief and can expand to full
  Markdown.
- Linked teams appear at Mission scope, not nested as the selected Wave's
  attempt.
- Selected Wave renders its full Markdown context and revision history.
- A compact responsibility table is rendered from Markdown when present.
- Member rows link to Member Focus.
- Carry-over badges use assignment origin and current state; they do not imply
  the Wave owns the member.
- “Advance Wave” is a Host plan decision and remains available while members
  run, with a confirmation summarizing the carry-over.
- “Update plan” edits the selected Wave Markdown and appends a revision.
- Lead Inbox groups member questions, blockers, and review requests. Answers
  reuse correlation and causation and acknowledge the source message.
- Team/Member controls expose the current Supervisor/reconnect state and typed
  author → recipient route; a stale owner disables live control without hiding
  durable mail.
- Member Focus shows the derived Current Assignment, completion standard,
  owned paths, latest Steer, peer/Host thread, and native subagent activity.
- Legacy direct-executor attempts remain visible in historical Missions with a
  clear compatibility label.

## Standard Two-Module Example

The Host gives two durable collaborators independent end-to-end lanes:

```markdown
| Member | Role | Responsibility | Deliverable |
| --- | --- | --- | --- |
| RuntimeBuilder | Runtime owner | Design, implement, and validate Inbox/delivery; use internal subagents as useful | Patch, tests, handoff |
| DashboardBuilder | UX owner | Design, implement, and validate Lead Inbox/Member Goal; use internal subagents as useful | UI patch, checks, handoff |
```

Each member plans its own lane and may delegate bounded design, coding, or test
work to native subagents. The Host answers correlated questions, integrates a
completed lane without waiting for the other, and advances the Wave while the
unfinished member keeps its original MemberRun, Assignment correlation,
Workspace, and provider session. A separate Reviewer Member is used when
high-risk acceptance must be independent.

## Integration Contract

The preferred Host experience is:

```text
thin orchestration skill
        ↓
canonical Harness CLI
        ↓
shared application services
        ├─ optional thin MCP adapter
        ├─ HTTP/Dashboard projection
        └─ append-only Harness coordination store
                  ↓
          provider-native sessions
```

The skill contains orchestration guidance and examples, never authoritative
schema or duplicated architecture. MCP is useful for typed discovery but is
not required for correctness.
