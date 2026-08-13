# Fidelity And Review Gates

Use these gates during implementation, self-review, independent review, and owner
acceptance.

## Staged gates

1. **Geometry gate**: module shell, regions, proportions, dominant surface, first
   viewport, and scroll ownership match the contract. Stop before styling details
   if this fails.
2. **Primary-journey gate**: navigation, entities, main actions, real data, and
   product/authority/privacy invariants work.
3. **Module-coverage gate**: every remaining surface, transition, and recovery path
   discovered by the approved product/UX work matches its contract.
4. **Visual-system gate**: typography, density, spacing, lines/surfaces, controls,
   icons/assets, focus, and motion form one coherent language across the module.
5. **Responsive/state gate**: required desktop/tablet/mobile transformations and
   loading/empty/dense/stale/permission/error/long-content states pass.
6. **Exact-revision self-review gate**: the complete coverage matrix, comparisons,
   engineering checks, and P0/P1 repair are finished before handoff.
7. **Independent spec-review gate**: fresh reviewers assess exact-revision evidence
   against the frozen spec and references.
8. **Owner gate**: owner/user accepts the complete module; this may overturn an
   earlier review PASS and invalidate it.

Each implementation gate inherits the spec's frozen screenshot threshold and
dimension floors. Do not advance merely because the stage is coded; advance only
when its blocking comparisons and hard invariants pass.

Do not request an independent review for each small local fix. Use bounded internal
critique while building, then hand off one complete-module candidate.

## Two acceptance classes

Hard invariants are pass/fail and must all pass. Examples include product meaning,
authority, privacy, action safety, data provenance, reachable journey completion,
accessibility blockers, and required surface coverage. A visual score cannot offset
a hard failure.

Visual fidelity is scored per blocking screenshot across these default dimensions:

| Dimension | Weight |
| --- | ---: |
| composition, geometry, and region proportions | 25 |
| hierarchy and information density | 20 |
| typography hierarchy and text behavior | 15 |
| spacing, alignment, and rhythm | 15 |
| color, line, surface, and material treatment | 10 |
| controls, icons, avatars, and other assets | 10 |
| state, focus, interaction, and motion fidelity | 5 |

Projects may change weights before implementation. Record the revision and reason.
Never tune weights after seeing the implementation.

Default high-fidelity threshold: every blocking screenshot scores at least 95/100,
every dimension meets its declared floor, and P0/P1 count is zero. Do not average
screenshots: a 99 and a 91 are not a 95 PASS. A project may set a different bar,
but the spec must freeze it before implementation.

Scores diagnose; they do not replace observations. Every score below full marks
must cite the mismatch, causal layer, repair, and recheck condition. Side-by-side,
overlay, pixel/dimensional measurements, and browser inspection are complementary.

## Evidence bundle

Bind evidence to exact implementation revision, frontend/server build revision when
separate, spec revision, reference hashes, route/entity/representative-data ids,
viewport, state, capture time, environment, and screenshot hash. Mark fixture and
real-data evidence separately.

For each required screenshot include first impression, hard-invariant result,
dimension scores, deviations, findings, and pass/fix/reject. Also record console,
network/data provenance, overflow, keyboard/focus, accessibility, reduced motion,
performance when relevant, and engineering checks.

## Invalidation

Invalidate prior self-review and independent review when any of these changes:

- implementation or relevant frontend/server build revision;
- canonical spec revision;
- approved reference or hash;
- required coverage matrix, threshold, or deviation set;
- representative data changes enough to alter the accepted layout;
- Owner/User rejects the candidate.

A narrow code-only delta may preserve unaffected evidence only when a fresh reviewer
records why the affected-surface set is complete and rechecks it. Never preserve a
PASS merely because the developer says rendering did not change.

## Review failure modes

Reject reviews that:

- only confirm component/data presence, tests, console, or overflow;
- repeat the Implementer's completion narrative;
- give one verdict without inspecting every blocking screenshot/scenario;
- average scores across screenshots;
- list local polish defects without a module/page-level diagnosis;
- accept one strong frame while the rest of the discovered module remains improvised;
- call themselves independent while sharing the Implementer's reasoning context.
