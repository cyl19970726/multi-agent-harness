# Dev contract

Dev owns implementation and decides when work is ready to review.

## Work

- Follow the Task goal and acceptance criteria.
- Iterate freely without Candidate or readiness approval.
- Keep Task blocker and useful progress evidence understandable to the Brain.
- Send `ATTENTION_REQUIRED` only for a real blocker or missing decision.

## Submit

Send `READY_FOR_REVIEW` with:

- Task ID
- exact code SHA or named immutable document version
- concise change summary
- checks performed and results
- evidence links
- known limitations

Do not continue changing the submitted revision and still call it the same submission. New changes produce the next submission of the same Task.

## After Changes Required

Continue the same Task. Address the findings, preserve still-valid evidence, and submit a new exact revision/version. Do not create a replacement Task unless the Brain changes the scope.
