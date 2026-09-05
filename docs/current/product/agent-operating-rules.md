# Agent Operating Rules — Detailed Companion

```text
status: canonical operating detail
owner_role: lead-operations
canonical_for: implementation-bound remainder of the AGENTS.md relocation (CLI commands, gate-grepped phrases, execution-space selectors)
```

## Authority

Product doctrine for this topic — agent operating rules, execution-object
semantics, lifecycle policy, self-hosting method, and the AGENTS.md
relocation map — is canonical in Notion; see the single authority-boundary anchor in
`docs/current/documentation-governance.md` (Authority boundary: Notion vs
repository) for the current Notion location.
This repository file survives only as the implementation-bound remainder
below. Root [AGENTS.md](../../../AGENTS.md) states repository-wide hard
invariants and routes to this file; this file owns only its declared
implementation-detail scope. On an apparent conflict, select the owner of the
exact question and repair the other projection. Neither file universally wins.

## Implementation-bound invariants

- The phrases `provider's native`, `streams into Harness ledgers`, and
  `Resume must use the provider-native session id` must stay in root
  `AGENTS.md` — `scripts/check-native-session-boundary.mjs` greps for them.
- Useful local commands:

  ```bash
  target/debug/firm init
  target/debug/firm node init
  target/debug/firm team create --name <team> --description <purpose> \
    --host-agent-id <agent-member-id> \
    --node-id <node-uuid> --member <agent-member-id>
  target/debug/firm team-run create --agent-team-id <team> --objective <objective>
  target/debug/firm team-run work create --team-run-id <team-run> \
    --title <title> --context <markdown> \
    --completion-criteria <criteria> --claim-mode host_assign
firm team-run work assign --work-id <work-id> --expected-version <version> \
    --membership-id <team-membership-id>
  target/debug/firm team-run work list --team-run-id <team-run>
  target/debug/firm dashboard snapshot
  target/debug/firm serve --addr 127.0.0.1:8787
  npx pnpm@9.15.4 acceptance:legacy-retirement
  ```

- Execution Space / Project Binding selectors: `--space <id>` /
  `HARNESS_SPACE` / `firm space switch`; `--project <id|path>` /
  `HARNESS_PROJECT` / `firm project switch`; `--store` / `HARNESS_ROOT` are
  deprecation-warned back-compat overrides. `AgentTeamRun.project_binding_id`
  pins the execution resource once set and later selector changes must not
  retarget it. Historical Workflow binding fields remain archive evidence only.
  The reserved GLOBAL
  `_global` (`~/`) project is non-git and rejects
  `writable`/`isolation="worktree"` nodes with an actionable message.

## Repository execution rules (relocated from AGENTS.md)

- Creating, assigning, executing, reviewing, retrying, blocking, or completing
  repository development follows the single
  [.agents/skills/agentfirm-development-loop/SKILL.md](../../../.agents/skills/agentfirm-development-loop/SKILL.md).
  That Skill owns the two-table workflow, exact readable submission policy,
  Session reconciliation, machine-manifest placement, statuses, and messages;
  this companion does not duplicate them.
- Ordinary repository repair uses a Primary Session. A native Agent Team run
  is required only when the runtime itself is the claim under test or an
  accepted Spec selects that scenario. Bounded temporary Sub-Agents may be
  used internally, but must not be labeled Harness Member or Agent Team
  execution. Product TeamWork acceptance remains separate from developer
  self-review. A native run is product evidence, not a prerequisite that
  silently replaces the repository development record. A small typo or
  single-line doc fix may be owner-local, but the final summary must identify
  the proportional exception.
- Harness dogfood runs follow
  [docs/current/product/agent-team-dogfood-loop.md](agent-team-dogfood-loop.md):
  dogfood is Spec-level acceptance, not a development mode. A finding required
  to keep the run executable and its evidence trustworthy may receive the
  narrowest hot-fix in the same acceptance Task. Other findings enter the Issue
  Pool without stopping the run; Brain later batches, defers, or only records
  them. Never weaken a scenario or edit store evidence to make a run appear
  green.
- Scenario execution rosters and research budgets (for example the current
  dogfood roster) are scenario policy, not repository-wide invariants: they
  live in [docs/current/operations/operations.md](../../../docs/current/operations/operations.md) and the owning scenario
  skill (`dogfood-company-os`, archived with the DOC-108 cutover; its source
  lives only in git history per ADR 0063)
  and must not be broadened into root instructions.
- Prefer the progression `doc -> skill -> schema -> CLI/API -> dashboard ->
  plugin`. The Agent Dashboard is the operator view for harness state; product
  dashboards for adapted projects remain separate.
- Local gates and commands: [docs/current/operations/operations.md](../../../docs/current/operations/operations.md).
  `acceptance:legacy-retirement` (formerly `acceptance:mission-wave`) proves
  the deterministic current Agent Team, CLI, Kimi ACP adapter, and Dashboard
  contracts plus the retired Mission/Wave legacy read and retired-write
  behavior; a real-provider claim still requires a separately recorded native
  live run.

## Proportional acceptance (relocated from AGENTS.md)

Every non-trivial Team-run Work slice advances in four small stages:
**Context** (Work intent, current Host judgment, permissions, risk, and
decision boundary are clear), **Execution** (the selected Host or Team owns
its internal plan and emits honest native records), **Outcome**
(explicit Work submissions, checks, artifacts, blockers, and review results
are recorded), and **Advance** (the Host accepts or requests changes on the
Work record and starts the next slice; unrelated active Works may carry
forward unchanged). Review depth is proportional to risk;
Proposal/Decision/outcome evaluation is not a universal product chain.

Each MemberRun snapshots its concrete `ProviderIntegrationProfile`; platform
capability, execution-mode capability, adapter coverage, and product permission
are separate claims. Provider-native questions that actually pause a turn are
`provider_interaction_request` Messages and their answers are correlated
`provider_interaction_response` Messages. Permissions are
frozen on AgentSession start: in-ceiling operations proceed directly and
out-of-ceiling operations fail closed without a second permission object.
Ordinary Host/Member planning remains correlated identity-first Message conversation. A provider
`completed` status is not by itself proof of semantic success, answer, or
approval.

The current trusted-development Team policy gives Codex, Claude, and Kimi
members full execution access so unattended work is not blocked by ordinary
tool authorization. This is a product policy, not a Provider capability and not
approval for protected external effects. Members decide when isolation is
useful and may create their own same-repository Git worktree; the Host declares
owned/conflicting paths and acceptance boundaries, not Git mechanics. Members
must report the actual worktree, branch, commit, checks, and conflicts.

New Agent Team members use only their persistent bidirectional mode:
`codex_app_server`, `kimi_acp`, or `claude_agent_sdk`. Bounded
`codex_exec`/`claude_cli` paths describe retired Dynamic Workflow and historical
records only; they are not current routes or Team fallbacks. The one declared exception is
`external_interactive`: a user's own already-open interactive provider CLI
session may join a run as a non-driven member that Harness never spawns or
drives — it polls its inbox and replies over the trusted loopback CLI,
and it has no provider-native session record (evidence claims about its work
cannot resolve to one). The Host explicitly creates, messages,
inspects, interrupts, closes, reopens, and retires members. Interrupt stops one
current turn. Close releases the managed runtime and freezes the mailbox while
retaining the same MemberRun and provider-native session; Reopen increments its
runtime generation and resumes that exact session. Deactivate/Retire is the
permanent coordination end. TeamRun completion never implies Close.
Physical live control handles remain process-local to the Harness
service that started them. A durable Team Supervisor lease is the cross-process
control authority and contains a loopback service locator. Dashboard, CLI, and
HTTP clients route controls to that owner; the owner revalidates supervisor id,
generation, status, and expiry immediately before driving its handle. After a
crash, a new Supervisor generation reattaches the recorded native sessions;
uncertain claimed deliveries require explicit reconciliation, never blind
replay.

Harness has no Plan Mode or Plan Gate. When the Host wants a plan first, it asks
through an ordinary correlated Markdown message; the Member replies, the Host
argues or approves in the same chain, and provider-native plan/goal features
remain internal execution aids.

Work is durable responsibility; a provider-native Goal is only one
possible continuation mechanism for executing it. Each active MemberRun/native
session must have exactly one top-level execution driver:
either Harness starts the next provider cycle (`host_driven`) or an observed
provider-native continuation loop does (`provider_driven`). Never activate a
native goal and also issue an ordinary Harness start for the same work. A
separate Session may share the same cwd; worktree isolation is optional.
provider-driven member may complete many native cycles without creating a new
MemberRun, but provider satisfaction never implies Host acceptance. Providers
without a reviewed native continuation capability remain first-class
host-driven members. See `docs/current/architecture/member-continuation-model.md` and ADR 0041.

Provider-native or chat-side subagents are implementation details of the Host
or member that invoked them. Optional hooks may record honest attribution, but
the harness must not claim lifecycle control it does not have.

Do not claim that Agent Team work was accepted unless the store shows the
durable `AgentTeam`, the `AgentTeamRun` records, role-specific MemberRuns with
owned or claimed Works, versioned WorkEvents and delivery facts, Work-linked
messages where conversation occurred, explicit submitted results and Host
acceptance on the Work record — with execution claims resolvable to the
provider-native session.

Sensitive actions (authentication, payment, license acceptance, permission
changes, organization changes) still require the appropriate Human or policy
approval under Hard Invariant 6; no Work or Message record replaces that
approval.

A Team-run Work slice is done only when the store can explain why the work
existed, which teams/runs and Works were used, which WorkEvents allocated,
blocked, submitted, or accepted responsibility, which Messages explained
coordination, which outcomes/checks/artifacts and provider-native sessions
support acceptance, and what should be reused, improved, split, or followed up
next. If a future agent cannot reconstruct the answer from repository files
and native harness state, the work is not fully accepted.
