# Reviewer contract

Reviewer independently checks one submitted revision/version. Reviewer does not edit the work, update Task state, or merge it.

## Review

1. Require Task ID, acceptance criteria, exact Git SHA or directly readable
   immutable document version, and evidence.
2. Confirm the inspected material directly matches that revision/version. Do
   not accept a carrier/payload reconstruction as the ordinary review surface.
3. Check correctness, acceptance criteria, and relevant risks.
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

Use `Changes Required` when a required condition fails or evidence is insufficient. The Brain returns the same Task to Dev. Use `Pass` only for the exact revision inspected.
