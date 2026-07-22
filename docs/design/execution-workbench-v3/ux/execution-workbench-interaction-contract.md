# Execution Workbench interaction contract

Status: active implementation contract

The approved screenshots define visual states. This file defines behavior that
cannot be recovered from a still image. Mission, Agent Team, and Member Focus
keep one URL-addressable selection model and preserve Mission/Wave parent context
when moving into an execution attempt or member.

## Hotspots and navigation

| Object | Kind | Destination | Preserved context | Return behavior |
| --- | --- | --- | --- | --- |
| Mission rail item | link-like button | selected Mission | Workspace | Browser Back and `Missions` return |
| Wave node/card | link-like button | selected Wave within Mission | Mission | prior Mission scroll/selection when history permits |
| Agent Team attempt | link-like button | Team War Room | Mission + Wave | breadcrumb/back returns to selected Wave |
| Agent member portrait/name | link-like button | Member Focus | TeamRun + Mission + Wave | `Back to team`, breadcrumb, and Browser Back |
| Pending member decision | action link | Member Focus at pressure context | TeamRun + Mission + Wave | same as member navigation |

Member identity is clickable wherever it represents a real MemberRun. A purely
decorative portrait must not imply navigation. Clickable member rows use a
visible hover/focus treatment, an accessible `Open member <name>` label, and the
same `memberRun` deep-link contract.

## Scroll ownership

| Surface | Desktop | Tablet/mobile | Invariant |
| --- | --- | --- | --- |
| Mission detail | Mission document region | same document region; context becomes inline | body may remain locked only while this region is vertically scrollable and keyboard-focusable |
| Team War Room | activity/main region plus independently reachable context | primary stream with context sheet/inline section | composer never covers final activity |
| Member Focus | conversation/activity main | main stream with context sheet/inline section | header/composer remain reachable without nested ambiguous scrolling |

The Mission detail region owns `overflow-y:auto`, has an accessible region name,
and accepts keyboard focus. Longest representative Mission content must reach
the final Wave and final context module.

## Identity assets

Every Agent member receives a deterministic project-default portrait when no
role-specific portrait matches. The same identity string resolves to the same
portrait across Mission, Team, and Member views. Name, role, provider, and
status remain textual product truth; the portrait is presentational.

## Member activity composition

The compact Member Focus stream joins two different truths without merging
their storage: Harness Assignment/Handoff/decision rows and an on-demand read of
the bound Provider-native Session. The default key projection includes the
native opening message, latest meaningful tool invocation, and latest native
message. Member Focus opens on the complete chronological history by default;
`Focus key activity` is an optional compact lens, not a replacement for the
execution record. Tool payloads remain collapsed until requested so the full
history is readable without flooding the page. Each native row is labeled
`native session`, while loading, item count, and unavailable states are
explicit. A completed member with a resolvable native Session must not appear
as only an Assignment plus Result.

### V4 completed-history projection

- `Briefing`, `Exploration`, `Implementation`, `Verification`, and `Handoff`
  are read-time visual groups. They are not persisted provider phases and do
  not create a second execution plan.
- A native tool invocation and its adjacent generic tool-result record may
  render as one `NativeToolStep`; both source records remain reachable through
  its disclosure.
- Messages and Handoff render Markdown. Links accept only safe HTTP(S), local,
  or fragment destinations; provider-supplied HTML is never interpreted.
- The completed page opens on `Complete history`. `Focus` is an explicitly
  pressed toggle and preserves chronology when switched back.
- Loading the Harness snapshot or native session uses a skeleton/status state;
  it must never flash `Member run not found` before the first read settles.

### V4 browser journeys

| Id | Route/state | Actions | Assertions |
| --- | --- | --- | --- |
| `member-history-content-reachability` | completed MemberRun | focus history scroll owner; PageDown and End | final Handoff and composer remain reachable |
| `member-history-focus-toggle` | complete history | activate Focus, then Complete history | compact projection appears; returning restores all projected source records |
| `member-history-tool-disclosure` | complete history | open one tool step | paired source detail is keyboard reachable and no raw payload is shown before disclosure |
| `member-history-return-context` | Member Focus | Back to team, Browser Back | exact TeamRun/member/Mission/Wave selection survives |
| `member-history-responsive-context` | tablet/mobile | open Context & controls | context sheet opens without horizontal overflow or hiding composer |

## Motion and state

| Trigger | Feedback | Motion | Reduced motion |
| --- | --- | --- | --- |
| select Wave | selected node/card and updated context | short color/position transition | immediate state change |
| open member | hover/focus affordance, then route change | chevron/foreground transition only | no transform required |
| running activity | live trace/status pulse | bounded status animation | static status indicator |
| async action | pending disables duplicate action; result or error is explicit | opacity/color only | immediate feedback |

No motion represents fake progress, unobserved child lifecycle, or persisted
thinking.

## Required browser journeys

| Id | Route/actions | Assertions |
| --- | --- | --- |
| `mission-content-reachability` | open longest Mission; focus Mission detail; PageDown/scroll to end | one scroll owner exists and `scrollTop` advances toward `scrollHeight-clientHeight` |
| `mission-member-deep-link` | select current Wave; activate a member row | URL contains exact `surface=team`, `team`, `memberRun`, `mission`, and `wave`; Member Focus heading matches |
| `member-return-context` | from Member Focus use `Back to team`, breadcrumb, and Browser Back | returns to the originating Team/Mission context without selecting a different attempt |
| `member-keyboard-path` | Tab to member row and press Enter | same deep link and visible focus treatment as pointer activation |
| `execution-responsive-path` | repeat primary navigation at desktop/tablet/mobile | no horizontal overflow; context remains reachable; composer/action remains usable |
| `execution-reduced-motion` | emulate reduced motion and repeat selections | non-essential transforms/pulses are removed |

Source checks may ensure these controls exist, but P0 acceptance requires these
journeys to run against the browser with the stable native fixture or a named
live store snapshot.
