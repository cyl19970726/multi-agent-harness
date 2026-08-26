# Dev contract

## Live Agent Team evidence

When the Task requires native Agent Team evidence, name the scenario before it
starts. A `coordination_canary` proves only its stated authority, delivery, or
lifecycle seam and must report that limitation. Do not call a read-only SHA
check, echo, or no-edit Work a coding dogfood run.

A `coding_dogfood` must produce a changed candidate revision, real changed
files and checks, a canonical WorkReport, independent AgentMember review, exact
Host acceptance, and provider-native implementer tool start plus terminal tool
evidence. Validate the response-local id/count bundle with
`pnpm verify:agent-team-dogfood -- <evidence.json>`; never copy the provider
transcript into Harness state or the evidence bundle.

Dev owns implementation and decides when work is ready to review.

## Work

- Begin from the `TASK_ASSIGNED` handoff. Confirm the Task ID, linked GitHub
  Issue when present, actual starting revision/context, and any scope conflict.
  This receipt is not a separate Claim lifecycle or message type.
- Follow the Task goal and acceptance criteria.
- Iterate freely without Candidate or readiness approval.
- Keep Task blocker and useful progress evidence understandable to the Brain.
- Send `ATTENTION_REQUIRED` only for a real blocker or missing decision.

When implementation exposes another problem:

- fix it in the same Task when it is required by that Task's acceptance;
- send the Brain a concise finding with reproduction and impact when it is an
  independent follow-up;
- do not silently broaden scope or create a replacement Task yourself unless
  the Brain explicitly delegates issue triage.

## Submit

Send `READY_FOR_REVIEW` with:

- Task ID
- exact code SHA or named immutable document version
- concise change summary
- checks performed and results
- evidence links
- known limitations

Do not continue changing the submitted revision and still call it the same submission. New changes produce the next submission of the same Task.

For a document submission, provide one directly readable named immutable
version. For a code or machine-data submission, provide the exact Git SHA and
paths; add a file hash when the file itself is the reviewed artifact. Large
inventories and structured manifests belong in the repository, not in encoded
or sharded Notion carrier pages.

## After Changes Required

Continue the same Task. Address the findings, preserve still-valid evidence, and submit a new exact revision/version. Do not create a replacement Task unless the Brain changes the scope.
