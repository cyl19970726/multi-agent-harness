# Implementation notes

## Reuse before adding dependencies

The current stack already has React, Tailwind CSS 4, Lucide, Radix Tabs, Tooltip, Separator, ScrollArea, `clsx`, and `class-variance-authority`. That is enough for the V3 structure and most interaction states.

Do not add a general dashboard or animation framework for this pass. Implement the visual hierarchy with layout, typography, pseudo-elements, CSS variables, and small composable primitives.

Recommended primitives:

- `ExecutionRail`: ordered Wave line, ordinal/status marker, active progress trace.
- `WaveWorkspace`: border-light expanded current Wave region.
- `PresenceRail`: connected MemberRun lanes with identity, assignment, and live state.
- `EventSpine`: semantic activity rows anchored to a shared chronological rail.
- `DecisionAnchor`: relates one pressure action to its member, event, or gate.
- `ContextSection`: typography/divider-based rail section; a bordered container is optional, not the default.
- `ConnectionPulse`: small shared live/transport status indicator.

## Semantic icon language

Operational icons remain code-native SVGs rather than generated raster assets so they stay crisp at 16–32px, inherit semantic tokens, and remain accessible under zoom and high-density displays.

- assignment: `SendHorizontal`, active blue;
- handoff: `ArrowRightLeft`, active blue;
- runtime action: `TerminalSquare`, active blue or result tone;
- evidence/artifact: `FileCheck2`, accepted green when succeeded;
- review request: `ShieldCheck`, waiting/pressure tone;
- decision: `CheckCircle2`, decision or accepted tone;
- blocker: `ShieldAlert`, pressure coral/red.

Every icon keeps an adjacent text label; color is reinforcement, never the only meaning.

## Motion

Use motion only to explain state change:

- active execution trace: 1.8–2.4s low-contrast translated highlight along the blue rail;
- live pulse: subtle scale/opacity loop on the current activity dot;
- new durable event: one 350–500ms background fade, never repeated;
- Wave expansion: 180–240ms opacity and height transition;
- context drawer/sheet: Radix-compatible transform and opacity transition.

All motion must stop under `prefers-reduced-motion: reduce`. Avoid continuously animating more than the active rail and one live indicator.

## Optional dependency decision

Start with CSS transitions and keyframes. Consider `motion` only if interruptible Wave-to-Team shared-layout transitions or complex enter/exit orchestration cannot remain correct with CSS. If added, use it for those transitions only; do not wrap every component.

## Visual acceptance

After approval, preserve these expected images and capture the real browser using the deterministic Mission/Wave fixture at 1440×1000, 900×1180, and 390×844. Compare hierarchy, scroll ownership, pressure visibility, reduced-motion behavior, and route continuity—not only pixel similarity.
