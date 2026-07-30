# ADR 0046: Supervised AgentOS self-hosting loop

```text
status: active
date: 2026-07-30
owner_role: product-architecture
```

## Context

ADR 0027 made Docs and the mixed Organization the Company OS product core.
ADR 0042 separated Company Store, Execution Space, and Project Binding. ADR
0045 added the explicit Company-owned relation from a StandingAgent to reusable
AgentMember execution configuration.

The missing operating decision was how a Human, the current AI task, a durable
Lead, governance Agents, and the execution runtime work together while AgentOS
uses itself to build itself. Without it, implementations risk one of four
errors:

- treating the current Codex conversation as the durable Lead identity;
- inventing a second organization out of TeamRun members;
- making a deterministic runtime process responsible for company judgment;
- claiming dogfood after one scripted Docs-to-Work demo.

## Decision

### 1. The current AI task is a supervising operator, not the durable Lead

During bootstrap the Human may communicate with an AI Supervising Operator in
Codex, Claude, Kimi, or another client. The Operator can inspect, coach, and
operate the Company OS within granted authority. It does not become a
StandingAgent merely because it is active.

AgentOS has an independent Company-owned Lead StandingAgent. Provider/runtime
replacement does not change that identity, its organization history, queue,
permissions, or source/result relations.

### 2. Runtime supervision and company leadership remain separate

The Lead makes product and operating judgments. A deterministic Runtime
Supervisor owns runtime leases, delivery claims, retries, health, recovery,
interrupt, resume, and Close. Runtime health does not grant authority, and a
company status does not prove a live provider process.

### 3. Standing Agent execution reuses the shared Agent runtime substrate

Company Store owns StandingAgent identity, Organization relationships, and
authority. Execution Space owns AgentMember, runtime, delivery, and
provider-native session bindings. The provider-native store remains the only
transcript and tool-activity truth.

The join is explicit:

```text
StandingAgent.execution_agent_member_ref
  -> AgentMember.id
  -> AgentRuntime / MemberRun
  -> provider-native session
```

No same-id inference, synthetic TeamRun, transcript mirroring, or execution
lifecycle writeback is allowed. Direct Standing Agent work may use the shared
Agent runtime without creating a TeamRun. Agent Team remains available when
several persistent Members are useful for one execution.

### 4. Stable Inbox delivery is state-aware

External UI, CLI, another Agent, or the Supervising Operator sends a durable
message to the linked Agent identity. Delivery behavior depends on runtime
state:

- **busy:** queue or use a reviewed provider-native steer path; never start a
  second top-level writable turn;
- **idle:** deliver and start the next provider cycle;
- **offline/dead:** retain the message, recover or resume the runtime from its
  native session, and deliver exactly once;
- **closed:** reject or require an explicit reopen/resume decision.

Message acceptance, transport delivery, provider receipt, semantic reply, and
WorkItem acceptance are distinct facts. A transport ACK or receipt-only peer
confirmation does not itself require a semantic reply. Messages that require a
new provider round must carry explicit response intent; otherwise idle Agents
may converge without emitting acknowledgement-only mail. This prevents
confirmation ping-pong while preserving durable questions, blockers, reviews,
handoffs, and Host decisions.

### 5. Dogfood is a continuous coequal-system loop

Docs, Work, and Organization may each produce the next observation and may each
receive the accepted result. There is no mandatory global order. Finance joins
only when a monetary effect exists. Execution is selected by Work and never
replaces company identity, responsibility, or knowledge.

Every material issue discovered while operating the product is recorded as a
linked WorkItem, an Org proposal, a Docs finding, or an explicit accepted
exception. The next iteration must use the improved product.

### 6. Permission catalogs belong to product systems

Docs, Work, Organization, and Finance define their own capabilities and
approval boundaries. Parent Agents may propose children and delegate work only
within their declared ceiling. Provisioning or expanding sensitive authority
remains governed and may require the Human Owner.

### 7. Harness updates reconcile runtimes; they do not replace Agents

Projection-only changes leave provider runtimes untouched. A same-contract
Harness process recovery acquires a new Supervisor generation and reattaches
the same unclosed MemberRun and native session. A delivery, adapter, permission,
model/effort, or Plugin contract change requires a controlled drain or
interrupt and an explicit replacement runtime generation.

StandingAgent identity, Work ownership, Assignment correlation, and company
history remain stable across that transition. The provider-native session is
resumed only when the reviewed mode/version declares it compatible. Two runtime
generations may never concurrently drive the same writable Workspace. A
replacement generation is not accepted merely because it acquired a lease: it
must reconcile the same durable Agent/Member identity, outstanding mail,
current Assignment, effective permissions and provider controls, and native
session binding before it resumes delivery.

## Consequences

- The user may continue speaking only to the Supervising Operator while the
  durable Lead receives normal governed messages.
- Organization pages show only durable Actors; runtime members and subagents
  appear as linked execution, not chart nodes.
- The Standing Agent workspace can reuse Member activity, Inbox, and runtime
  components without sharing identity or lifecycle.
- Company OS projections must select relations by explicit refs. First-row
  fallback is forbidden on selected entity pages.
- "Dogfood complete" cannot mean one happy path. Acceptance is a continuously
  reconstructable set of real operating cycles.

## Rejected alternatives

- **Current Codex task is the permanent Lead:** loses stable company identity
  and makes history depend on one UI task.
- **TeamRun roster is Organization:** confuses one execution with durable
  authority and lifecycle.
- **Runtime Supervisor decides company work:** mixes deterministic process
  control with business judgment.
- **One universal permission bit:** cannot express module ownership or
  sensitive effects.
- **One fixed Docs -> Work -> Docs pipeline:** hides valid Work-to-Org,
  Org-to-Docs, and runtime-to-Work loops.

## Validation

- Company Store contains the Human Owner, AgentOS Lead, and governance
  Standing Agents with explicit memberships and permission refs.
- The selected Standing Agent resolves to at most one AgentMember execution
  configuration; equal ids alone do not bind.
- UI deep links preserve Company Store, Execution Space, and Project Binding.
- A selected WorkItem or Document renders only explicitly related native rows.
- Busy, idle, offline/recovered, and closed Inbox delivery have deterministic
  tests before the composer is enabled.
- At least three real dogfood cycles exercise different starting systems and
  return accepted results without manual ledger editing.

## Related contracts

- [AgentOS self-hosting dogfood loop](../company-os/agentos-self-hosting-loop.md)
- [Organization and actors](../company-os/organization-and-actors.md)
- [Collaboration and Agent Work](../company-os/collaboration-and-agent-work.md)
- [ADR 0042](0042-company-store-execution-space-project-binding.md)
- [ADR 0045](0045-company-owned-standing-agent-execution-relation.md)
