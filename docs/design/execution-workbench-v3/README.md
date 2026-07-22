# Execution Workbench V3

This visual-direction package refines the Mission, Agent Team, and MemberRun
plugin experience around one operator question: **what is happening now, what
needs me, and what happens next?**

V3 replaces the card-heavy dashboard treatment with three surface levels:

1. a continuous execution rail for Mission/Wave progress;
2. a calm, border-light work surface for the currently selected Wave or TeamRun;
3. bounded floating controls only for decisions, intervention, and transient context.
4. a Codex-like continuous member workspace for one run-scoped collaborator.

The expected images are design intent, not browser evidence. They must remain immutable after approval. Implementation acceptance will use the same deterministic fixture and desktop/tablet/mobile capture path as Workbench V2.

## Product invariants

- Mission is durable intent and owns an ordered set of Waves.
- Wave remains lightweight: objective, executor, outcome/artifacts, and gate.
- Agent Team is an executor attempt inside a Wave, not a replacement hierarchy.
- Assignment messages and correlations explain member ownership and handoffs.
- The Host owns the Wave gate; the Team page can report readiness but cannot accept its own Wave.
- Raw provider thinking is never shown as durable history.
- No Goal, GoalPhase, task graph, or generic Project object appears.

## Selected expected designs

- `expected/mission-wave-canvas/running-gate-pending--desktop-fidelity-v2-1440x1000.png`
- `expected/team-war-room/running-needs-you--desktop-fidelity-v2-1440x1000.png`
- `expected/member-run-focus/completed-history--desktop-concept-v4.png`
- `expected/member-run-focus/running-needs-you--tablet-context-open-900x1180.png`
- `expected/member-run-focus/running-needs-you--mobile-context-open-390x844.png`

The normalized Fidelity V2 Mission and Team direction was approved by the user
on 2026-07-21. The MemberRun completed-history V4 desktop expected was approved
on 2026-07-22 and replaces the earlier desktop candidate. Its warm-editorial
implementation, exact 1536×1024 browser capture, comparison, and overlay were
approved by the user on 2026-07-22. Tablet/mobile V3 references remain only
until V4 responsive expectations are generated from this accepted desktop
language.
