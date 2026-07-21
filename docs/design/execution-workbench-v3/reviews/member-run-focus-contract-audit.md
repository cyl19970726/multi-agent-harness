# MemberRun Focus contract audit

Date: 2026-07-21

## Finding

MemberRun Focus already exists as a truthful independent route. The next slice
is a V3 visual and acceptance upgrade, not a new object model or page rewrite.

Implemented today:

- URL-addressable `memberRun` selection beneath one AgentTeamRun;
- native Mission, Wave, attempt, MemberRun, assignment-correlation, message,
  action, event, delegation, evidence, session, and live-preview joins;
- one continuous activity/conversation stream;
- direct member composer through TeamMessage transport;
- Wave, Team, Assignment, Outputs, Runtime, and Delegations context modules;
- explicit finished, missing, empty, unavailable, and transient-thinking
  boundaries.

## Gaps

1. The page still uses the quieter Workbench V2 visual density rather than the
   V3 semantic node-first timeline and execution identity hierarchy.
2. The first viewport does not make assignment, live work, evidence, and review
   pressure feel like one deliberate work story.
3. There is no MemberRun-specific deterministic visual fixture assertion or V3
   expected/actual comparison.
4. Tablet/mobile context ordering is specified but not independently captured
   against a V3 expected image.
5. The shared shell still contains one `Compatibility Team Run` fallback label;
   this must become neutral missing-context copy rather than an active product
   compatibility promise.

## Truth boundary

- Ownership: assignment TeamMessage plus correlation id.
- Durable work: MemberAction, TeamRunEvent, messages, artifacts, checks, and
  explicit outcome summaries.
- Live-only work: sanitized `member_activity` with expiry; never evidence.
- Runtime: provider/model/session/worktree facts only when present.
- MemberRun is run-scoped and never projected into Standing Agent identity.
- Member completion never accepts the parent Wave.

## Selected information hierarchy

1. Member identity and back-to-Team path.
2. Assignment anchor.
3. One semantic chronological stream: assignment, acknowledgement, action,
   files, transient preview, evidence, pressure/review.
4. Sticky direct-member composer.
5. Context ordered Wave, Team attempt, Assignment, Outputs, Runtime,
   Delegations; mobile moves Needs attention before Assignment and Wave.

This hierarchy is now the input to the V3 expected-image and implementation
Waves.
