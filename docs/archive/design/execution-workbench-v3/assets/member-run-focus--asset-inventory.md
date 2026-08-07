# MemberRun Focus V4 asset inventory

| Asset | Purpose | Implementation |
| --- | --- | --- |
| Eight member portraits | Stable execution and Organization identity | generated 512px editorial portrait set under `apps/agent-dashboard/src/assets/agent-members/avatars/`, with deterministic text-keyed fallback |
| Portrait contact sheet | Human review of the complete identity set | `agent-portrait-set-v1.png` (documentation only) |
| Semantic activity nodes | distinguish assignment, action, file, transient, evidence, review | code-native lucide icons and status tones |
| Timeline spine | continuous execution narrative | CSS line with responsive collapse |
| Live-only preview | transient provider activity | lavender bounded activity surface with expiry label |
| Gate readiness | parent Wave context, not member progress | shared accessible progress primitive |
| Team portraits | parent attempt orientation | shared execution portrait stack |
| Composer send control | direct TeamMessage action | existing typed action transport and pending/disabled state |

The portrait set is a runtime product asset. The generated page image remains a
layout reference, not a runtime UI texture. Semantic activity icons remain
code-native so state, accessibility, and future provider tools are maintainable.
