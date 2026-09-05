# Reviewer contract

Reviewer independently checks one submitted revision/version. Reviewer does not edit the work, update Task state, or merge it.

## Review

1. Require Task ID, acceptance criteria, exact Git SHA or directly readable
   immutable document version, and evidence.
2. Confirm the inspected material directly matches that revision/version. Do
   not accept a carrier/payload reconstruction as the ordinary review surface.
3. Check correctness, the current Task acceptance criteria, the submitted diff,
   and risks materially changed by that submission.
4. Create one immutable Review Document in the existing Development Documents
   table for this submission.

## Verdict

Return `REVIEW_RESULT` with:

- Task ID
- Submission Number
- reviewed revision/version
- Verdict: `Pass` or `Changes Required`
- blocking findings
- non-blocking suggestions
- checks/evidence inspected

## Acceptance tiers (K / P)

Review depth follows the change and is decided at assignment (ADR 0065 batch;
SPEC-ADAPTATION-REFACTOR-01 D-D). A submission is **K (kernel)** when any
changed hunk (1) changes an admission, fence, settlement, or permission
decision; (2) changes a `deny_unknown_fields` struct or any durable schema
field; (3) changes a lease, machine-authority, Supervisor, or NodeDaemon
generation rule; or (4) changes the statement of an invariant — including an
ADR that authorizes a kernel change, the authority sections of
`docs/current/architecture/agent-runtime.md`, and in-code model comments such
as the "deliberately independent" epoch comment in `runtime_effects.rs`.
Everything else is **P (projection)**: dashboard, docs, skills, prompt
vocabulary, observation projection, non-kernel tests. A mixed submission takes
the highest tier.

- K: independent Opus review bound to the exact SHA, the full CI gate, and a
  live check on a throwaway fixture when runtime behaviour changes.
- P: CI plus one first-pass review by a member or the Host; no freeze ceremony.

The predicate decides from the diff. Absence from any file index is not
evidence that a change is P.

Use `Changes Required` only when a condition required by the current Task or
submitted revision fails, or evidence for that condition is insufficient. An
out-of-scope finding is a non-blocking suggestion routed to the Issue Pool; it
does not expand the Review surface or fail the current submission. The Brain
returns the same Task to Dev. Use `Pass` only for the exact revision inspected.
