# ADR 0062: Host-Owned Work Requires Exact Peer Acceptance

```text
status: accepted; implemented by DEV-102
date: 2026-08-26
amends: ADR 0037, ADR 0050, ADR 0057
canonical_for: Work acceptance authority when the accountable owner is Host
```

## Context

Host is an ordinary AgentMember with stronger Team permissions. The Work model
previously required Host acceptance for every Work, which made Host-owned Work
self-reviewed or permanently non-terminal. Adding a Reviewer table, nested
role hierarchy, or second review lifecycle would duplicate existing Team
identity and runtime authority.

## Decision

Ordinary Member-owned Work remains accepted only by the exact Team Host.
Host-owned Work cannot be accepted by its owner. It may be accepted by one exact
active non-owner AgentMember peer in the same Team and TeamRun. The peer must
have exactly one active TeamMembership on the Team's immutable Node and exactly
one active canonical MemberRun in the Work's TeamRun.

This is a narrow acceptance capability derived from AgentMember,
TeamMembership, MemberRun, TeamRun, and Work ownership. It is not a durable
Reviewer role and creates no second ledger. A peer cannot accept another
Member's Work. Revision requests for Host-owned Work use ordinary Work-linked
Messages; the existing request-changes mutation remains Host authority.

## Consequences

Owner self-acceptance continues to fail closed in Store authority. RoleView
projects the accept action only to an eligible active peer for Host-owned Work
and only to Host for ordinary Member Work. Provider completion, transport
receipt, CI, PR merge, or conversation never imply acceptance. Solo Hosts do not
receive a synthetic success path; Work stays in review unless an explicit
Human/Service trust action applies.
