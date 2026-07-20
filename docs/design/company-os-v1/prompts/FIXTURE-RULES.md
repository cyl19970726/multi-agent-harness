# Company OS V1 prompt fixture rules

Every prompt in this directory must be used together with:

- `../fixtures/company-os-trademark-v1.json`
- `../fixture-contract.md`
- the prompt's matching `page_slices.<page-key>` entry

These three sources are the complete authority for visible business facts. The
fixture wins over layout copy, visual convention, prior screenshots, and any
request to make a page feel richer.

## Non-invention contract

- Render the page slice's `required_facts` and resolve every visible actor,
  record, amount, status, relation, and timestamp from the fixed fixture.
- Supporting records may appear only when they exist in the fixed fixture and
  are relevant to the page slice. Do not create plausible companion records.
- Do not invent agenda items, WorkItems, approvals, decisions, projects,
  budgets, totals, breakdowns, sessions, assignments, capacity, health,
  availability, activity entries, charts, trends, metrics, evidence, document
  counts, page counts, orphan counts, or status counts.
- The only metric that may be shown is `metric-july-spend`, and only on the
  Finance page whose slice requires it.
- The only financial record is the `¥3,000` pending-approval commitment. It is
  not a payment. Never show paid, settled, receipt, cash outflow, authorized
  payment, or settlement evidence.
- Keep requester, submitter, assignee, accountable owner, contributor,
  reviewer, legal reviewer, and approver distinct exactly as recorded.
- In approval summaries, keep approval status, requester, and required approver
  as separate visible fields. For this fixture they are `Requested`,
  `Trademark Agent · Standing Agent`, and `Brand Owner · Human`.
- Show organization status only when it appears in
  `organization.explicitly_reported_statuses`: Trademark Agent is `proposed`;
  Document Architecture Agent is `available`. Do not infer any other presence,
  capacity, health, availability, or workload state.
- When the proposed trademark role is shown, preserve both actor type and role
  state: `Trademark Agent · Standing Agent · Proposed`.
- Do not show provider thinking, runtime idleness, chat recency, model/provider
  telemetry, MemberRun ownership, or fabricated activity as business truth.
- If the fixture does not contain enough data for a panel, leave intentional
  whitespace, omit the panel, or use a restrained empty state. Never enrich the
  page by inventing a record, number, state, or time.

## Time contract

- Do not show a date unless the individual prompt explicitly permits it.
- When a prompt permits a date, copy the exact timestamp or month from the
  referenced fixture object. Never move an event earlier than its `created_at`.
- All visible dates must be in July 2026, Asia/Shanghai.

## Shared visual shell

Generate a 1536x1024 desktop product design. Use a warm light-gray application
shell, white content surfaces, fine neutral borders, charcoal text, restrained
coral-red selection and primary-action accents, compact enterprise density,
and readable English labels. Keep the visual language calm, polished, and
Notion-like without becoming a generic file manager.

Primary navigation is `Home`, `Docs`, `Organization`. Grouped secondary
navigation is exactly:

- `OPERATIONS`: Work, Approvals, Finance
- `EXECUTION`: Missions, Workflows, Agent Teams
- `PLATFORM`: Providers, Plugins, Settings

No dark mode, decorative illustration, giant analytics wall, raw provider
logs, or persisted thinking.
