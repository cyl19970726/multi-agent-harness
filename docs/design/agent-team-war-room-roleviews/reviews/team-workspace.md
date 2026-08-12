# Team Workspace review log

## Baseline first impression — `c0ab362a`

The page reads as a diagnostic stub, not an execution workbench. Roughly the
lower four-fifths of the desktop viewport is unused. The largest visual object
is a generic empty Work box, while Mission intent, run/Supervisor health,
pressure, navigation tabs and the next safe action are absent. Member identity
is reduced to ids plus `available/unknown`; Messages is an inert empty card.
The provenance strip is present but visually competes with the only primary
button.

Product-truth strengths:

- authenticated RoleView provenance is visible;
- the Team id and canonical members are not fabricated;
- zero Works/messages are stated honestly.

P0 gaps:

- no operational hierarchy or first-viewport pressure;
- no Works/Activity/Members navigation;
- no Mission Log, attempt, Supervisor/runtime or Lead context;
- empty state offers no useful, authorized next action;
- no selected Work/member context or responsive execution transformation.

This baseline is a regression reference only. The historical V4 War Room
contract supplies the approved hierarchy; old client joins and writers are not
part of the visual reference.

## First exact-candidate review — `92b2a845`

The first independent PM and real-user/operator passes both rejected the
candidate. PM reported P0=0, P1=5, P2=2; operator reported P0=0, P1=6, P2=2.
The overlapping P1 findings were that HostConsole replaced instead of composed
the shared workspace, Team route state was not URL/back-owned, the Work sheet
lacked a focus trap, empty/loading/error coverage was incomplete, and browser
evidence covered only the Host top view. The source review additionally found
a remaining global-snapshot TeamRun join and generated-at remount, normal Works
missing from the Host composer, noisy Host Inbox membership, and cross-Team
failed-delivery leakage.

That SHA is retained as rejected evidence. The corrective slice now composes
shared TeamWorkspace plus Host-only tools, removes the active-route snapshot
join/remount, scopes server projections, reuses canonical avatars and mature
Work/conversation/member components, and expands state and responsive browser
acceptance. Retired client authority and superseded coordination objects were
not restored.

## Second exact-candidate review — `f80409ca`

The second independent passes also rejected the candidate: PM reported P0=0,
P1=3, P2=2; operator reported P0=0, P1=5, P2=3. Blocking findings covered
historical-TeamRun Host action scoping, correlation-wide Inbox resolution,
screenshots stamped with the previous SHA, missing completed-run Close proof,
read-only/raw-id Activity, collapsed Work lifecycle lanes, implicit Host
membership in capacity, and incomplete selected-context/action evidence.

The next correction preserves the selected TeamRun route identity through
HostConsole and fails closed on mismatched action targets; resolves Inbox rows
only by exact causation; keeps Host separate from explicit Team membership;
restores the four canonical Work phase lanes with Assigned grouped inside
Open; adds same-surface authenticated
Activity reply/composer, filters, readable actors and portraits; and expands
evidence across empty mobile, Work details/actions, selected member, Member
home, and completed-run Close. Exact-SHA evidence is captured only after the
corrected source commit is frozen.

## Final review

The earlier engineering-complete and review-ready conclusions are superseded.
User comparison against the approved concepts found a visual P0: the candidate
kept a generic administration-dashboard composition, excessive bordered panels,
weak first-screen density and insufficient chat dominance. Passing product
checks, screenshot capture and interaction checks did not prove visual parity.

The replacement visual contract and immutable rejected/corrected candidate
families live in `../visual-rebaseline-v2.md`, `../expected-v2/` and
`../expected-v3/`.

### Final visual acceptance — `bbcf3df2`

An independent visual reviewer inspected the approved expected-v3/v3.1 frames
and the exact browser evidence under
`.visual-evidence/agent-team-war-room-roleviews/final-exact/` at desktop,
tablet, 390 px and 320 px. Earlier P0/P1 findings were corrected rather than
waived:

- Works uses content-height phase regions and a mobile Priority Work instead
  of full-height empty lane panels;
- Activity is a continuous typed record stream;
- Agent Conversation gives the center canvas majority width, keeps Current
  Work visible on mobile and demotes provider-native facts below conversation;
- Host Console shows Lead Inbox pressure followed immediately by Current
  Decision at every viewport while retaining the desktop decision cockpit;
- Member Home is identity-led and separates AgentMember, MemberRun, native
  session, Workspace and Work responsibility;
- Members uses a continuous roster and preserves AgentMember/MemberRun/native
  separation.

The reviewer reported visual P0=0 and P1=0 and set
`visual_fidelity=pass`. Minor generated-image asset, typography and color
differences remain governed by `../expected-v3/README.md`; they do not override
RoleView data or justify expected-image replacement.

The accepted source additionally passed:

- deterministic product, authority, accessibility and responsive gates;
- viewport-matched implemented evidence at desktop, tablet, mobile and 320 px;
- explicit reference/expected/implemented comparison by an independent visual
  reviewer who inspected the named files rather than screenshot existence;
- independent PM and real-user/operator review of that same SHA;
- visual and product P0/P1 findings reduced to zero without overwriting the
  approved expected images to make a diff pass.

### Exact candidate closeout — `387a2992`

The final runtime candidate preserves every accepted visual correction above
and closes the remaining product-model findings without treating the generated
concept frames as pixel-exact assets:

- responsive `Attention preview` is explicitly display ordering and the same
  Work remains present in its canonical Open / Active / Review / Closed phase;
- Work detail renders phase, condition and Closed-only resolution as independent
  facts;
- Host conversation labels consistently use the explicit Host Agent identity;
- Member Home evidence now covers desktop, tablet, mobile and 320 px.

The candidate passed the full Dashboard gate, the focused Team War Room browser
gate at all required viewports, and the RoleView Rust tests. A clean exact-SHA
recapture is versioned under `../implemented/`; every run waits for the complete
`frontend 387a29922b5883dade528845f0ff2ef913754ee3` provenance string before
capturing.

Independent final reviews of that exact runtime SHA reported:

- PM / product logic: PASS, P0=0, P1=0, P2=0;
- real-user / operator usability: PASS, P0=0, P1=0, P2=1;
- visual reviewer: `visual_fidelity=pass`, P0=0, P1=0.

The non-blocking operator P2 asks for an additional deep-scroll Member Home
capture showing the narrow-screen My Work row. Current 390 px and 320 px runs
already assert no document overflow and show a stable identity-first first
screen; no observed defect was waived. Additional visual refinements remain
recorded as P2 rather than silently changing the approved expected frames:
compact the 320 px Attention caption, remove the unused final Work-detail fact
cell, optionally add a direct My Work jump, and continue small mobile Activity
density and portrait-temperature refinement.

The CI follow-up candidate `e3549544` additionally narrows evolving ledger
actor objects to closed RoleView `kind` / `id` references and updates Store-live
browser waits to the shipped Team surface and accessible disabled-reason
contract. It changes no visual composition or product hierarchy. The full live
five-view check then passed 31/31. Visual evidence was recaptured with the exact
new runtime provenance; the independent `387a2992` product and visual findings
therefore remain applicable to this non-visual projection-boundary fix.

Because this review and the implemented screenshots are evidence-only changes
after the runtime source, `e3549544` is the final source-of-runtime authority
named by the visual contract.
