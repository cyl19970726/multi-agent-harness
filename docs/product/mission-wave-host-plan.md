# Mission, Host Plan Waves, And Agent Teams

```text
status: canonical
owner_role: product
architecture: ADR 0034
```

## Product Promise

Mission/Wave gives a capable Host Agent a durable external memory without
turning that memory into a rigid scheduler.

- **Mission** says what we are trying to accomplish and why.
- **Wave** records the Host's current plan, judgment, and important changes.
- **Agent Team** is an independent reusable group the Host may use.
- **Assignment messages** say who is doing what.
- **Provider-native sessions** prove what each member actually executed.

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
| WorkspaceFixer | Lead builder | Build and launch from latest master | Run evidence |
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
- Supports update and explicit advance.
- Does not require all assignments or TeamRuns to finish before advance.
- May cite assignments, members, artifacts, checks, or team runs in prose.
- Optional legacy executor fields remain read-only-compatible, not required on
  the new authoring path.

### Agent Team

- Stable definition with editable name, description, owner, status, and member
  identities.
- Can be standalone or linked to Missions.
- A Mission-scoped TeamRun uses `mission_id` and `agent_team_id`; `wave_id` is
  absent in the primary path.
- Members can continue, join, be renamed, or deactivate across Waves.
- Deactivation preserves the MemberRun history; an active provider turn must
  first use its real interrupt path.

### Messaging

- Assignment ownership uses a correlation id.
- Question, answer, progress, blocker, handoff, review, and control messages
  preserve the correlation.
- `origin_wave_id` is optional navigation metadata.
- Host can query an inbox/status projection without reading every transcript.

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
- Legacy direct-executor attempts remain visible in historical Missions with a
  clear compatibility label.

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
