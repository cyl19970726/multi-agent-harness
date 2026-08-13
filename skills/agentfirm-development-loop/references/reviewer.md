# Reviewer contract

Reviewer independently checks one submitted revision/version. Reviewer does not edit the work, update Task state, or merge it.

## Review

1. Require Task ID, acceptance criteria, exact revision/version, and evidence.
2. Confirm the inspected material matches that revision/version.
3. Check correctness, acceptance criteria, and relevant risks.
4. Create one Review record for this submission.

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

