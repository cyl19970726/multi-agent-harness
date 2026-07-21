# Semantic icon refinement

Date: 2026-07-21

The first V3 implementation preserved the right information hierarchy but used low-contrast generic activity glyphs. This refinement introduces a coherent code-native SVG icon language for assignments, handoffs, runtime actions, evidence, reviews, decisions, blockers, live team presence, and context modules.

## Technical decision

Generated raster icons were intentionally rejected for operational controls. At the rendered 16–32px sizes they would be less deterministic, harder to theme, and less crisp than the repository's Lucide SVG system. Generated imagery remains appropriate for Agent portraits and decorative art; semantic controls use vector glyphs plus product-owned color surfaces.

## Acceptance

- Icons differ by shape and label, not color alone.
- Icon surfaces use existing semantic design tokens.
- No new runtime dependency was added.
- Desktop, tablet, and mobile capture completes without console errors or horizontal overflow.
- Mission blocked-member navigation still resolves the canonical MemberRun.
