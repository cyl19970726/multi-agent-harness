---
name: connect-github-company-os
description: Connect GitHub repositories, Issues, pull requests, commits, checks, reviews, comments, releases, and repo documents to Company OS as external source observations and software-delivery evidence. Use when an Agent needs to register or sync a repository, map repo docs into Docs, link GitHub activity to a Work, reconcile delivery state, triage an Issue, or design/operate the GitHub connector without letting GitHub replace Company Docs, Work, Organization, Approval, or Finance truth.
---

# Connect GitHub To Company OS

Use GitHub for software source and delivery. Use Company OS for company
commitments, responsibility, acceptance, authority, and operating memory.

This Skill is a procedural connector, not product authority. It does not
replace `gh`, Git, the Company module operators, or the repository's own
instructions.

## Load The Contract

Read only the relevant canonical sources before changing durable records:

- `docs/current/company-os/external-project-product-sources.md`
- `docs/current/company-os/external-gateway-and-plugins.md`
- `docs/decisions/0042-company-store-execution-space-project-binding.md`
- `docs/current/company-os/work-items-and-approvals.md`

Use `$company-docs-operator` for source records/relations and
`$company-work-operator` for Work links/lifecycle. Use
`$company-org-operator` only when actor identity or permission is in scope.

## Keep Three Identities Distinct

```text
Company Store
  owns Docs, Work, Organization, approvals, finance, and acceptance

Project Binding / Git worktree
  identifies repository source and execution cwd/instructions

GitHub repository
  owns Issues, PRs, commits, checks, reviews, releases, and hosted repo docs
```

Selecting a Project Binding does not reroute Company Store writes. A GitHub
repository is not a Company, Work board, or Organization unit.

## Use Existing Transport First

For the first slice use `gh`, Git, GitHub API polling, or a verified webhook.
Do not build a new MCP server or GitHub-specific Harness command merely to wrap
working GitHub operations.

Add dedicated transport only when it provides a missing governed capability,
stable webhook service, secret isolation, idempotency, or lower operational
variance. The durable Company result must be the same regardless of transport.

## Map Facts, Do Not Mirror GitHub

The connector may project:

| GitHub fact | Company OS use |
| --- | --- |
| repository/default branch | external project / Project Binding relation |
| repo PRD, ADR, schema, design doc | product source/snapshot mapped to Docs |
| Issue | source observation or DeliveryRef linked to a Work |
| PR/commit | implementation deliverable |
| check/workflow run | delivery evidence snapshot |
| review | review evidence; not Company Approval |
| comment | external conversation evidence; not company policy |
| release | delivery evidence; not automatic Work acceptance |

Prefer stable external ids, URLs, repo, number/SHA, observed state, observed
time, sync cursor, and freshness. Do not copy complete GitHub history into
Company Store or make a second transcript.

Current local source sync writes Docs `TypedRecord`s such as
`external_project`, `product_doc_source`, `product_doc_snapshot`, and
`source_sync_run`. GitHub Issue/PR/check/review projections and webhook
transport remain partial until their schema/store/API/UI acceptance exists.
Never describe a target record or UI as implemented merely because this Skill
names it.

## Field Ownership

Resolve conflicts by owner:

| Field/decision | Owner |
| --- | --- |
| business context, priority, accountable owner, assignee, acceptance | Company Work/Docs/Org |
| branch, commit, PR state, checks, GitHub reviews, release tag | GitHub |
| Work completion and result return | Company Work |
| actor authority and repository permission policy | Company Organization / Human policy |
| observed external status/freshness | connector projection |

Do not implement naive bidirectional sync. Use reconciliation with explicit
field ownership and surface conflicts for review.

## Read Before Writing

Inspect both sides:

```bash
harness --company <company-id> company docs query --document <source-doc-id>
harness work list --team-id <team-id>
gh repo view <owner/repo> --json nameWithOwner,defaultBranchRef,url
gh issue view <number> --repo <owner/repo> --json number,title,state,url,labels,milestone
gh pr view <number> --repo <owner/repo> \
  --json number,title,state,url,headRefName,baseRefName,mergeCommit,statusCheckRollup,reviews
```

Also inspect the current Project Binding and local Git facts. Never infer a
remote merge from a local branch or a Work acceptance from a green check.

## Run The Connector Loop

### 1. Observe

Fetch the smallest GitHub fact set required for the current Work or source
mapping. Preserve repo identity, external id, URL, revision/SHA, observation
time, and transport.

For repository documents, use the governed Docs source-sync path:

```bash
harness --company <company-id> --project <project-binding> \
  company docs source sync \
  --definition <page-definition-id> \
  --module <software-source-module-id> \
  --source-document <software-source-document-id> \
  --actor <docs-agent-or-human-id> \
  --repo-path <worktree> \
  --repo <owner/repo> \
  --branch <branch> \
  --project-id <external-project-id> \
  --path <path> \
  --dry-run
```

Confirm before dispatch. A sync is Docs-only and must not create Work, Org,
Finance, Approval, or execution side effects.

### 2. Correlate And Route

Reuse explicit relations or Work refs. One Work may link several
Issues, PRs, commits, and checks; one repository may support several Company
modules. Do not correlate by title alone.

Prefer an idempotency identity such as:

```text
github:<owner>/<repo>:issue:<number>
github:<owner>/<repo>:pull:<number>
github:<owner>/<repo>:check:<run-or-check-id>:<sha>
```

If no Work owns a material finding, submit the observation to the
continuous Company intake path through `$company-work-operator`. The Company
Lead decides priority, deduplication, and capacity; the accountable Domain Lead
receives one Work ownership and delivery and may delegate delivery only inside its
attenuated Organization ceiling. The Supervisor may route intake or execution
mail, but it does not choose business priority, create authority, or close
Work.

Route to a Human queue only when policy requires a named Human gate, the GitHub
effect is protected, authority/permission must expand, evidence conflicts
materially, or no bounded actor can proceed. Ordinary read, correlation,
low-risk internal triage, and freshness refresh do not require ceremonial
Human approval.

### 3. Perform Governed Actions

Classify external effects:

| Risk | Examples | Default |
| --- | --- | --- |
| R0 observation | read repo/Issue/PR/checks, calculate freshness | automatic |
| R1 reversible triage | draft reply, add low-risk internal relation, prepare label proposal | Agent may prepare; preserve evidence |
| R2 product commitment | public roadmap promise, close disputed Issue, merge/release | Lead/Human policy gate |
| R3 protected | permissions, branch protection, security disclosure, destructive repo action | explicit Human/Policy gate |

Opening a PR during an explicitly assigned development Work is normal
delivery when repository policy permits it. Merging, release, permission
change, deployment, or destructive cleanup requires the applicable explicit
authority. Never treat a logged-in `gh` session as that authority.

### 4. Reconcile And Return

After GitHub changes:

1. read the remote object again;
2. record the final URL/id/SHA/state and check/review evidence;
3. attach delivery/evidence refs to the Work through Work;
4. let the accountable reviewer decide Company acceptance;
5. return the durable result to the source Document/module; and
6. update connector freshness/sync state.

A merged PR may satisfy a delivery criterion but never silently closes Company
Work. The Company Lead replans from the accepted result, remaining blockers,
and current capacity rather than from GitHub state alone.

Current implemented connector truth is local Docs source sync plus direct
Git/GitHub observation through existing transports. Typed
Issue/PR/check/review projections, durable freshness reconciliation, and exact
Work ownership and delivery → Team delivery → native execution → Handoff → GitHub
observation linkage remain partial/target until their schema, Store, API, UI,
and acceptance checks exist. Never invent those rows from a URL or Skill
procedure.

## Webhook Boundary

A webhook path must:

1. verify signature and registered repository;
2. deduplicate by GitHub delivery/event id;
3. append an immutable source event or equivalent observation;
4. enqueue/project a bounded sync;
5. create or update review Work only under declared policy; and
6. retain failure, retry, cursor, and freshness facts.

Webhook delivery is notification, not authority. It cannot approve money,
change Organization, grant permissions, accept Work, or overwrite company
memory.

## Truthful Projection And UI Acceptance

Connector views are derived projections over GitHub observations and Company
relations. They must expose observation time, freshness, partial/planned
coverage, and source deep links. A cache, webhook, dashboard card, label, or PR
status must never manufacture Company priority, assignment, approval,
acceptance, or completion.

The Company UI should show GitHub where it helps decisions:

- Work: source Issue/finding, PR/commit deliverables, checks/reviews,
  freshness, and deep links;
- Docs: source path/branch/commit snapshots and drift review;
- Agent detail: assigned Issues/PRs/failed checks derived through Work/Org
  relations;
- repository governance: issue inbox, PR queue, CI health, source drift, and
  unresolved correlations.

It must not reproduce GitHub as another full interface or display stale facts
without observation time/freshness.

## Handoff

Report:

- Company, Project Binding, and GitHub repo identities;
- source observations and sync cursor/freshness;
- linked Work, Docs, actor, execution, and delivery/evidence refs;
- Company Lead triage/replan, Domain Lead delegation, and Human exceptions;
- external actions and their authority;
- remote readback and checks;
- conflicts or unmapped facts; and
- implemented, partial, planned, or blocked connector capabilities.
