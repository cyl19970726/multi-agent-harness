# Team War Room V3 interaction contract

## Information and scroll ownership

Desktop has one scroll owner: the conversation stream. Team identity, mailbox
strip, filters and composer remain reachable. The context rail may scroll only
when its own content exceeds the viewport. Tablet turns context into a right
sheet. Mobile has one page stream plus a sticky composer; context and full
filters open in sheets and never create Chat/Activity tabs.

## Mailboxes

Mailboxes are read-model projections of TeamMessage recipients and delivery
records, not new persisted objects. Selecting a participant mailbox filters the
conversation to messages sent or received by that participant. Inbox and
Outbox affordances may narrow direction. `All activity` clears the participant
filter. Every selected filter is visible and keyboard removable.

Host is the Team Lead and has a mailbox projection but is not fabricated as a
MemberRun. Member-to-Host question, blocker and review-request messages surface
as Lead pressure. Queued, delivered and acknowledged remain distinct.

## Conversation and filters

The stream joins coordination messages, PendingInteraction pressure and honest
Harness actions in chronological order. Provider-native activity is loaded on
demand and labelled `native session`; it is never mirrored into Harness.

Every durable message has one identity route:

```text
[sender portrait]  Sender  →  [recipient portrait(s)] Recipient  [message type]
```

The timeline portrait is always the sender, never a generic activity icon.
Recipient portraits use the same deterministic identity mapping as Team
mailboxes and the context roster. Each message type has an adjacent semantic
icon, text label and restrained color treatment: Assignment, Broadcast,
Question, Answer, Progress, Blocker, Review Request, Review Decision, Plan
Request, Plan Proposal, Host Challenge, Plan Approval, Handoff, Tool Activity
and Evidence must remain visually distinguishable without reading the body.
Color is reinforcement; the type label and delivery text are required.

Participant, message-kind and text-search filters combine with AND semantics.
Empty results explain which filters are active and offer one clear reset.
Message bodies use the safe shared Markdown renderer. Large plan proposals and
handoffs show a useful preview and an explicit expansion control.

Avatars and participant names deep-link to Member Focus when a MemberRun
exists. Returning preserves Team, Mission, Wave and active filters. Host
identity is non-clickable unless a real Host detail surface exists.

## Reply and delivery

Selecting Reply puts the composer into an explicit correlation context. The
composer distinguishes a new message from a reply and never silently creates
an unrelated message. Sending, queued, delivered, acknowledged and failed
states have text labels in addition to color.

The Operator may send as Host only. The UI never impersonates a Member.
Member-originated messages come from the Member CLI/provider session.

## Keyboard and motion

- Tab order follows mailbox strip → filters → conversation actions → composer.
- Enter activates buttons; Escape clears reply context or closes a sheet.
- Focus rings are visible on every interactive element.
- New live rows use a subtle opacity/translate entrance only when motion is
  allowed. `prefers-reduced-motion` disables movement and pulsing.

## Required browser journeys

1. Select a Member mailbox; only sent/received rows remain; clear it.
2. Filter to plan messages, search text, and reset without losing the Team.
3. Expand a Markdown plan/handoff and verify headings, lists, code and links.
4. Open Member Focus from avatar/name and return with Mission/Wave context.
5. Reply to a correlated question and verify delivery/ACK text.
6. Reach the newest message and composer at desktop, tablet and mobile sizes.
