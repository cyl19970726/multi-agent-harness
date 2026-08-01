# AgentOS Wave 6 Work Queue

```text
status: canonical operating queue for Wave 6
owner_role: AgentOS Work Governance
canonical_for: AgentOS WorkItem classification, priority, lane ownership, and collision boundaries
source: agent-company Company Store work list + master commit 869a870, inspected 2026-08-01
```

This queue classifies every AgentOS WorkItem in the `agent-company` Company
Store exactly once: **completed** (already closed in the Store; listed only
for accounting), **reconcile** (already covered on master; verify and close,
no new implementation), **duplicate-obsolete** (superseded by another
WorkItem), **genuine next** (ranked implementation/governance lanes), or
**blocked** (external gate with an exact resume condition).

Exact-once accounting: 25 AgentOS WorkItems = 1 completed + 13 reconcile +
2 duplicate-obsolete + 7 genuine next + 2 blocked.

It preserves current-vs-target truth: an item is "covered" only when the
merged master commit, the Store projection, or the installed runtime proves
it. Design contracts that are documented but not implemented stay target-only
and remain in the genuine-next queue.

Inspection commands (read-only, re-runnable):

```bash
harness company current
harness company work list
harness company docs query --document document-agentos-root
harness company docs health
git log --oneline origin/master
gh pr list --repo cyl19970726/multi-agent-harness --state open
```

Execution roster for all lanes below (from
`skills/dogfood-company-os/SKILL.md` and `docs/operations.md`): Kimi
`kimi_acp` with the reviewed K3 model alias at `max` thinking effort is
primary; Claude `claude_agent_sdk` joins only while its installed SDK passes
`harness member providers --fail-on-review`; no Codex execution members.
After each lane merges to master, remaining live lanes go through rolling
Supervisor reconciliation (rebase worktree, resume MemberRun/native session
when compatible, never two generations in one Workspace).

## A. Completed In Store — No Action (1)

Already closed in the Company Store; listed only so the exact-once accounting
covers it. No reconciliation needed.

| WorkItem | Store state | Evidence |
| --- | --- | --- |
| work-agentos-social-content-gateway-v0 | completed | `github-pr-268` (merged `dac9c9d` social content gateway readiness) |

## B. Covered On Master — Reconcile And Close (13)

No new implementation. The owner verifies the cited evidence against the
WorkItem's stored acceptance criteria, attaches the evidence refs, and closes
the WorkItem through `harness company work transition|close`. If a criterion
is not actually met, the item drops back to genuine next with the failing
criterion named.

| WorkItem | Covering evidence (verified) | Reconcile owner |
| --- | --- | --- |
| work-agentos-kimi-mid-turn-delivery-v1 | PR #276 (`80f9bfb`, `d58b596`), #278 (`98afe40`), #292 (`682d4c3`): queued/claimed/delivered/acknowledged states, ordered exactly-once next-round delivery, stale-handoff fencing (`docs/integration/kimi-agent-team.md`) | agent-agentos-platform-development |
| work-agentos-provider-execution-controls-v1 | Requested-vs-effective `provider_controls` on MemberRun (`schemas/member-run.schema.json`, provider_controls); Kimi `session/set_config_option` with effective receipt (`crates/harness-cli/src/kimi_acp.rs`); Codex/Claude mappings; #296 Claude `provider_error` rounds | agent-agentos-platform-development |
| work-agentos-generic-workitem-focus-v1 | PR #279 (`ad91e7e`; `38580de` project Work execution provenance; `73ba38d` complete Work provenance chain); item is `in_review` with deliverable commit `73ba38de` already attached | agent-agentos-platform-development |
| work-agentos-workitem-detail-fields-v1 | `28003e9` feat(company-os): WorkItem detail fields and AgentOS docs flow | agent-agentos-platform-development |
| work-agentos-store-docs-foundation-v1 | Store-verified: `document-agentos-root` in space `agentos` with 6 children; `module-agentos-project-home` and `module-agentos-software-product-sources` active; `product-doc-source-agentos-*` and `source-sync-run-1785334715161-p59475-0` records exist | agent-agentos-docs-governance |
| work-agentos-doc-space-cleanup-v1 | Store-verified: all 4 legacy `company`-space AgentOS documents are `archived`; canonical docs live under `document-agentos-root` in space `agentos` | agent-agentos-docs-governance |
| work-agentos-org-agent-split-v1 | Store-verified: AgentOS Standing Agents exist (`agent-agentos-lead`, `-docs-governance`, `-work-governance`, `-org-governance`, `-platform-development`) with explicit AgentMember execution links | agent-agentos-lead |
| work-agentos-org-work-doc-loop-v1 | `50763b9`/`c4b0f73` Standing Agent execution link; `38580de`/`73ba38d` assignment→TeamMessage→MemberRun→native-session provenance chain; live evidence already attached to the item | agent-agentos-work-governance |
| work-agentos-organization-operability-v1 | PR #291 (`e2736e1`): Org hierarchy from Store truth, execution binding without MemberRun/identity conflation, ambiguous identity withheld (`apps/agent-dashboard/src/company-os/operations/pages.tsx`) | agent-agentos-org-governance |
| work-agentos-external-gateway-registry-v1 | PR #273 (`e9de920`): gateway registry contract in `docs/company-os/external-gateway-and-plugins.md` | agent-agentos-platform-development |
| work-agentos-wecom-gateway-plugin-v0 | PR #273 (`e9de920`): WeCom v0 design contract documented; implementation is a separate blocked item (see section E) | agent-agentos-platform-development |
| work-agentos-workitem-reassignment-action-v1 | `b033965` governed WorkItem update command (`harness company work update`) | agent-agentos-work-governance |
| work-agentos-org-role-permission-closure-v1 | `company.work.execute` enforcement with actionable denial (`crates/harness-cli/src/company_os_api.rs`); `harness company org update-permissions` CLI; Org governor self-transition proven live (see `docs/company-os/agentos-self-hosting-loop.md` "Current implementation truth") | agent-agentos-org-governance |

Residual target-only gap inside the last item: a governed module/policy-scoped
grant lineage with revocation and durable denial evidence remains design-only
(`docs/company-os/scoped-authority-broker.md`). The reconcile action closes
the implemented enforcement criteria and must name this residual in the close
outcome; it becomes genuine-next scope only when the scoped-authority broker
contract is accepted for implementation.

## C. Duplicate-Obsolete (2)

Close as superseded with a relation to the successor; do not re-implement.

| WorkItem | Successor | Why |
| --- | --- | --- |
| work-agentos-workitem-detail-contract | work-agentos-workitem-detail-fields-v1 | Same scope (description, acceptance, context/deliverable refs); v1 carries the precise acceptance and shipped in `28003e9` |
| work-agentos-store-docs-foundation | work-agentos-store-docs-foundation-v1 | v0's home/intake/gateway docs are the archived legacy set; v1's root/modules/source-sync are Store-verified |

## D. Genuine Next — Ranked Queue (7)

Lane rules for every implementation item: one same-repository worktree off
latest master per lane; the member reports worktree path, branch, commit,
checks, and conflicts; owned paths below are the product boundary; after the
lane merges, other live lanes reconcile per the rolling rule.

### N1. work-agentos-team-message-convergence-v1

- Owner: agent-agentos-platform-development
- Status: implemented — `TeamMessage.response_intent` plus the sender-aware
  `effective_response_intent` default (`crates/harness-core/src/lib.rs`), the
  response-intent delivery/fence gate (`crates/harness-store/src/lib.rs`,
  `crates/harness-cli/src/main.rs`), `--response-required`/`--informational` on
  the CLI, `response_intent` on HTTP/MCP, and the Dashboard read-side label.
  ADR 0046 §4 now states the sender-aware rule and is no longer design-only
- Dependencies: none
- Collision boundary: `crates/harness-core/src/lib.rs` (TeamMessage),
  `crates/harness-cli/src/main.rs` (team-run send/gateway), `crates/harness-store`,
  message schemas, `apps/agent-dashboard/src/surfaces/TeamWarRoom.tsx`.
  Sequence before N2 (both touch `harness-core` and schemas)
- Acceptance: ack-only peer mail triggers no provider round unless
  `response_required` is explicit; Host/Operator/Service mail still wakes an
  idle Member by default so questions, blockers, reviews, Host decisions, and
  handoffs stay durable, correlated, and reachable; deterministic two-peer
  bounded-convergence test; Dashboard distinguishes informational delivery from
  response-required

### N2. work-agentos-runtime-upgrade-reconciliation-v1

- Owner: agent-agentos-platform-development
- Status: submitted; partial — per-MemberRun `ProviderIntegrationProfile`
  (provider/adapter versions, `current|review_required|incompatible`) exists;
  missing: Harness build fingerprint, `restart_required` state, automatic
  restart-required reconciliation
- Dependencies: provider-controls substrate (B-list, covered); N1 merged first
  to avoid `harness-core`/schema conflicts
- Collision boundary: `crates/harness-core` MemberRun/profile, `crates/harness-cli`
  supervisor + `member providers`, `schemas/member-run.schema.json`,
  `apps/agent-dashboard/src/surfaces/MemberRuns.tsx`
- Acceptance: per the stored criteria — build/adapter/config fingerprints on
  every live runtime; CLI+Dashboard distinguish
  current/restart_required/review_required/disconnected/incompatible with an
  actionable reason; contract-changing upgrades create an explicit
  replacement generation preserving Standing Agent, WorkItem, Assignment
  correlation, and compatible native sessions

### N3. work-agentos-archived-source-provenance-v1

- Owner: agent-agentos-docs-governance (projection contract) +
  agent-agentos-platform-development (implementation)
- Status: submitted; partial — archived badge exists on the Docs page
  (`BasicDocumentPage.tsx`); the Work page resolves source title but shows no
  archived badge/history navigation; Docs health does not yet report active
  Work referencing archived sources
- Dependencies: none; pairs with N5 (share the Docs audit)
- Collision boundary: `crates/harness-cli/src/company_os_api.rs`,
  `apps/agent-dashboard/src/company-os/work/` and `.../docs/`; serialize with
  N5 (same dashboard areas)
- Acceptance: every visible WorkItem source resolves to a navigable active
  Document or an explicit archived-source history surface; Docs health
  reports the class with deterministic projection tests; no ledger edits

### N4. work-agentos-dashboard-snapshot-budget-v1

- Owner: agent-agentos-platform-development
- Status: submitted; not implemented — scoped TeamRun route still
  materializes the full global snapshot server-side (open issue #264,
  confirmed in `crates/harness-cli/src/main.rs` dashboard snapshot path)
- Dependencies: none
- Collision boundary: `main.rs` snapshot/read-model paths, dashboard types;
  same file as N1's gateway code — sequence or rebase with explicit Host
  boundary
- Acceptance: scoped startup projections per Company/surface; deterministic
  payload-byte and latency benchmark in checks; honest loading/partial
  states; canonical ledgers unchanged and read models rebuildable

### N5. work-agentos-docs-information-architecture-v1

- Owner: agent-agentos-docs-governance (audit + proposed canonical structure
  first), then agent-agentos-platform-development (projection only)
- Status: submitted; partial — archived documents are already filtered from
  the default tree (`fixtureAdapter.ts`); missing: explicit archive
  view/filter and the IA audit record
- Dependencies: N3 recommended first (same Docs surfaces)
- Collision boundary: `apps/agent-dashboard/src/company-os/docs/` only;
  dashboard lanes serialize on the committed build artifact
  `apps/agent-dashboard/web/index.html`
- Acceptance: default tree shows the active operating hierarchy; archived
  material behind an explicit filter; navigation derived from canonical
  relations; empty/error/long-document states verified; audit returned
  before projection edits

### N6. work-wcw-agentos-work-overview-ui

- Owner: agent-wcw-development
- Status: submitted; partial — the Work surface already defaults to its
  overview tab (`WorkOperatingPage.tsx`), but the app-wide landing is still
  the home surface (`apps/agent-dashboard/src/app/selection.ts`)
- Dependencies: none
- Collision boundary: dashboard company-os router/selection files only
- Acceptance: Work overview is the default Company OS operating board for the
  wanchengwanling and agentos areas, with deep links preserving Company
  Store, Execution Space, and Project Binding context

### N7. work-agentos-docs-work-org-autonomy-v1 (umbrella, in_progress)

- Owner: agent-agentos-work-governance with agent-agentos-lead
- Status: in_progress; this is the continuous Org+Docs+Work self-hosting loop
  the other lanes feed — keep it open and attach lane results as evidence
- Dependencies: consumes N1–N6 and the B-list reconciliations
- Collision boundary: governance lane — `docs/company-os/**`, Company Store
  records through governed CLI only; no runtime code
- Acceptance: per the stored criteria — truthful Org/Work/Docs projections,
  context-preserving deep links, unavailable actions visibly disabled, and
  multiple real self-hosting cycles reconstructable from native state

## E. Blocked (2)

| WorkItem | Blocker | Exact resume condition |
| --- | --- | --- |
| work-agentos-github-source-binding-v0 (in_progress) | Acceptance requires PR #277 merged by the authorized Human path; #277 is OPEN (`feat(company-os): govern dogfood and reconcile team assignments`). The HARNESS_SPACE set/unset Company Assignment execution-bridge suite is also unimplemented | Human merges PR #277 and the exact merge commit is read back; then the bridge suite becomes a genuine-next slice for agent-agentos-platform-development |
| work-wcw-agentos-wecom-gateway-v0 | WeCom gateway CLI/schema is planned-only (`docs/company-os/external-gateway-and-plugins.md`); requires merchant WeCom credentials and policy approval | Human provides the WeCom app credentials and approves the external-access gate; contract from B-list item work-agentos-wecom-gateway-plugin-v0 then drives implementation |

## Cross-Lane Collision Rules

- `crates/harness-cli/src/main.rs` is a single shared file: N1 (team-run
  gateway) and N4 (snapshot paths) must not hold concurrent edits; sequence
  them or rebase on master before handoff.
- N1 then N2, never in parallel: both touch `crates/harness-core` message and
  MemberRun types plus schemas.
- Dashboard lanes (N3, N5, N6) serialize on the committed build artifact
  `apps/agent-dashboard/web/index.html`; one dashboard lane at a time, or the
  later lane rebases and rebuilds.
- Store mutations happen only through governed CLI/actions by the owning
  Company actor; this queue document never edits the Company Store, and old
  execution JSONL is read-only evidence.
