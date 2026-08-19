# Agent Operating Rules

This repository builds Star Harness itself. Product truth lives in canonical
docs, schemas, ADRs, and implemented stores. Execution claims must additionally
be reconstructable from the native runtime records of the executor used.

This root file is deliberately small: it carries only product identity, the
hard invariants an agent must hold before acting, and routing. Process detail
(repository execution rules, proportional acceptance) lives in [docs/current/product/agent-operating-rules.md](docs/current/product/agent-operating-rules.md);
canonical contracts live in the docs linked under [Routing](#routing). Where
this file and a canonical doc conflict, the canonical doc wins — fix this file.

## AgentFirm Execution Foundation Authority

AgentFirm's current product mental model and development control plane live in
Notion. Repository files remain the versioned implementation truth for code,
tests, CI, executable contracts, and shipped behavior.

- Start at [AgentFirm Home](https://app.notion.com/p/3b849a4fa3798115939cca2b0b9e6f2d)
  and verify its current authority notice before any Notion mutation.
- Use the [Development System](https://app.notion.com/p/21e49a4fa37982a5b9f781cf04584034)
  for the current Task and its immutable Review history. Delivery Runs are
  Legacy / Advanced history, not a second current authority.
- Follow the [Development Playbook](https://app.notion.com/p/3b849a4fa37981a990a5cf0059dcfa4a)
  for Brain assignment, Dev work, exact-revision submission, Review, merge,
  and closeout.
- Never claim or update work in the Notion area labeled Legacy Production /
  READ ONLY / DO NOT CLAIM. Page location and relations never grant authority;
  authority follows the current AgentFirm Home notice.

Notion owns current product intent and operating state. The repository owns
what is actually implemented. When they diverge, record the gap in the
Implementation Crosswalk and Development Work rather than silently rewriting
either side.

## Product Identity

Star Harness is the AgentFirm execution foundation: durable flat AgentTeams
of standing AgentMembers, accountable Work, identity-first Messages, and
provider-neutral runtimes across machines. DOC-108 retired the legacy
Company OS layer: the legacy Company Store, built-in Docs, Organization,
Finance, generic Approval, and the legacy Mission, Wave, and Mission Log. Its
writers are closed on every surface and its rows are export/verify-only through
`harness legacy-company-os export|verify`. See
[docs/current/product/prd.md](docs/current/product/prd.md).

Agent Team, Dynamic Workflow, Host execution, providers, plugins, and MCP are
the shared execution foundation. Their native relations are:

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
   no Assignment Message compatibility path. For `dynamic_workflow`,
   WorkflowRun/WorkflowStep and its
   result/artifacts are the execution truth; for `host`, record the observable
   outcome and artifacts without inventing controlled child objects.
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
4. **Execution/Project separation.** Execution Spaces own Agent Team and
   Workflow coordination; Project Bindings identify the
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
   the same writable Workspace; resume the same native session under a higher
   Supervisor generation only when the reviewed contract allows, otherwise
   record the reason and start a new session, retaining the old one as
   history.
8. **One execution driver.** Each active MemberRun/native session/writable
   Workspace has exactly one top-level execution driver: `host_driven` or
   `provider_driven`. Never activate a provider-native goal and also issue an
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
11. **Skill optionality.** Skills are optional capabilities, never the
    authority for product architecture or Lead behavior. Do not load a skill
    merely because you are working in this repository; canonical docs, schemas,
    code, and ADRs win any conflict. Retired planning skills must not be
    installed, loaded, or referenced from active repository instructions. The
    generic harness core stays domain-neutral.

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

- A source file over ~1500 lines is a defect to be scheduled, not a style
  preference. `crates/firm-cli/src/main.rs` is currently 53k lines and is the
  standing counter-example, not a precedent.
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
- Host and Member collaboration procedure:
  [skills/collaborate-as-agent-team-member/SKILL.md](skills/collaborate-as-agent-team-member/SKILL.md)
- Gates and commands:
  [docs/current/operations/operations.md](docs/current/operations/operations.md)

Keep it that way: a rule earns a place here only if an agent would act wrongly
without it before reading anything else.
