# Product Requirements — AgentFirm execution foundation

```text
status: canonical repository contract
supersedes: the Company OS primary model (DOC-108; ADR 0027 superseded)
```

## Product mission

Star Harness is the provider-neutral execution foundation for an AI-native
company: a person runs durable, flat AgentTeams of standing AgentMembers that
hold accountable Work, converse over identity-first Messages, and execute
through provider-native sessions across machines.

The product intent and operating control plane for the company being built on
top of this foundation live in Notion (AgentFirm Home). This repository is the
versioned implementation truth for the execution substrate: code, schemas,
stores, CLI/HTTP/MCP surfaces, tests, and CI.

## Retired layer (DOC-108)

The legacy Company OS product model — the Company Store registry, built-in
Docs system, Organization/actor module, Finance records, generic business
Approval, Mission, Wave, and Mission Log — is retired from the product model
and normal navigation:

- no writer exists on any surface (CLI, HTTP, MCP, SSE, frontend);
- historical rows remain readable as legacy provenance
  (`harness mission list|show|log show`, `harness legacy wave
  list|show|history`, `AgentTeam.legacy_mission_id`);
- historical runtime data is export/verify-only through
  `harness legacy-company-os export|verify` (the Stage A contract);
- ADR 0026/0027/0034/0051 remain as superseded historical evidence, never
  deleted.

## Product thesis

A company of agents needs more than parallel provider sessions:

- durable AgentMember identities with roles, permissions, and provider runtime
  history, bound to Teams by versioned TeamMembership generations;
- one explicit record of who requested, claimed, executed, submitted,
  reviewed, and accepted each unit of Work;
- execution that survives process restarts and machine boundaries without
  inventing transcript authority;
- provider neutrality: the harness never mirrors provider-native transcripts
  into its own ledgers, and provider satisfaction never implies Host
  acceptance.

## Primary systems

### Agent Teams

A durable `AgentTeam` is flat and placed immutably on one machine (`node_id`)
under that machine's single NodeDaemon. TeamMembership binds AgentMembers with
exact generations. `AgentTeamRun` and `MemberRun` are coordination/history
projections — they never own a provider process.

The Host explicitly creates, messages, inspects, interrupts, closes, reopens,
and retires members. Close releases the managed runtime while retaining the
MemberRun and provider-native session; Reopen increments the runtime
generation and resumes that exact session. TeamRun completion never implies
Close.

### Work

`Work` is the single durable unit of accountable execution. Its truth is
rebuilt from ordered `WorkOperation` rows, each preserving append-only
`WorkEvent` and `WorkDelivery` deltas. Work carries lifecycle axes (phase,
condition, resolution), owner TeamMembership, evidence, artifacts, checks,
gate results, and explicit submission/acceptance. The Global Work RoleView is
a read-only aggregate over authoritative TeamWork and owns no second task
ledger or mutation path.

### Messages

Identity-first `Message` is authored conversation. `MessageSubscription`
selects authorized sources; each recipient owns one `CanonicalMessageDelivery`
with its own claim/acknowledgement state. Peer-Team messaging is admitted by
MessageSubscription authorization and the `collaboration.peer_message_deliver`
capability — never by a Company policy object or an implicit roster.

### Runtime and fabric

- one machine-scoped NodeDaemon owns all local Team execution across
  registered Execution Spaces;
- every provider effect is prepared and settled through a durable
  `RuntimeCommand` bound to exact NodeDaemon and AgentSession generations;
- `AgentSession` binds the provider-native session, which remains the sole
  execution truth for transcripts, tool calls, and turn lifecycle;
- provider admission is versioned and review-gated; an unreviewed provider
  tuple is `review_required`, never silently compatible.

### Execution Spaces and Project Bindings

Execution Spaces own Agent Team and Workflow coordination. Project Bindings
identify the repository where providers execute and discover instructions,
Skills, plugins, and MCP configuration. Selecting `--project` never switches
the coordination store. Provider cwd resolves the attached
`MemberWorkspaceBinding.canonical_root` > TeamRun `execution_root` > binding
`project_root`.

## Required product experiences

1. An operator dashboard (Agent Dashboard) presents Nodes, durable Teams,
   runs, member lifecycle, the shared Team Inbox, and Work boards from store
   truth with bounded snapshots.
2. CLI, HTTP, and MCP surfaces share one TeamMembership and Work authority.
3. Cross-machine Teams collaborate through the remote fabric with explicit,
   fenced delivery.
4. Dynamic Workflows run provider-neutral scripted processes for bounded
   outcomes.
5. Every acceptance claim reconstructs from the store and the provider-native
   session.

## Non-goals

- no second organization-agent identity beside AgentMember;
- no Company task ledger, migration fallback, or dual-write Work path;
- no writers for the retired Mission/Wave/Mission Log or Company OS objects, no
  reads of them as current authority, and no dual runtime control;
- no raw provider transcripts or thinking as Harness evidence;
- no name-based mapping of responsibility or identity.

## Implementation truth

The execution foundation described here is implemented and acceptance-gated.
Documentation must label planned fields and projections honestly until
schemas, store, APIs, fixtures, and acceptance checks exist. Historical
compatibility schemas (Mission, Wave) remain validated only so old rows can
be read and exported.
