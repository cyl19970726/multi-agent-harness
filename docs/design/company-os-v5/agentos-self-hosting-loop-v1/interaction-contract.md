# Interaction contract

## Global context

- Every Company OS internal link preserves `company`, `space`, `project`, and
  `api` when present.
- A link must not switch from Store-live data to fixture fallback.
- Loading, empty, failed, and unavailable states are visually distinct.

## Organization

- Selecting a durable Agent opens its Standing Agent workspace.
- Org hierarchy shows reporting/management structure, not runtime nesting.
- Runtime and Agent Team participation are linked evidence, never child
  organization nodes.
- Creating an Agent/unit remains visibly unavailable until the governed
  OrgChange transport exists.

## Standing Agent workspace

- The composer is enabled only when the StandingAgent has an explicit
  execution AgentMember link and an approved Inbox transport.
- Sending shows queued, delivered, provider-received, replied, and failed as
  distinct states.
- Busy delivery never starts a second top-level writable turn.
- Offline delivery remains queued and exposes recover/resume ownership.
- Work, Docs, permissions, runtime state, and Inbox are separately labelled.

## Work

- Every list, board card, milestone row, and table row opens
  `?surface=work&workItem=<id>`.
- The selected WorkItem page renders only refs that resolve from that WorkItem.
  No Approval, finance, typed-record, or Actor fallback may use the first
  snapshot row.
- Reviewer labels are role-neutral unless a finance/legal role is explicit.
- `New work` looks disabled until a governed create transport is connected.

## Docs

- Document properties distinguish creator, last maintainer, and declared owner.
- Related participants come only from explicit document refs or linked work.
- `Ask an agent` is disabled until a target, permission, and transport are
  available.
- Structure trees do not mix unrelated business areas under the selected
  AgentOS root.

## Motion

- Use 120–180 ms opacity/color/translate transitions for hover, expansion, and
  delivery acknowledgement.
- Respect `prefers-reduced-motion`.
- Do not animate lifecycle state optimistically before Store acknowledgement.
