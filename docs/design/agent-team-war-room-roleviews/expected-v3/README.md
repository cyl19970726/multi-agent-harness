# Expected v3 candidate set

This directory is the non-destructive revision of `../expected-v2/` after the
independent visual reviewer rejected the v2 set. The v2 files remain immutable
evidence of the rejected direction.

V3 specifically corrects the visual P0/P1 findings:

- Works replaces full-height outlined swimlanes with typographic phase regions,
  denser cards and explicit owner/evidence/next-decision footers;
- Activity becomes a continuous operational record stream with one attention
  row visually linked to Lead Inbox;
- Agent Conversation makes the center canvas dominant, reduces native-source
  color weight and consolidates the right rail into Decision Context;
- Host Console makes Lead Inbox plus one Current Decision the primary
  relationship and demotes runtime/provenance to flat factual rows;
- mobile makes one Priority Work primary, collapses other phases, flattens the
  member roster and starts conversation near the top under Current Work.

Members and Member Home carry forward the two v2 frames that the visual
reviewer allowed to freeze. Their v3 filenames make the seven-page approved
candidate family explicit without overwriting the source files.

| Candidate | Source | Status |
| --- | --- | --- |
| `team-workspace-works-desktop-v3.1.png` | surgical semantic correction of v3 | frozen visual direction |
| `team-activity-desktop-v3.1.png` | complete family navigation restored | frozen visual direction |
| `team-members-desktop-v3.png` | frozen from v2 | aesthetically approved with P2 asset refinements |
| `agent-conversation-desktop-v3.png` | new v3 revision | frozen visual direction |
| `host-console-desktop-v3.png` | new v3 revision | frozen visual direction |
| `member-home-desktop-v3.png` | frozen from v2 | aesthetically approved with P2 asset refinements |
| `team-workspace-mobile-family-v3.1.png` | Phase and AgentMember/MemberRun hierarchy corrected | frozen visual direction |

These files remain design intent, not browser evidence. Final normalized
viewport expectations and implemented comparisons must be versioned
separately and bound to the exact source SHA.

## Image/spec boundary

Generated frames are compositional references, not pixel-perfect fixtures.
Implementation must preserve their macro layout, focus hierarchy, surface
depth, density and core model semantics. Minor generated-text, portrait,
lighting, icon and asset inconsistencies are corrected from the written spec
rather than causing repeated image regeneration.

The v3.1 images were regenerated only because the issues could misdirect the
product implementation: Accepted appeared as a Review state, Activity omitted
the Works family tab, mobile Priority Work omitted Phase, and mobile rows mixed
AgentMember identity with MemberRun instance type. Remaining P2 refinements are
implementation requirements:

- one paper/background token across every page;
- sans for metadata, labels, numeric facts and controls; editorial serif is
  limited to page titles and record titles where the page calls for it;
- portraits share crop, color temperature and contrast where real assets allow;
- coral is reserved for current focus and the next primary decision;
- semantic blue, purple, green, amber and red are muted and never become a tag
  collection;
- selected state uses a warm wash plus slim marker, not a large coral outline;
- text and exact ids shown inside generated frames never override RoleView data.
