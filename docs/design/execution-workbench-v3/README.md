# Execution Workbench V3

This visual-direction package refines the Mission and Agent Team plugin experience around one operator question: **what is happening now, what needs me, and what happens next?**

V3 replaces the card-heavy dashboard treatment with three surface levels:

1. a continuous execution rail for Mission/Wave progress;
2. a calm, border-light work surface for the currently selected Wave or TeamRun;
3. bounded floating controls only for decisions, intervention, and transient context.

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

The normalized Fidelity V2 direction was approved by the user on 2026-07-21 and is now the only V3 expected design contract. Superseded `1536×1024` concepts and unreferenced intermediate comparisons were deleted so they cannot be mistaken for an active UI version.
