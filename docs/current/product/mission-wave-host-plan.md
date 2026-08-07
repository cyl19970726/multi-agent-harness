# Mission, Host Plan Waves, And Agent Teams

```text
status: canonical; Works cutover in progress; Wave retired for Host judgment (ADR 0051)
owner_role: product
architecture: ADR 0034 + ADR 0037 + ADR 0050 + ADR 0051
```

## Product Promise

Mission plus its append-only Mission Log gives a capable Host Agent a durable
external memory without turning that memory into a rigid scheduler.

- **Mission** says what we are trying to accomplish and why.
- **Mission Log** records the Host's judgment as append-only entries
  (`judgment`/`replan`/`recovery`/`closeout_evidence`) — durable, versioned,
  and read before every recovery or re-entry (ADR 0051). Wave played this
  role before the cutover; historical Wave rows remain readable, but no new
  Host judgment is written there.
- **Agent Team** is an independent reusable group led by the Host that created
  and coordinates it.
- **Works** say what exists, who owns it, and its current execution state.
- **Messages** let Host and Members discuss Works without becoming task state.
- **Provider-native sessions** prove what each member actually executed.
- **Agent Members** own end-to-end Works and may use native subagents
  without giving away responsibility.

## Example

Mission context:

```markdown
# Ship Star Harness host integration

Deliver a repeatable Codex-first integration with a live Dashboard. Preserve
provider-native sessions and use Kimi only for targeted review.

## Success
- Mission, Mission Log, Team and Member views remain navigable.
- Chat, pending interaction, steer, interrupt and resume are honest.
- All acceptance checks pass from latest master.
```

Mission Log entry 1, `--kind judgment`, appended before the Host starts either
lane (log-before-act, ADR 0051):

```markdown
# Establish the baseline

The Host will validate the current build and start the linked Platform Team.
WorkspaceFixer owns MCP registration and Dashboard startup. InteractionReviewer
checks question/approval behavior.

| Member | Role | Responsibility | Deliverable |
| --- | --- | --- | --- |
| WorkspaceFixer | Primary builder | Build and launch from latest master | Run evidence |
| InteractionReviewer | Reviewer | Exercise interaction edge cases | Review report |

Start both lanes concurrently. Integration may proceed after the build lane
passes; the interaction lane may carry into a later entry.
```

```bash
firm mission log append --mission-id <id> --kind judgment \
  --body "$(cat wave-1-judgment.md)"
```

When the build lane completes but review is still running, the Host appends
the next judgment BEFORE integrating, not after:

```markdown
# Integrate and keep review running

The baseline is reproducible. Merge the build evidence now. Keep
InteractionReviewer on the same MemberRun and native session; its Work
continues unchanged.

Add RepairFixer only if the live interaction check finds a defect.
```

```bash
firm mission log append --mission-id <id> --kind judgment \
  --body "$(cat integrate-judgment.md)"
```

No runtime is moved by appending a Mission Log entry. The entry only records
the changed Host judgment. The existing Work, Member identity, and provider
session continue.

## Required Behaviors

### Mission

- Stores Markdown `context`.
- Links `agent_team_ids[]`.
- Can link/unlink an independent team without mutating that team.
- Shows linked teams and active runs as relations.
- Closes with an explicit Host outcome; team lifecycle is unchanged.

### Mission Log

- Append-only rows: `mission_id`, `revision` (monotonic per Mission,
  store-assigned — callers never choose it), `kind` in `judgment` / `replan` /
  `recovery` / `closeout_evidence`, a non-empty Markdown `body`, `actor`, and
  `created_at`. There is no update, delete, or gate: a correction is a new
  entry, not a mutation of an old one, and an append-only log has nothing
  analogous to a Wave gate to accept, revise, or block (ADR 0051).
- Log-before-act: at a material decision point — a new Work tranche, a
  composition change, recovery, or a model/provider switch — the Host appends
  the judgment entry before mutating runs or Works, never as after-the-fact
  narration.
- Mandatory readers: the recovery entrypoint (`firm team-run recover`)
  prints the linked Mission's Log tail before any mutation; a replacement
  Host derives its native goal/plan from `mission show` plus the Log tail
  plus the board summary, never from provider-native goal/plan state, which
  is explicitly disposable.
- May cite Works, members, artifacts, checks, or team runs in prose, same as
  the Wave memo it replaces.
- **Historical Waves remain readable.** Rows created before this cutover
  (Markdown `context`, `revision`, `updated_by`, append-only history, and
  optional legacy executor fields) stay available through `wave
  list`/`show`/`history`. There is no data migration, and no new Host
  judgment is written there — `wave create`/`update`/`advance`/`gate` are
  retired on every surface (CLI, HTTP, MCP).

### Agent Team

- Stable definition with editable name, description, Team Lead, status, and
  member identities.
- The Host Agent that creates and coordinates a team is its **Team Lead**.
  `owner_agent_id` is the compatibility wire field for that identity; `host`
  means the current Host Agent.
- The Team Lead owns formation, Works, member interaction, composition
  changes, integration, and acceptance. It is not an ordinary MemberRun and is
  not counted in the member roster unless it explicitly joins as an executing
  member.
- Can be standalone or linked to Missions.
- A Mission-scoped TeamRun uses `mission_id` and `agent_team_id`; `wave_id` is
  absent in the primary path.
- Members can continue, join, be renamed, or be explicitly closed across
  re-plans. Interrupt stops only the current turn; Close ends one runtime
  generation, and explicit Reopen resumes the same MemberRun/native session.
  Controls require the selected provider mode's real acknowledgement.
- Closing preserves MemberRun coordination and its native-session locator.
  Appending a Mission Log entry and TeamRun completion never close a member
  implicitly.

### Works and messaging

- Work ownership uses the latest Work projection rebuilt from ordered
  WorkOperations; each operation preserves its append-only WorkEvent audit and
  delivery deltas. It is not inferred from a Message or correlation id.
- Host assignment and later Host-originated resume/request-changes/rebind
  create WorkDelivery records that use the Supervisor's durable delivery
  substrate without becoming authored TeamMessages. Member self-claim is an
  atomic pull inside the bound runtime and therefore creates no loopback
  delivery; its Claimed WorkEvent and command result are the possession proof.
- Ready unassigned Works may be atomically claimed by eligible Members. Host
  assignment remains available for constrained or high-risk work.
- Team messages carry typed Host, Member, stable Agent, Operator, or Service
  sender and recipients; display names never define authorship.
- Messages may link a Work and preserve conversational correlation, but never
  change owner, status, readiness, submission, or acceptance by themselves.
- Members may send direct peer messages inside the same TeamRun. Routine peer
  collaboration is visible to the Lead but does not require Lead approval.
- Member-to-Host messages are delivered when the control plane receives them.
  Host-to-member and peer messages queue for the recipient's next available
  round.
- Accepting a prerequisite Work may make another Work ready. The Host may
  assign it, or an eligible Member may atomically claim it.
- `origin_wave_id` is optional navigation metadata.
- Host and members can query inbox/status projections without reading provider
  transcripts.
- One current Team Supervisor generation owns provider delivery and live
  controls. Claim, provider receipt, recipient ACK, semantic reply, and Host
  acceptance remain distinct.

### Member autonomy

- Member Goal is derived from the active Work, completion standard,
  owned paths, progress/blocker state, and latest Steer. There is no `Goal`
  object.
- A member owns its Work through correction until the Team Host accepts it.
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
- Linked teams appear at Mission scope, not nested as a selected Wave's
  attempt.
- The Mission Log renders newest-first as a plain list: revision, kind badge
  (`judgment`/`replan`/`recovery`/`closeout_evidence`), body, and created_at.
  It is a read projection in this cutover; recording judgment goes through
  `firm mission log append`, not a Dashboard write form.
- The Wave canvas/list is labeled **Historical** and renders read-only: full
  Markdown context and revision history for rows that predate ADR 0051. It
  never gains new entries.
- A compact responsibility table is rendered from Markdown when present.
- Member rows link to Member Focus.
- Carry-over badges use Work origin and current state; they do not imply a
  Mission Log entry or a historical Wave owns the member.
- Lead Inbox groups member questions and coordination. Works separately expose
  blockers, submissions, and reviews, with linked discussion where present.
- Team/Member controls expose the current Supervisor/reconnect state and typed
  author → recipient route; a stale owner disables live control without hiding
  durable mail.
- Member Focus shows Current Work, queued Works, eligible unassigned Works,
  completion standard,
  owned paths, latest Steer, peer/Host thread, and native subagent activity.
- Legacy direct-executor attempts remain visible in historical Missions with a
  clear compatibility label.

## Standard Two-Module Example

The Host gives two durable collaborators independent end-to-end lanes:

```markdown
| Member | Role | Responsibility | Deliverable |
| --- | --- | --- | --- |
| RuntimeBuilder | Runtime owner | Design, implement, and validate Inbox/delivery; use internal subagents as useful | Submitted Work with patch/tests |
| DashboardBuilder | UX owner | Design, implement, and validate Works/Member Focus; use internal subagents as useful | Submitted Work with UI checks |
```

Each member plans its own lane and may delegate bounded design, coding, or test
work to native subagents. The Host answers correlated questions, integrates a
completed lane without waiting for the other, and appends the Mission Log
judgment while the unfinished member keeps its original MemberRun, Work
ownership, Workspace, and provider session. A separate Reviewer Member is
used when high-risk acceptance must be independent.

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
