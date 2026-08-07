# Team War Room Works interaction contract

## Context

- Parent object: one Mission-scoped `AgentTeamRun` with one shared Works board.
- Primary journey: understand pressure, find owned/unassigned Work, open one
  Work, inspect details, return without losing the Team/Wave context.
- Covered viewports: 1440×1000, 900×1180, 390×844, plus 320px overflow guard.

## Hotspots

| # | Object | Kind | Destination/action | Preserved context | Focus result |
| --- | --- | --- | --- | --- | --- |
| 1 | Works / Activity / Members | control | Switch the center workspace | team, mission, wave | selected tab heading |
| 2 | Work card | control | Open Work detail drawer/sheet | selected filters and parent route | Work title |
| 3 | Member portrait/name | link | Member Focus | team, mission, wave | member heading |
| 4 | Coordination pressure | control | Activity filtered to needs-attention | team and Work relation | first pressure row |
| 5 | Context & controls | control | Open responsive rail sheet | current tab and selected Work | sheet heading |
| 6 | Message team | control | Expand composer | current Work relation when selected | message field |

## Scroll owners

| Viewport | Region | Owner | Sticky/fixed elements | Reachability assertion |
| --- | --- | --- | --- | --- |
| 1440×1000 | center workspace | Team War Room main region | header, tab row, composer | final lane/card reachable by keyboard and pointer |
| 900×1180 | center workspace | Team War Room main region | compact context/composer bar | stacked status sections reach final Work without nested dead-end scroll |
| 390×844 | one primary flow | page main region | collapsed message/context toolbar + native bottom nav | two real Works visible before expansion; final Work reachable after scroll |

## State and motion

| Trigger | Pending/success/failure | Motion | Reduced motion |
| --- | --- | --- | --- |
| switch tab | selected underline and heading update | 120ms opacity/translate only | immediate |
| open Work | drawer/sheet with canonical latest version | 160ms slide | immediate display |
| change filter | board count and visible Works update | no fake progress | immediate |
| open context/composer | focus enters panel; Escape closes and restores | 160ms sheet/height | immediate |

## Browser journeys

| Id | Fixture/route | Actions | Assertions |
| --- | --- | --- | --- |
| `works-content-reachability` | Works default | focus main, PageDown to end | final Work is reachable; one vertical owner |
| `mobile-first-work` | 390×844 Works | load without interaction | at least one real Work card and primary status visible above sticky controls |
| `work-detail-return` | Works default | open Work, close, Browser Back where applicable | exact Work id visible; filters and Team context retained |
| `keyboard-work-detail` | Works default | Tab to Work, Enter, Escape | detail opens, closes, and restores focus |
| `responsive-context-sheet` | 900/390 | open Context & controls, close | rail facts reachable; no content occlusion or overflow |
| `reduced-motion` | every viewport | emulate reduced motion | non-essential transitions are disabled |
