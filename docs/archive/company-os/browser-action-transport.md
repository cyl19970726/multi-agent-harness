# Company OS browser Action transport

```text
status: first implemented slice
scope: Approval Focus -> approval.decide
transport: same-origin browser request -> governed Company OS Action API
credential: session-memory operator capability
```

## Implemented contract

Approval Focus may dispatch `approval.decide` only when the Dashboard source is
an authority-labelled Store projection, its projection contains the declared
action and server policy reference, the Approval remains `requested` with a
named Human approver, and the operator supplies a transport capability plus a
durable decision note.

The capability is held only in React memory, rendered as a password input, sent
in `X-Harness-Company-OS-Token`, cleared after success, and omitted from capture
manifests and checked-in evidence. Prototype pages cannot enable the action.

The browser does not append an Approval directly. It sends a requested
`ActionCommand(command_name=approval.decide)`. The server stays authoritative
for actor status, permission, policy shape, declaration/module scope, named
approver, expiry, state transition, idempotency, and audit reservation.

The two subjects remain distinct:

```text
ActionCommand.subject_ref = the Approval being decided
Approval.subject_ref       = the governed Work/financial/business object
```

Conflating these references is rejected by the server.

## Honest identity boundary

This first slice proves capability-authenticated local operator control and
durable attribution to the named Human actor. It does **not** prove an
independent Human login, phishing-resistant authentication, actor-bound session
credential, or remote multi-user authorization. A holder of the current global
transport capability can submit a command claiming an eligible actor, after
which the server validates that actor against stored authority policy.

Before remote or multi-user deployment, replace the manually entered transport
capability with a server-issued authenticated session bound to one `ActorRef`,
CSRF/origin protection, expiry/revocation, and step-up authentication for high
risk decisions. The client must then take `requested_by` from that session,
not from the record it is deciding.

## Deliberate exclusions

- `Request changes` remains disabled because `ApprovalStatus` has no native
  request-changes state and no follow-up WorkItem contract yet.
- Approving the Approval does not mutate the linked Commitment. A later
  `commitment.append` transition must consume the approved decision separately.
- No Payment is created, prepared, settled, or implied.
- Governance proposals, organization mutation, and Standing Agent messaging
  remain outside this slice.

## Acceptance evidence

[`../design/company-os-v2/approval-action-v1/review.html`](../design/company-os-v2/approval-action-v1/review.html)
shows requested, invalid-capability denial, approved, and rejected states from
isolated Store runs. Its manifest records the executed ActionCommands,
authorization/execution audits, idempotent replay, unchanged pending Commitment,
and zero Payments. Local archived stores remain under `.visual-evidence/`.
