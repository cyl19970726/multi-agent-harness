# Member Run Focus — completed history V4

## Intent

Turn Member Focus from a colored event log into a premium execution narrative.
The complete provider-native history remains present, but the interface groups it
into readable phases and gives messages, tool steps, verification, and handoff
different visual grammar.

## Image-generation prompt

Create a high-fidelity 1536×1024 desktop product mockup for the Star Harness
Agent Member detail page. Use the current Member Activity screenshot only as a
content/problem reference; redesign the complete page.

- Use a slim icon navigation rail, a continuous main work surface, and a
  flexible right context rail.
- Header: illustrated member portrait, `WorkspaceFixer`, role, completed state,
  `Codex · gpt-5.6-sol`, and a quiet `Back to team` action.
- Main narrative: `Work history`, with `Complete history` and `Focus` controls.
  Divide the work into `Briefing`, `Exploration`, `Implementation`,
  `Verification`, and `Handoff`.
- Render Lead and member messages as editorial conversation blocks. Render
  Markdown headings, lists, code, and links with a strong typographic hierarchy.
- Combine a tool call and its result into one compact execution row. Use distinct
  crafted monoline icons for spawn/team, command, file/edit, search, wait, and
  check. Show status, duration, and a concise summary; keep raw payload behind a
  disclosure.
- Make Handoff the visual culmination with rendered `RESULT`, `SUMMARY`,
  `FILES`, and `CHECKS`, artifact chips, and a low-noise linked-correlation
  control.
- Right rail: compact Agent Team, Mission/Wave progress, Runtime, and Artifacts.
- Bottom composer should feel integrated and calm in a completed read-only run.
- Visual language: warm ivory paper, white content, charcoal text, warm coral
  product accent, ink blue execution, emerald success, muted amber attention,
  thin dividers, almost no shadows, high density with clear rhythm.
- Avoid glassmorphism, neon, glossy 3D, dominant purple, oversized cards,
  dashboard grids, giant padding, heavy gradients, repeated terminal icons, and
  browser/device chrome.

## Implementation contract extracted from the concept

1. Phase grouping is a projection over native history, not new persisted truth.
2. Tool call/result pairing must preserve access to both native records.
3. Handoff Markdown is rendered from the Harness message while provider-native
   final output remains separately attributable.
4. Right-rail modules reuse TeamRun, Mission/Wave, native session, workspace,
   and artifact projections; they do not duplicate storage.
5. Decorative color never replaces status text or semantic icons.
