# Company OS V2 visual language

## V2.2: Art-directed Workbench

V2.2 keeps the approved V2 information architecture and operating density. It
adds a coherent asset layer so the product feels authored rather than merely
assembled. The asset layer has three jobs:

1. **recognition** — actors, modules, WorkItem types, and pressure states are
   identifiable before their labels are read;
2. **orientation** — small visual anchors clarify where the user is and how the
   current object relates to the company;
3. **emotional durability** — restrained art, material, and motion make a
   long-session workbench feel calm, cared for, and worth returning to.

The V2.2 asset layer may enrich a layout, but it must not change object
semantics, conceal dense operational content, or turn a work surface into a
marketing page.

## Direction

The interface should feel like an exceptionally crafted AI-native company
workbench: editorial and calm like a premium knowledge tool, operationally
precise like a professional control surface, and alive through subtle Agent
presence. Workbench density and capability are desirable; generic admin chrome,
equal-weight containers, and visually exhausting density are not.

Beauty is part of productivity. The design should make people want to remain in
the workbench for long sessions by reducing cognitive friction, creating clear
visual rhythm, making actions feel responsive and intentional, and giving every
actor and object a recognisable place. Visual refinement must strengthen—not
remove—functional depth.

## Shared shell

- 1536 × 1024 desktop canvas for direction approval.
- 232–248 px warm stone sidebar with compact grouped navigation.
- 56 px top utility strip that recedes behind page content.
- main canvas uses page-specific composition rather than a universal card grid.
- optional 292–320 px Context Rail containing relationships, authority, and
  next actions; it never repeats the central narrative.
- coral/raspberry accent is used only for focus, primary action, and pressure.

## Surfaces and typography

- warm ivory application background, milk-white working surfaces, graphite
  text, muted stone secondary copy;
- restrained coral, sage, amber, and muted blue semantic accents;
- crisp sans-serif interface typography with an editorial display face or
  high-contrast weight for selected page titles;
- hairline borders, subtle material layering, small controlled shadows;
- radius hierarchy: compact controls 8–10 px, grouped surfaces 12–14 px,
  exceptional focus/decision surfaces 16 px;
- no purple gradient, glassmorphism, oversized pill collections, equal card
  grids, empty BI charts, or large decorative illustrations that compete with
  company work.

## Asset system

- **Actor portraits:** one canonical portrait per Human or Standing Agent;
  smaller appearances use a simplified crop and a role-colour ring.
- **Object iconography:** one scalable outline family for Document,
  BusinessModule, Milestone, WorkItem, Approval, Finance, Evidence, Relation,
  Mission, Wave, Agent Team, Workflow, Human, Standing Agent, and External.
- **WorkItem type glyphs:** development, design, research, content, legal,
  procurement, finance, operations, governance, Human Action, and general.
- **Module emblems:** distinctive editorial emblems for major business
  functions. They appear in headers, covers, and compact relationship cards.
- **Ambient art:** quiet line-work, engraved patterns, paper grain, and small
  object illustrations may occupy intentionally reserved negative space.
- **Motion:** hover lift, selection wash, progress draw, presence pulse, and
  relationship-line reveal. Motion never represents state that is not present
  in the underlying object.

Functional icons are implemented as a single vector icon family. Raster image
generation is reserved for portraits, module art, cover imagery, and visual
direction mockups; generated icon shapes are never the production icon source.

## Placement limits

- a P0 page gets at most one dominant art anchor and two supporting motifs in
  the initial viewport;
- tables and ledgers remain visually quiet; art belongs in headers, empty
  space, relationship summaries, and selected focus surfaces;
- visual assets may reinforce an existing state but never introduce a new
  status, metric, approval, payment, or authority claim;
- all decorative assets can be removed without losing the workflow or reading
  order.

## Page-specific composition

- Home reads top-to-bottom like a morning operating brief.
- Docs is a document and knowledge canvas, not a dashboard.
- Organization is a connected structural canvas with reporting lines.
- Lead Agent is a broad central collaboration stream with a working composer.
- Business Module is an authored domain page mixing narrative and live views.
- Work combines a compact Milestone ribbon/roadmap with a dense, readable
  WorkItem ledger and filters.

## UI truth

- Human, Standing Agent, External, and temporary MemberRun identity remain
  explicit.
- Milestone is not Wave; WorkItem is not WorkflowStep or Team message.
- Mission, Wave, Agent Team, and Workflow are one-time execution drill-ins.
- transient thinking may appear only as a small live preview and is never part
  of durable Activity.
- Commitment never implies Payment; pending Approval does not imply authority.
