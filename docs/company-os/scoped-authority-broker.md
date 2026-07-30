# Scoped Company Authority Broker

```text
status: target canonical contract; not implemented
owner_role: Organization and Work Governance
canonical_for: least-privilege Company Action authority for bound Standing Agent execution
```

## Current truth and target boundary

Today Company OS write transport is protected by the service-side
`HARNESS_COMPANY_OS_TOKEN`. The Action dispatcher separately checks an active
Company Actor, the server-owned Action policy, the Actor's permission refs,
scope, risk, and any required Human Approval. Successful dispatch preserves
policy-authorized and terminal AuditEvents. The transport token is still one
root capability, however, and pre-authorization denials are not durable
AuditEvents.

`ScopedPermissionGrant`, `CapabilityLeaseReceipt`, and the Company authority
broker defined below are **target contracts only**. They do not exist in the
current schemas, Store, API, CLI, Dashboard, skills, or plugins. Until those
surfaces and their acceptance checks exist, a StandingAgent, AgentMember,
MemberRun, or provider-native session must not receive
`HARNESS_COMPANY_OS_TOKEN` and must not be described as holding brokered
Company write authority.

The broker adds a narrow authorization path; it does not replace the Action
dispatcher. Company identity and Work Assignment remain durable Company truth.
MemberRun and provider-native session remain execution bindings. Provider
permission profiles remain a separate execution concern.

On the broker path, the matching active grant is the authorization basis for
the Action policy's `required_permission`; the StandingAgent does not need a
broad matching value added to `permission_policy_refs`. The dispatcher must
accept that basis only as broker-attested context. Non-broker callers retain
the existing Actor-permission checks.

## Binding invariants

An authority request is eligible only when the broker resolves both chains
without inference:

```text
StandingAgent.execution_agent_member_ref
  -> AgentMember.id
  -> MemberRun.agent_member_id
  -> MemberRun.native_session

Company Assignment
  -> delivery_evidence_ref
  -> delivered TeamMessage(kind=assignment, correlation_id)
  -> exact MemberRun
```

The Company Assignment recipient must be the same StandingAgent named by the
grant. Its WorkItem, correlation, and delivery evidence must match the
TeamMessage delivered to the exact MemberRun. Equal ids, names, roles,
providers, timestamps, process ownership, or possession of a receipt never
create a missing edge.

The active Team Supervisor supplies the MemberRun, native-session, Supervisor
id, and Supervisor generation from its authenticated control channel. Request
fields are selectors to cross-check, not caller-supplied identity proof.

## Target `ScopedPermissionGrant`

A `ScopedPermissionGrant` is a durable, non-secret Company record:

```text
ScopedPermissionGrant
  id
  company_id
  grantee_ref = ActorRef(agent, <standing-agent-id>)
  permission
  command_rules[]
    command_name
    subject_kind
    subject_ids[]                 # non-empty exact ids
    allowed_effect
    constraints                  # closed, command-specific fields
  assignment_id
  assignment_correlation_id
  grant_generation
  canonical_grant_digest
  valid_from
  expires_at?
  max_lease_ttl_seconds
  max_successful_dispatches = 1
  activation_approval_ref
  created_by / created_at
  display_metadata?                 # non-authorizing and outside the digest
  lifecycle_status                  # projection of append-only lifecycle events
```

Scope is an intersection, never a union of implied rights:

```text
company
AND StandingAgent
AND delivered Assignment/correlation
AND permission
AND command name
AND subject kind + exact subject id
AND allowed effect
AND command-specific constraints
AND remaining successful-dispatch budget
```

V1 permits no wildcard, prefix, regex, module-wide subject selector, nested
delegation, or permission inheritance. `constraints` is rejected unless the
server has a closed validator for that command. For
`work_item.transition`, it names exact allowed `from_status` and `to_status`
values plus the complete allowed payload patch. The grant cannot authorize its
own creation, activation, expansion, renewal, or revocation.

Each `(grant_id, grant_generation)` authority snapshot is immutable. The
canonical digest covers the Company, grantee, permission, command rules,
Assignment, correlation, validity window, expiry, lease TTL, successful-use
budget, and every other value that can change authorization. Any change to one
of those values creates a new grant id or a higher generation and immediately
fences every older unconsumed receipt. Append-only activation, revocation, and
expiry events project lifecycle status without rewriting the generation
snapshot. Revocation fences the named generation immediately and reserves any
later authority for a new generation; reactivation always creates a new
generation.

Only explicitly display-only metadata may change without rotation. It is
excluded from the canonical digest and must not influence authorization,
receipt contents, audit meaning, policy selection, or UI claims about effective
scope.

## One-command broker and target `CapabilityLeaseReceipt`

The target member-facing operation is one broker call:

```bash
harness company authority dispatch \
  --grant <scoped-permission-grant-id> \
  --assignment <company-assignment-id> \
  --action-command <action-command-json>
```

This command is available only through a Supervisor-bound member channel.
There are no flags for the root token, MemberRun identity, native-session id,
or Supervisor generation. The Company-side broker owns the root transport
credential, validates the request, dispatches at most one canonical
ActionCommand, consumes the internal lease, and returns a non-secret receipt.
It never returns, logs, or injects `HARNESS_COMPANY_OS_TOKEN`.

Every request returns one durable `CapabilityLeaseReceipt`, including a denial:

```text
CapabilityLeaseReceipt
  id / request_id
  company_id
  grant_id / grant_generation / canonical_grant_digest
  decision = allowed | denied
  standing_agent_ref
  agent_member_id
  member_run_id
  native_session_ref
  supervisor_id / supervisor_generation
  assignment_id / team_message_id / correlation_id
  action_command_id / command_name / subject_ref
  canonical_request_digest
  issued_at / expires_at
  consumed_at?
  outcome = executed | failed | denied | expired | revoked | indeterminate
  denial_code?
  action_command_ref?
  audit_event_refs[]
```

The receipt contains evidence, not a bearer credential. It cannot authorize a
second command, be exchanged for a token, be delegated, or refresh itself.
The broker chooses a TTL no greater than both `max_lease_ttl_seconds` and the
remaining grant lifetime.

## Issuance, use, expiry, and revocation

The broker validates in this order:

1. resolve the selected Company Store and an active grant whose immutable
   generation/digest exactly matches its Human R3 activation decision;
2. resolve the exact StandingAgent -> AgentMember -> unclosed MemberRun ->
   native-session chain and the current Supervisor generation;
3. resolve the exact Company Assignment -> delivered TeamMessage chain;
4. canonicalize and hash the ActionCommand, then intersect it with every grant
   scope dimension and the server-owned Action policy;
5. issue the short lease, revalidate grant and Supervisor generations
   immediately before dispatch, dispatch once, and consume the lease.

Expiry is checked at issuance and again before dispatch. An expired receipt has
no authority. A revocation event fences its exact generation and every
unconsumed receipt immediately; it never edits the snapshot in place. Scope
expansion or reactivation creates a new generation and requires a new Human
gate. Every other authority-bearing change, including reduced scope,
Assignment, correlation, validity, expiry, TTL, or successful-use budget,
follows the same rotation rule; callers never decide that a mutation is "safe
enough" in place.

V1 permits one successful dispatch for the whole grant generation, not one per
receipt. The broker reserves and consumes that use atomically with the
ActionCommand. Denied requests do not consume it; an indeterminate dispatch
holds it unavailable until reconciliation proves the terminal outcome.

Repeated delivery of the same `request_id` and identical ActionCommand returns
the existing receipt and ActionCommand outcome. Reusing either id with
different content is a conflict. A retry never creates a second effect.

## Denial and audit semantics

Default is deny. The target Store must preserve the broker receipt and an
append-only AuditEvent for both decisions:

- allowed dispatch retains the existing `PolicyAuthorized` plus
  `Executed` or `Failed` ActionCommand events;
- denied dispatch atomically records `ActionCommand.status=rejected`, a new
  `PolicyDenied` AuditEvent kind, and a denied receipt before returning.

This is an explicit target extension: `PolicyDenied` and durable
pre-authorization rejection are not implemented today. Stable denial codes
include inactive, expired, or revoked grant; identity or Supervisor-generation
mismatch; Assignment delivery mismatch; out-of-scope permission, command,
subject, effect, transition, or payload; Human gate required; protected effect;
successful-use budget exhausted; and idempotency conflict.

Audit detail includes the grant/generation, identity and Assignment refs,
canonical request digest, failed scope dimension, and policy ref. It contains
no root token, provider credential, lease secret, prompt, transcript, or
private thinking. A denial changes no business subject.

## Recovery and generation fencing

Grant generation, Team Supervisor generation, and ActionCommand idempotency are
independent fences:

- a higher Supervisor generation must reconcile the same StandingAgent,
  AgentMember, MemberRun, native session, Assignment, queued mail, and grant;
- an unconsumed receipt bound to an older Supervisor or grant generation is
  revoked, not replayed;
- after a crash, the broker first reads the receipt, ActionCommand, and
  AuditEvents by their stable ids;
- if dispatch outcome cannot be proven, the receipt becomes `indeterminate`
  and requires reconciliation. The broker never blindly resubmits it.

A compatible Supervisor restart does not create a new Company identity,
Assignment, MemberRun, or native session. An incompatible or closed session
requires a new execution binding and a new broker request; it does not rewrite
the durable grant or Assignment.

## Human authority gate

Creating or merging this contract, adding schemas, or deploying broker code
does not activate authority. Grant activation or scope expansion is a Company
`ChangePermission` effect at risk tier R3. It requires an in-scope,
unexpired, evidence-backed decision by the named active Human authority and a
durable activation Action/Audit trail. That decision binds the exact tuple
`grant_id + grant_generation + canonical_grant_digest`; it cannot activate a
different or later snapshot. The first live proof must use a grant created for
that proof; fixtures and repository state cannot silently make it active.

Revocation is fail-closed and may be performed immediately by an authorized
Human or emergency policy. The grantee cannot delay or approve its own
revocation.

## Concrete allow/deny example

Given an active Human-approved grant for Org Governance:

```text
permission: company.work.execute
command: work_item.transition
subject: work_item:work-agentos-org-role-permission-closure-v1
effect: transition_state
constraint: in_progress -> in_review
assignment: assignment-cli-1785418946013-p39030-0
correlation: work-agentos-org-role-permission-closure-v1
max successful dispatches: 1
payload patch:
  status: in_review
  result_document_ref: document-agentos-03-org-work-doc-loop
  result_record_refs: []
  evidence_refs: [team-run-1785417589241-p28630-0]
  artifact_refs: []
  deliverable_refs: []
  execution_refs: []
  outcome_summary: "Scoped Company authority proof is ready for AgentOS Lead review."
  completed_at: null
  updated_at: broker-generated current timestamp
```

The broker builds the target from the current WorkItem plus only this patch;
every unlisted field must remain byte-for-byte equal to the current record. The
literal result destination, evidence ref, outcome summary, empty provenance
arrays, and server-owned timestamp rule are part of the generation digest.
Client-chosen or additional payload values are not allowed.

The exact bound MemberRun may therefore transition this one WorkItem from
`in_progress` to `in_review` once. The same request with subject
`work-agentos-docs-information-architecture-v1` is denied as
`out_of_scope_subject`, writes no WorkItem version, and preserves the rejected
ActionCommand, `PolicyDenied` AuditEvent, and denied receipt.

Changing the allowed WorkItem payload—such as title, owner, assignees, source,
permission refs, result/evidence ids, outcome text, or adding any other
provenance—is denied as `out_of_scope_payload` even when the subject id and
transition match.

Acceptance must additionally deny Organization/permission mutation, Finance,
legal filing, credential, external-dispatch, and any other command/effect not
named by the grant.

## Non-goals

- no universal provider permission model or replacement for provider sandbox,
  approval, workspace, budget, or tool controls;
- no root token distribution, long-lived bearer token, or browser-stored
  Company capability;
- no authority inferred from a StandingAgent/AgentMember link, Assignment,
  TeamMessage delivery, runtime health, or provider completion alone;
- no Finance, legal, credential, Organization, permission, payment, or
  external-publication authority in the first Work-transition proof;
- no replacement for Human Approval, Action policy validation, non-broker
  Actor-permission checks, or idempotent Action dispatch;
- no schema/API/CLI/skill/plugin implementation or authority activation in this
  documentation change.

## Short implementation sequence

1. Add target schemas and Store operations for grants, receipts, rejected
   ActionCommands, `PolicyDenied`, generation fencing, and atomic receipt/audit
   writes.
2. Add the Supervisor-bound Company broker and one
   `harness company authority dispatch` client command without exposing the
   root token.
3. Implement only the one exact proof-specific `in_progress -> in_review`
   ActionCommand payload and Human R3 activation; add deterministic allow,
   unrelated-Work/payload/protected-effect deny, expiry, revocation, immutable
   generation, one-use, idempotency, and crash-recovery checks.
4. Run one real proof binding the exact StandingAgent, AgentMember, MemberRun,
   native session, Company Assignment, and delivered TeamMessage; review the
   native and Company audit evidence before expanding scope.

## Related decisions

- [ADR 0042](../decisions/0042-company-store-execution-space-project-binding.md)
- [ADR 0044](../decisions/0044-durable-team-supervision-and-typed-mail.md)
- [ADR 0045](../decisions/0045-company-owned-standing-agent-execution-relation.md)
- [ADR 0046](../decisions/0046-supervised-agentos-self-hosting-loop.md)
- [ADR 0047](../decisions/0047-scoped-company-authority-broker.md)
