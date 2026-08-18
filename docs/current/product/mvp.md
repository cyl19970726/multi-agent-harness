# MVP

## Purpose

The active MVP proves a simple Firm workflow:

```text
AgentTeam (durable, flat) → TeamRuns → MemberRuns → Work → Result
              └──────────── WorkDelegation ────────→ peer Team
```

## Required slice

1. AgentTeam is durable and flat; it is created without any Mission.
   Pre-cutover Teams may carry read-only `legacy_mission_id` provenance
   (DOC-108).
2. AgentTeam requires `host_agent_id` and one immutable `node_id`. Teams do
   not nest and Members do not cross machines.
3. AgentTeamRun requires Team, execution Node, and project binding.
4. Work is the responsibility kernel. Member planning and Sub-Agent checklists
   stay internal unless promoted to a Finding, Result, Failure, or new Work.
5. WorkDelegation is the only cross-Team responsibility transfer. It is
   explicit, versioned, cycle-safe, and observable from source and target.
6. One machine NodeDaemon drives every admitted local TeamRun. Public surfaces
   fail explicitly when it is unavailable; there is no per-run fallback.
7. Provider-native sessions own transcript and tool truth. Firm owns identity,
   Work assignment, identity-first Message plus per-recipient
   CanonicalMessageDelivery, lifecycle, evidence refs, and acceptance. Work,
   Message delivery, and RuntimeCommand remain independent planes.
8. Global Work is an aggregate/filter view over the same Team Work kernel,
   not a second lifecycle model.

## Acceptance journey

1. Initialize a Node and register two isolated Execution Spaces.
2. Create one flat durable Team in each space.
3. Start one NodeDaemon and concurrently run both Teams.
4. Assign Work, report Findings and Result, and delegate one bounded Work item
   between Teams.
5. Restart the daemon and prove parent-generation fencing, recovery without
   duplicate delivery, and isolation from one broken store.
6. Verify CLI, HTTP, MCP, and Dashboard show the same Team/Node/delegation truth.
7. Complete the TeamRun with explicit Host acceptance on the Work records —
   without inventing Wave gates or deleting reusable records.

## Non-goals

- nested Teams or cross-machine Members;
- a second Global Work state machine;
- Mission/Wave-owned scheduling or Run lifecycle;
- hidden automatic Team selection in production;
- permission complexity beyond what is needed to run the workflow safely;
- Harness-owned transcript, tool-call, or chain-of-thought storage.
