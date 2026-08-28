# Agent Operating Rules

This repository builds Star Harness itself. Authority is question-scoped:
accepted Notion Docs own current product intent; this checkout's code, schemas,
configuration, and applicable `AGENTS.md` files own executable constraints;
registered repository docs own only their named implementation/reference
scope. Execution claims must additionally be reconstructable from the native
runtime records of the executor used.

This root file is deliberately small: it carries only product identity, the
hard invariants an agent must hold before acting, and routing. Process detail
(repository execution rules, proportional acceptance) lives in [docs/current/product/agent-operating-rules.md](docs/current/product/agent-operating-rules.md);
canonical contracts live in the docs linked under [Routing](#routing). On an
apparent conflict, identify the exact question and repair the non-owning
projection. Neither this router nor any linked document wins outside its named
scope.

## AgentFirm Execution Foundation Authority

AgentFirm's current product mental model and development control plane live in
Notion. Repository files remain the versioned implementation truth for code,
tests, CI, executable contracts, and shipped behavior.

- Start at [AgentFirm Home](https://app.notion.com/p/3b849a4fa3798115939cca2b0b9e6f2d)
  and verify its current authority notice before any Notion mutation.
- Use the [Development System](https://app.notion.com/p/21e49a4fa37982a5b9f781cf04584034)
  for its two ordinary tables: Development Tasks owns current work state;
  Development Documents holds readable Dev/Spec submissions and immutable
  Review history. Delivery Runs are Legacy / Advanced history, not a second
  current authority.
- Follow the [Development Playbook](https://app.notion.com/p/3b849a4fa37981a990a5cf0059dcfa4a)
  for Brain assignment, Dev work, exact-revision submission, Review, merge,
  and closeout.
- Never claim or update work in the Notion area labeled Legacy Production /
  READ ONLY / DO NOT CLAIM. Page location and relations never grant authority;
  authority follows the current AgentFirm Home notice.

Accepted Notion docs own current product intent. This checkout's code, schemas,
configuration, and applicable `AGENTS.md` files own executable constraints.
When accepted intent is ahead of this checkout, treat target-only text as
non-operative, record the Implementation Crosswalk delta, and fail closed; use
the development Skill for the transition procedure.

The Development System has two ordinary tables. One Development Task owns the
current state of a change. Development Documents contains directly readable
Dev/Spec submissions and immutable Review documents; a Review binds the exact
document version or Git SHA it inspected. A provider Session is only an
executor and native transcript owner, never a Task ledger or Review authority.
If Task, Session, submission, or Review state disagrees, stop dispatching new
work and reconcile the Task from those authoritative records first.

## Development System Model

Repository development uses four layers. Each instrument serves its own layer;
using acceptance or feedback as an unbounded development driver is a process
failure:

```text
Intent      Notion PRD / Spec / ADR
                | Brain slice / triage
Task        one Development Task owns current execution state
                | exact revision
Acceptance  Task Review -> PR CI -> Spec acceptance / integrated dogfood
                | findings
Feedback    GitHub Issue Pool -> current Task / batch later / record only
```

Repository code, schemas, tests, and CI own merged shipped implementation
truth. They are not a second Task state layer. The Implementation Crosswalk
maps accepted intent to repository evidence; it is not another task ledger or
approval gate.

- A finding required by the current Task acceptance or submitted diff stays in
  that Task. An out-of-scope finding enters the Issue Pool and is non-blocking
  by default; Brain may batch it into a later Task, defer it, or only record it.
- Only a finding that prevents the current acceptance run, invalidates its
  evidence, or breaks safety, integrity, or an authority boundary may be
  hot-fixed before that run continues.
- Reviewer blocking findings bind to the submitted revision and current Task
  acceptance. Spec-level dogfood is an exam after that Spec's Tasks merge, not
  a development mode; ordinary findings return to the Issue Pool.
- Observer audits trajectory on cadence for long work and when a repair chain
  exceeds two links, instructions are repeatedly restated, or the user is
  dissatisfied. It returns continue / intervene / escalate / stop and never
  edits the artifact, owns Task status, or replaces Reviewer.
- Human decides scope trade-offs, architecture authority, and risk acceptance.
  Protected external-effect approval remains a separate safety decision.

Task has one non-success terminal state, `Cancelled`, for an obsolete,
explicitly superseded, or no-longer-authorized outcome. It does not mean Pass
and does not create a second archive object.

Creating, assigning, executing, reviewing, retrying, blocking, or completing a
repository Development Task triggers the single
[agentfirm-development-loop](.agents/skills/agentfirm-development-loop/SKILL.md).
Reading, explaining, or exploring the repository without operating that
lifecycle does not trigger it merely because the file exists here.
Before editing, inspect the actual branch, revision, worktree, and affected
paths. Preserve user and other-session changes; do not reset, clean, revert, or
reformat unrelated work. Resolve a real path collision through the Brain.

## Product Identity

Star Harness is the AgentFirm execution foundation: durable flat AgentTeams
of standing AgentMembers, accountable Work, identity-first Messages, and
provider-neutral runtimes across machines. DOC-108 retired the legacy
Company OS layer: the legacy Company Store, built-in Docs, Organization,
Finance, generic Approval, and the legacy Mission, Wave, and Mission Log. Its
writers are closed on every surface and its rows are export/verify-only through
`harness legacy-company-os export|verify`. See
[docs/current/product/prd.md](docs/current/product/prd.md).

Agent Team execution, providers, plugins, and MCP are the shared execution
foundation. A Team Host is an AgentMember using the ordinary managed Team
runtime or the explicit pull-only `external_interactive` exception, not a
second executor model. Dynamic Workflow is retired; its historical records are
available only through lossless legacy archive export, verification, and
restore-read. Their current native relations are:

```text
AgentTeam -> immutable node_id -> one machine-scoped NodeDaemon
AgentTeam -> TeamMembership -> AgentMember
AgentMember -> AgentSession -> provider-native session/thread
Work -> WorkExecutionBinding -> exact AgentSession generation
identity-first Message -> MessageSubscription -> per-recipient CanonicalMessageDelivery
NodeDaemon -> durable RuntimeCommand -> provider effect
```

`AgentMember` is the sole durable agent identity; `TeamMembership` records only
participation and never carries identity. The legacy `AgentIdentity` name survives
solely as a deprecated same-ID read-only projection of `AgentMember`: the
legacy compatibility edge `AgentIdentity -> AgentSession` names the exact same
edge as `AgentMember -> AgentSession` above, never a second identity root.

`Mission` is retired (DOC-108): pre-cutover rows remain read-only legacy
provenance through `harness mission list|show|log show`,
`AgentTeam.legacy_mission_id`, and the Stage A export. A Team never spans
machines, and no new legacy Mission, Mission Log, or Wave row may be written
on any surface.
`NodeDaemonLease` is machine-scoped authority for all local Teams across
registered Execution Spaces; each machine has one machine-scoped NodeDaemon and
the lease is never scoped to one Execution Space.
`TeamRun` and `MemberRun` remain coordination/history projections; they never
own a provider process or authorize a provider effect. Every provider effect is
prepared and settled through a durable `RuntimeCommand` bound to the exact
NodeDaemon and AgentSession generations. Messages, Work delivery, and runtime
control are separate planes and cannot impersonate one another.
Cross-Team responsibility uses explicit WorkDelegation rather than parent/child
Team topology. AgentMember is the one durable organization-agent identity.
Global Work (DOC-106) is a read-only aggregate over authoritative TeamWork and
must never regain a second task ledger or mutation path; it replaced the
former Company Work aggregate. Repository self-hosting remains the first
execution-foundation scenario.

## Hard Invariants

These rules bind every agent working in this repository. The linked canonical
doc carries the contract behind each rule.

1. **Native-session truth.** The provider's native session store is the sole
   execution truth for a member's transcript, tool calls, commands, file
   events, and provider turn lifecycle; do not mirror those
   streams into Harness ledgers. A provider `completed` status is not by
   itself proof of semantic success, answer, or approval.
   Resume must use the provider-native session id and verified provider
   operation, never a replay assembled from Harness events (ADR 0032,
   [docs/current/integration/native-session-storage.md](docs/current/integration/native-session-storage.md)).
2. **Harness owns only coordination records.** For Agent Team execution:
   `AgentTeam` (with optional legacy Mission provenance), `AgentTeamRun`,
   `MemberRun` plus its
   native-session binding, identity-first `Message`, `MessageSubscription`,
   per-recipient `CanonicalMessageDelivery`, explicit outcome and
   artifact/check references, and control acknowledgements.
   Agent Team responsibility is proven by the latest `Work` rebuilt from
   ordered `WorkOperation` rows, each preserving its append-only `WorkEvent`
   and `WorkDelivery` deltas. A `Message` is authored conversation only and may
   link a `work_id`; correlated provider requests and responses are Message
   kinds, not a second interaction object. `Work`, Message delivery, and
   `RuntimeCommand` are independent planes and cannot authorize or mutate one
   another. `TeamMessage`, `TeamMessageProjection`, `team_messages.jsonl`, and
   their ACK/manual-ACK writers are Legacy read/export evidence only. There is
   no Assignment Message compatibility path. Dynamic Workflow is retired: its
   historical records are legacy archive evidence only and no current surface
   may write or project them as live state. For `host`, record the observable
   outcome and artifacts without inventing controlled child objects or another
   task ledger.
3. **Mission/Wave/Mission Log are legacy, export-only history.** DOC-108
   retired them from the current model: no Mission, Mission Log, or Wave row
   may be created, updated, advanced, gated, or closed on any surface (CLI,
   HTTP, MCP). Historical rows stay readable through `harness mission
   list|show|log show`, `harness legacy wave list|show|history`, and the
   Stage A `harness legacy-company-os export|verify` path.
   The superseded coordination stack is being removed under ADR 0028: do not
   load it into normal planning context, create new records, use its commands,
   or add new dependencies. Historical stores must be exported and verified
   before their old ledgers or code are deleted.
4. **Execution/Project separation.** Execution Spaces own current Agent Team
   coordination and historical archive selection; Project Bindings identify the
   repository where providers execute and discover instructions, Skills,
   plugins, and MCP configuration. Selecting `--project` never switches the
   coordination store. Provider cwd resolves the attached
   `MemberWorkspaceBinding.canonical_root` > TeamRun `execution_root` > binding
   `project_root` — never an Execution Space,
   Company Store, or compatibility store root. Select space and project
   explicitly; never silently migrate or dual-write
   ([docs/current/operations/multi-project.md](docs/current/operations/multi-project.md), ADR 0033, ADR 0042).
5. **Provider upgrade gates.** Provider capability claims are execution-mode
   and version specific. Run `harness member providers --fail-on-review` after
   provider upgrades; an unreviewed version is `review_required`, not silently
   compatible. Change only one provider at a time; record the current version,
   candidate, install channel, and rollback path; never hot-replace the runtime
   of an active MemberRun or native session; roll back when installation,
   protocol probing, deterministic acceptance, or the live canary fails.
6. **Safety.** Authentication, payment, license acceptance, new credentials,
   and other protected actions require the appropriate Human or policy
   approval. The trusted-development Team policy that gives Codex, Claude, and
   Kimi members full execution access is a product policy — not a Provider
   capability and not approval for protected external effects. That permission
   policy is separate from execution-roster selection, which is scenario
   policy rather than a repository invariant (see Repository Execution Rules).
7. **Member lifecycle and control honesty.** New Agent Team members use only
   their persistent bidirectional mode: `codex_app_server`, `kimi_acp`, or
   `claude_agent_sdk`. Interrupt stops one current turn; Close ends the member
   runtime; TeamRun completion never implies Close. Cross-process
   control routes through the durable Team Supervisor lease, revalidated
   immediately before every drive; uncertain claimed deliveries require
   explicit reconciliation, never blind replay. Replacing a runtime drains or
   interrupts active turns first and never lets two runtime generations drive
   the same MemberRun/native session; resume that native session under a higher
   Supervisor generation only when the reviewed contract allows, otherwise
   record the reason and start a new session, retaining the old one as
   history.
8. **One execution driver.** Each active MemberRun/native session has exactly
   one top-level execution driver: `host_driven` or `provider_driven`.
   Multiple explicitly bound Sessions may share one cwd; worktrees are optional
   task isolation. Never activate a provider-native goal and also issue an
   ordinary Harness start for the same work. Provider satisfaction never
   implies Host acceptance
   ([docs/current/architecture/member-continuation-model.md](docs/current/architecture/member-continuation-model.md),
   ADR 0041).
9. **No Plan Gate.** When the Host wants a plan first, it asks through an
   ordinary correlated Markdown message; the member replies, and the Host
   argues or approves in the same chain. Provider-native plan/goal features
   remain internal execution aids. Provider-native or chat-side subagents are
   implementation details of whoever invoked them; the harness must not claim
   lifecycle control it does not have.
10. **Honest capability claims.** The execution-foundation contract set —
    AgentTeam, TeamMembership, AgentMember, AgentSession, Work, Message, and
    RuntimeCommand — is the only contract set you may cite as shipped, and
    only where schemas, stores, APIs, and acceptance checks prove the specific
    object or field. Do not claim planned objects or fields exist before that
    proof, and keep the design-contract vs implemented-schema distinction
    explicit. In particular,
    AgentTeam authority is flat, and every
    Team has immutable `node_id` placement under one machine-scoped NodeDaemon.
    The unified Work kernel is the shipped authority: do not
    create a second organization-agent identity or recreate a retired
    company-scoped task ledger, migration fallback, or dual-write Work path.
11. **Skill optionality.** Skills are procedural capabilities, never product
    architecture authority. Load the development Skill when operating the
    repository Development Task lifecycle named above; do not load it for an
    unrelated read-only question. Other Skills load only when their own trigger
    applies. Accepted Docs and checked-out executable constraints remain the
    question-scoped authorities. Retired planning skills must not be installed,
    loaded, or referenced from active repository instructions. The generic
    harness core stays domain-neutral.
12. **One agent-config source.** All repository skill edits happen under
    `.agents/skills/` only. `.claude/skills` is a committed symlink to
    `.agents/skills`, and root `CLAUDE.md` is a thin import of this file —
    neither may grow independent content. Enforced by
    `scripts/check-agent-config-sync.mjs` in `pnpm check`.

## Routing

- Product requirements: [docs/current/product/prd.md](docs/current/product/prd.md); architecture:
  [docs/current/architecture/architecture-map.md](docs/current/architecture/architecture-map.md); concept model:
  [docs/current/architecture/concept-model.md](docs/current/architecture/concept-model.md)
- Detailed operating rules and the relocation map for this slimming:
  [docs/current/product/agent-operating-rules.md](docs/current/product/agent-operating-rules.md)
- Execution Spaces and Project Bindings:
  [docs/current/operations/multi-project.md](docs/current/operations/multi-project.md)
- Member continuation and execution drivers:
  [docs/current/architecture/member-continuation-model.md](docs/current/architecture/member-continuation-model.md)
- Provider runtime substrate: [docs/current/architecture/agent-runtime.md](docs/current/architecture/agent-runtime.md);
  integration model:
  [docs/current/architecture/agent-integration-model.md](docs/current/architecture/agent-integration-model.md)
- Operations gates and commands: [docs/current/operations/operations.md](docs/current/operations/operations.md)
- Dogfood method:
  [docs/current/product/agent-team-dogfood-loop.md](docs/current/product/agent-team-dogfood-loop.md)
- Documentation governance:
  [docs/current/documentation-governance.md](docs/current/documentation-governance.md)
- ADRs: [docs/decisions/README.md](docs/decisions/README.md) — especially 0028
  (retired coordination stack), 0032 (provider-native session truth), 0033
  (member workspace), 0041 (member continuation), 0042 (Execution Spaces and
  Project Bindings). ADR 0026/0027/0034/0051 are superseded legacy
  Mission/Wave/Company-OS history (DOC-108), never deleted.

## Code structure

Structure is a first principle here because this repository is edited mostly by
agents, and an agent pays for every line it must scan to find one seam.

- A maintained source file over 1,500 lines is a defect and the blocking
  governance gate enforces that ceiling. Passing the size gate does not prove
  that package ownership or dependency direction is correct.
- Prefer one seam per module: command dispatch, HTTP routing, help text and
  provider runtimes do not belong in one file.
- Keep tests out of the middle of an implementation file. Put them at the end
  or in `tests/`; interleaved `#[cfg(test)]` blocks make "which tests cover
  this?" unanswerable without reading everything.
- A new file is cheaper than a new 500-line function. Splitting is mechanical
  and test-guarded; growth is neither.

The governance `size` gate warns (never blocks) on oversized files so existing
debt can retire in order instead of freezing work.

## Where the rest lives

This file states first principles only. Everything procedural is one hop away:

- Repository execution rules and proportional acceptance:
  [docs/current/product/agent-operating-rules.md](docs/current/product/agent-operating-rules.md)
- Repository development procedure:
  [.agents/skills/agentfirm-development-loop/SKILL.md](.agents/skills/agentfirm-development-loop/SKILL.md)
- Host and Member collaboration procedure:
  [skills/collaborate-as-agent-team-member/SKILL.md](skills/collaborate-as-agent-team-member/SKILL.md)
- Gates and commands:
  [docs/current/operations/operations.md](docs/current/operations/operations.md)

Keep it that way: a rule earns a place here only if an agent would act wrongly
without it before reading anything else.
