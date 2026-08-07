# MemberRun Focus V4 implementation review

Status: approved by the user on 2026-07-22; product truth and visual fidelity
pass with the documented data-truth deviations below.

## What now passes

- The desktop page uses an 80px focus rail and removes the global TopBar from
  Member Focus, matching the approved single-member composition.
- The hero uses a stable generated portrait, large identity treatment, real
  provider/model, run status, and an explicit return to the parent Team.
- Complete history is the default. Harness coordination and the provider-native
  session are joined only on read; 49 real source records are available in the
  captured run.
- Briefing, Exploration, Implementation, Verification, and Handoff are a
  read-time editorial projection. They are never stored as coordination truth.
- Repeated tool start/result records are grouped by native tool family. Every
  underlying message and tool record remains reachable from the phase
  disclosure.
- The right rail is a stable set of Team, Wave, Runtime, and artifact/evidence
  summaries rather than another activity log.
- The completed run truthfully renders a read-only composer; active runs retain
  real message/steer/interrupt behavior.
- No thinking is persisted or replayed. Native transcript and tool truth remain
  in the provider session.

## Intentional deviations from the generated expected

- The expected image contains illustrative filenames, durations, token totals,
  three artifacts, and a placeholder collaborator. The actual page does not
  invent those facts: this run has two real members and no linked artifact
  references at MemberRun scope.
- The expected depicts an active send control. The captured real MemberRun is
  completed, so its composer is read-only.
- Native Codex records expose tool families such as `spawn_agent`,
  `send_message`, and `wait`; they do not expose the expected image's fabricated
  file-operation rows. The implementation groups what the provider actually
  recorded.
- The portrait comes from the approved reusable eight-avatar product set, not a
  crop copied from the generated page image.

## Evidence

- Expected: `../expected/member-run-focus/completed-history--desktop-concept-v4.png`
- Actual: `../implemented/member-run-focus/member-run-focus--completed-history--desktop-1536x1024.png`
- Capture manifest: `../implemented/member-run-focus/capture-run.json`
- Three-way comparison: `../comparisons/member-run-focus/completed-history--desktop-1536x1024.png`
- Exact-size overlay: `../overlays/member-run-focus/completed-history--desktop-1536x1024.png`
- Automated contract: `npx pnpm@9.15.4 check:dashboard`

The product-truth and desktop visual-fidelity gates pass. Responsive V4
expected designs should now inherit this approved warm-editorial language;
tablet/mobile implementation should still wait for those responsive references
rather than extrapolating silently.
