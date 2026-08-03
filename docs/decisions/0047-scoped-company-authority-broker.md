# ADR 0047: Scoped Company authority broker

```text
status: accepted target contract; Agent Team execution chain amended by ADR 0050; implementation pending
date: 2026-07-31
owner_role: company-authority-architecture
```

## Context

Company OS write transport currently depends on the service-side
`HARNESS_COMPANY_OS_TOKEN`. Action policy then validates Company Actor status,
permission refs, scope, risk, and Human Approval. Giving the root transport
secret to a StandingAgent, AgentMember, MemberRun, or provider-native session
would collapse those layers and make one execution binding a Company
administrator.

ADR 0045 keeps StandingAgent identity separate from AgentMember and MemberRun.
ADR 0044 fences provider control with Supervisor generations. ADR 0046 requires
permission catalogs and Company authority to remain separate from runtime
health. A bounded Company write therefore needs an explicit bridge across all
three decisions without becoming a provider permission framework.

## Decision

Adopt the target contract in
[Scoped Company Authority Broker](../company-os/scoped-authority-broker.md).

The durable, non-secret `ScopedPermissionGrant` belongs to the Company Store
and grants one StandingAgent an intersection of exact Company Assignment,
Agent Team Work/version, WorkDelivery to an exact MemberRun, permission,
command, subject id, effect, and
command-specific constraints. V1 has no wildcard, inherited, module-wide, or
delegable scope.

Every authority-bearing `(grant_id, grant_generation)` snapshot is immutable
and has a canonical grant/scope digest. Any change to the grantee, permission,
rules, Company Assignment, Work/version, WorkDelivery, validity, expiry, lease TTL, use budget, or
other authorization input creates a new grant or higher generation and fences
older unconsumed receipts. The Human R3 activation decision binds the exact
grant id, generation, and digest. Display-only metadata is outside the digest
and cannot affect authorization.

A Supervisor-bound Company broker resolves:

```text
StandingAgent -> AgentMember -> MemberRun -> native session
Company Assignment -> WorkExecutionChain -> Agent Team Work
Agent Team Work -> WorkDelivery -> exact MemberRun
```

It then issues and consumes one short internal lease while dispatching at most
one canonical ActionCommand. The returned `CapabilityLeaseReceipt` is durable,
non-secret evidence; it is not a bearer credential. The broker alone holds the
root Company transport secret. For this broker path, the active grant satisfies
the server policy's matching required permission without placing a broad
permission ref on the StandingAgent; non-broker Actor-permission checks remain
unchanged.

Grant and Supervisor generations are both revalidated immediately before
dispatch. Expiry or revocation fences unconsumed receipts. Stable request and
ActionCommand ids make retry observational and idempotent; uncertain
post-crash delivery is reconciled, never blindly replayed.

Allowed and denied broker decisions are durable. Allowed commands keep
`PolicyAuthorized` and terminal ActionCommand audits. The target implementation
adds `PolicyDenied`, rejected ActionCommand persistence, and an atomic denied
receipt without mutating the business subject.

Grant activation and expansion are R3 `ChangePermission` effects. They require
an explicit, unexpired Human decision and Company Audit trail. Documentation,
code merge, deployment, runtime start, Assignment delivery, and receipt
possession never activate authority.

## Consequences

- Standing Agents can later perform one reconstructable Company command
  without receiving the root token.
- Company responsibility, execution identity, transport delivery, and
  authorization remain separate facts that must all resolve.
- The first implementation is intentionally limited to an exact
  one-use `work_item.transition` for
  `work-agentos-org-role-permission-closure-v1`, from `in_progress` to
  `in_review`, with an exact result/evidence/outcome payload; unrelated payload
  mutations plus Organization, permission, Finance, legal, credential,
  payment, and external-dispatch effects remain denied.
- The Store and dispatcher need explicit denial evidence and generation-aware
  atomic operations before any live authority is activated.

## Rejected alternatives

- **Pass the root token to the runtime:** grants every transport-level write and
  makes revocation, attribution, and least privilege impossible.
- **Authorize from StandingAgent permission refs alone:** does not bind a
  delivered Assignment, exact runtime/session, command, subject, transition,
  or Supervisor generation.
- **Treat provider sandbox permission as Company authority:** conflates local
  execution controls with business authorization.
- **Issue a reusable scoped bearer token:** adds secret distribution and replay
  risk when the broker can dispatch the one command itself.

## Validation

Acceptance requires deterministic expiry, revocation, generation,
idempotency, denial-audit, and recovery checks plus one real proof that:

- the exact StandingAgent/AgentMember/MemberRun/native-session and Company
  Assignment/WorkExecutionChain/Work/WorkDelivery chains resolve;
- the named WorkItem transitions once from `in_progress` to `in_review` with
  only the frozen result/evidence/outcome payload;
- unrelated Work, unrelated payload mutation, a second successful use, and
  every protected action class are denied without mutation;
- allowed and denied receipts and AuditEvents are reconstructable; and
- no Company root token appears in the member environment, request, receipt,
  logs, artifacts, or provider-native transcript.
