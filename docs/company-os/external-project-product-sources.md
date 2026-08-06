# External Project Product Sources

```text
status: target product contract
owner_role: product-architecture
canonical_for: mapping external software repositories and GitHub activity into Company OS Docs, Work, and delivery evidence
```

## Purpose

Company OS can operate real companies whose software product truth lives in
Git repositories. A repository may contain PRDs, ADRs, acceptance matrices,
frontend design contracts, API schemas, and implementation code. Those files
are valuable product inputs, but they are not the whole company.

This contract defines how an external repository such as
`cyl19970726/wanchengwanling` connects to Company OS:

- the repository remains the source of truth for software product contracts,
  implementation contracts, and delivery evidence;
- Company OS Docs remain the source of truth for company memory, operating
  structure, commercial model, merchant operations, procurement, content
  operations, creator relations, finance context, and cross-functional
  decisions;
- WorkItems link the two through explicit source, assignment, GitHub delivery
  references, evidence, and result updates; and
- GitHub webhooks or polling may create sync events and review queues, but
  they do not silently overwrite Company OS knowledge.

This is the first AgentOS connector priority. The GitHub connector should
initially be a sync/projection connector, not a new command surface. Agents can
already use `gh` and normal Git commands for issue, branch, pull request, and
review operations. The Company OS gap is to synchronize issue/PR/check/source
facts into typed records, delivery refs, WorkItem links, and views so Docs,
Work, Organization, and Agent detail pages can see the same delivery state.
Dedicated MCP tools or plugin-owned CLI commands are optional later, only when
they reduce variance or add a governed operation that `gh` cannot safely cover.

ADR 0042 separates the long-term identities involved here:

```text
Company Store       Execution Space       Project Binding
```

This document describes the Company Store side of external software source
mapping. A Git repository is a Project Binding and software source; it is not
the owner of company memory, WorkItems, Organization, or Finance.

## Ownership boundary

| Truth | Owning system | Examples |
| --- | --- | --- |
| Software product contract | External Git repo | mini-program PRDs, API contracts, ADRs, acceptance scenarios, design snapshots |
| Software delivery evidence | GitHub / Git repo | issue, branch, PR, commit, CI, release tag, device signoff artifact |
| Company business model | Company OS Docs | revenue model, merchant strategy, prize policy, launch plan, content strategy |
| Operational commitments | Company OS Work | merchant onboarding tasks, procurement tasks, AR asset signoff, content calendar work |
| Actor authority | Company OS Organization | Lead Agent, Product Agent, Merchant Ops Agent, human owner, external shop contact |
| Monetary effects | Company OS Finance | purchase commitments, invoices, payments, refunds, sponsored content fees |

The repository is not demoted to an attachment. Its files may remain canonical
for the software product. Company OS holds a mapped projection and operating
context around those files so Agents can plan, assign, verify, and report work
without copying scattered facts by hand.

## Native mapping objects

The following objects are the target Company OS records for repository
integration. Implementation may start with append-only Store rows and later add
SQL read/index projections.

| Object | Purpose |
| --- | --- |
| `ExternalProject` | A durable external software-source registration: owner/repo, default branch, GitHub URLs, responsible Company OS module, and sync policy. In the ADR 0042 target model this relates to a Project Binding instead of owning Store routing. |
| `ProductDocSource` | A declared document source inside that project, such as `docs/prd/**/*.md`, `docs/architecture/**/*.md`, or `docs/frontend-design-os/**`. |
| `ProductDocSnapshot` | A versioned observation of a source file at a commit: path, title, classification, hash, commit, extracted headings, declared business line, and links to owning Company OS records. |
| `ProductDocMapping` | A governed relation between an external source file and a Company OS `Document`, `BusinessModule`, `WorkType`, `Milestone`, or custom page. |
| `SourceChangeEvent` | A webhook or polling observation: push, PR open/update/merge, file changed, ADR added, acceptance file changed, or deleted source. |
| `SourceSyncRun` | The result of a sync attempt: input event, fetched commit, changed paths, created/updated snapshots, warnings, and required human/Agent review. |
| `DeliveryRef` | A WorkItem link to GitHub issue, PR, commit, CI run, release, preview, or device signoff artifact. |

These records do not replace the underlying repository. They make it possible
for Company OS to ask: *Which company module does this PRD affect? Which
WorkItems were created from it? Which PR closed the work? Which docs or
commercial assumptions need review after the software contract changed?*

## Sync model

Company OS should support two sync modes.

### 1. Manual or scheduled pull

An Agent, scheduled job, or GitHub connector runs a read-only sync against a
registered repo and branch. The first implementation may call `git` and `gh`
under the hood rather than adding new GitHub-specific Firm CLI commands:

```bash
firm --project <current-compat-project-selector> \
  company docs source sync \
  --definition <software-source-page-definition-id> \
  --module <software-product-sources-module-id> \
  --source-document <software-product-sources-document-id> \
  --actor <docs-governance-agent-or-human-id> \
  --repo-path <local-git-worktree> \
  --repo <owner/repo> \
  --branch <branch> \
  --project-id <external-software-project-id> \
  --path <prd-or-design-path> \
  --path <additional-path>
```

The top-level `--company` selects Company Store truth and the top-level
`--project` independently selects the local Project Binding used to read the
source worktree. The command-level `--project-id` identifies the external
software source being observed; it is not a Store selector. The command reads
a local Git worktree, records repo metadata and file snapshots, and writes
native Docs `TypedRecord`s:
`external_project`, `product_doc_source`, `product_doc_snapshot`, and
`source_sync_run`.

The current implemented command writes Docs records only. Creating a review
WorkItem for material drift is the target policy path, not an automatic
side-effect of the sync command. A sync must not change commercial policy,
operating plans, Organization authority, Finance state, or delivery status
without an explicit governed Action.

### 2. GitHub webhook / connector event

GitHub can send `push`, `pull_request`, `issues`, `check_suite`, and release
events to a Company OS endpoint or connector worker. The webhook path should be
deliberately small:

```text
GitHub webhook
  -> verify signature and repo registration
  -> append SourceChangeEvent
  -> enqueue SourceSyncRun
  -> update ProductDocSnapshot projection
  -> create or update review WorkItems when policy requires
  -> link merged PRs and CI results to existing WorkItems when correlation exists
```

Webhook delivery is a notification and evidence mechanism. It is not a
permission to mutate business records, approve spending, publish content,
submit legal filings, or grant Organization authority.

The connector's views should render GitHub facts where they help operation:
Development WorkItem delivery panel, PR/check/review table, Docs source mapping
panel, stale-source queue, and Development Agent detail panel. Each view reads
Company OS records and delivery refs; it does not use GitHub state to infer
Company OS completion or acceptance.

## Mapping policy

Every mapped source must declare or infer a stable role:

| Repo source class | Company OS mapping |
| --- | --- |
| PRD / business-line contract | `BusinessModule`, `Document`, `WorkType`, acceptance source |
| Architecture / ADR | `Document`, architecture decision, affected module relation |
| Frontend design contract | custom page visual contract, UI acceptance evidence |
| Acceptance scenario | Work acceptance checklist, DeliveryRef requirement |
| API schema or shared domain model | implementation reference for WorkItem or module capability |
| Ops/runbook | operating procedure document; may create Work templates |

If a file cannot be safely mapped, the sync must surface it as `unmapped` and
create a Docs Governance review queue item rather than guessing. Deletions are
also reviewed: an external repo may delete outdated docs, but Company OS must
decide whether a mapped company decision, commercial record, or operational
template remains valid.

## GitHub and WorkItem linkage

Software work remains a Company OS `WorkItem` when it is a company commitment.
GitHub objects are delivery references, not the task source of truth.

```text
Company OS WorkItem
  source: ProductDocSnapshot / Company OS Document
  accountable_owner: Product or Engineering owner
  assignee: Development Agent / human developer
  execution_ref: Mission/Wave, Agent Team, Workflow, Host, or direct human work
  delivery_refs:
    - GitHub issue
    - branch
    - pull request
    - commit
    - CI/check run
    - preview/device signoff artifact
  result:
    - accepted implementation evidence
    - updated source document mapping
    - product decision or follow-up WorkItems
```

Opening a GitHub issue may create or link a WorkItem when correlation is
explicit. Merging a PR may complete software delivery evidence, but it does not
automatically complete the Company OS WorkItem unless the required acceptance
criteria, review, and result update are present. Selecting the repository as a
Project Binding for execution never reroutes Company Store writes by itself.

## What belongs outside the external repo

The external software repository should not become the full operating database
for the company. The following should be Company OS Docs/Work/Finance records
first, with only necessary outputs synced into the software product:

- commercial model, pricing strategy, merchant economics, sponsorship model;
- merchant lead pipeline, contact logs, onboarding checklist, and go-live
  readiness;
- prize procurement, supplier records, purchase orders, invoices, payments,
  logistics, receipt, quality check, and allocation decisions;
- content calendar, channel operations, published posts, metrics, and
  retrospective decisions;
- creator/KOL outreach, contract terms, deliverables, payments, evidence, and
  performance metrics;
- launch readiness and field operations across software, AR content, merchants,
  inventory, staff, content, and risk.

The software repo receives only the product-facing or implementation-facing
effects: shop records, reward definitions, inventory allocations, media
manifests, API changes, mini-program UI changes, acceptance evidence, and
release notes.

## Governance rules

1. **Map before automating.** A webhook may observe unknown files, but durable
   auto-actions require registered `ProductDocSource` and `ProductDocMapping`.
2. **Snapshots are observations.** A `ProductDocSnapshot` records what the repo
   said at a commit; it does not by itself change Company OS commercial truth.
3. **Review material drift.** New business lines, changed acceptance criteria,
   deleted PRDs, finance-impacting product changes, privacy/security changes,
   and external-facing launch commitments create review WorkItems.
4. **Preserve provenance.** Every derived Company OS update links the source
   repo, path, commit, event, sync run, and Actor or automation that performed
   the mapping.
5. **Keep money and authority native.** GitHub labels, comments, or merged PRs
   cannot approve payments, grant permissions, or add Standing Agents.
6. **Use GitHub for delivery.** Code changes, software PRDs, implementation
   evidence, CI, reviews, and releases stay in GitHub and are linked from
   WorkItems.

## Wanchengwanling initial mapping

The first target external project is:

```text
ExternalProject:
  id: wanchengwanling
  repo: github.com/cyl19970726/wanchengwanling
  branch: dev
  target_company_store: <agent-company-id>
  target_operating_area: wanchengwanling
  project_binding: wanchengwanling
  source classes:
    - README.md                  -> external project overview
    - IMPLEMENTATION_PLAN.md     -> implementation planning source
    - docs/frontend-design-os/** -> visual and UX evidence
    - specs/**                   -> feature requirements/design/tasks
    - tools/mp-agent-os/**       -> mini-program acceptance harness
    - infra/nfc-gateway/README.md -> NFC gateway reference
    - reports/live-prd/README.md -> live PRD report contract
```

The Company OS side should create or map at least these modules:

- Wanchengwanling Product & Software Delivery;
- AR Field Rollout and Asset Acceptance;
- Merchant Onboarding;
- Prize Procurement and Logistics;
- Content Operations;
- Creator / Blogger Outreach;
- Launch Readiness.

The project-specific commercial model, merchant plan, procurement plan,
content strategy, creator strategy, and launch dashboard belong in Company OS
Docs. The repo PRDs remain mapped software product sources.

The current Wanchengwanling local Store is compatibility state from the
repo-derived `ProjectContext` implementation:

```text
Company OS project id: new-day-wanchengwanling
project_root: /Users/hhh0x/new-day/wanchengwanling
store_root: /Users/hhh0x/.firm/projects/new-day-wanchengwanling
external software project id: wanchengwanling
```

This keeps the distinction explicit in the current implementation but is not
the final Store boundary. The ADR 0042 target is one Agent Company Workspace
that contains both Wanchengwanling and AgentOS operating areas while mapping
`cyl19970726/wanchengwanling` and `cyl19970726/multi-agent-harness` as separate
Project Bindings / external sources. Company OS owns the commercial operating
memory; Git owns software source files and delivery evidence; source sync
creates linked observations rather than moving either truth into the other.
