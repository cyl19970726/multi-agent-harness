# Implementation Plan — Persistent Claude Agent Team Member

Replaces the `claude -p`-per-delivery adapter with a persistent Agent SDK
member, and builds the executable form of ADR 0037's unmet acceptance items.

Context: ADR 0037 §Acceptance items 5 and 6 have no deterministic coverage
anywhere in the repo (`review_result` appears once in all of
`crates/harness-cli/tests`; `subagent` zero times). Those two items are exactly
the two P0s in the 2026-07-26 architecture review. The gap is causal: with no
executable acceptance for them, the runtime could diverge from the frozen model
while `pnpm check` stayed green.

Live evidence for every "Complete" below is in [FINDINGS.md](FINDINGS.md).

## Stage 1: Persistent member runtime (Node)
**Goal**: A member that survives an empty mailbox and is ended only by the Host.
**Success Criteria**: Deliver → lull → deliver produces two turns and zero
member-lifecycle events; `member_closed` only after an explicit `close`.
**Tests**: 9 deterministic cases (fake SDK) + live run (A1–A3).
**Status**: Complete — live-verified

## Stage 2: Gates — owned paths, plan approval, evidence
**Goal**: Turn three advisory constraints into provider-enforced ones.
**Success Criteria**: `PreToolUse` denies writes outside `ownedPaths`; denies
mutating tools until a correlated `plan_approval`; `PostToolUse` accumulates
`evidence_refs`.
**Tests**: Unit only. **No live `PreToolUse` denial has run** — the live runs
used `allowedTools: []`. See FINDINGS §Not verified.
**Status**: Complete (unit) / unverified (live)

## Stage 2.5: Desktop visibility
**Goal**: A harness-owned member is visible in Claude Desktop.
**Success Criteria**: `open "claude://resume?session=<id>"` imports the member's
native session; it appears in the desktop list as `local_<id>`.
**Tests**: A5, plus A6 for the post-import concurrency question.
**Status**: Complete — sequential access only; simultaneous writes untested

## Stage 3: Rust caller
**Goal**: `run_claude_team_member` spawns and drives this process instead of
looping `claude -p`.

The seam is three additive edits; `claude_cli` is not touched:

1. `crates/harness-cli/src/main.rs:7252` — add `("claude", "claude_agent_sdk")`
   to the `(provider, execution_mode)` allowlist.
2. `crates/harness-cli/src/main.rs:9354` — leave the existing
   `matches!(execution_mode, Some("claude_cli") | None)` branch alone; add a
   sibling branch for `Some("claude_agent_sdk")`.
3. New `run_claude_agent_sdk_team_member`: spawn
   `node apps/claude-member-runner/bin/claude-member-runner.mjs`, write
   `start` / `deliver` / `interrupt` / `close` as NDJSON on stdin, fold inbound
   events into the ledger. `session_bound` supplies `native_session_id`;
   `turn_complete` is a turn boundary, **not** a member lifecycle event.

**Success Criteria**: A deterministic test proving a message delivered *after* a
turn completes is consumed by the *same* MemberRun and native session — ADR 0037
acceptance item 6, currently uncovered anywhere.
**Tests**: `crates/harness-cli/tests/`.
**Status**: Not Started

## Stage 4: Acceptance for items 5 and 6, wired to `pnpm check`
**Goal**: Stop the class of drift, not just this instance.
**Success Criteria**: `review_result` acceptance and cross-Wave member
continuation are executable checks; a Skill or ADR claiming an unimplemented
capability fails docs-governance unless marked `target contract`.
**Status**: Not Started

## Stage 5: Live canary and version review
**Goal**: Promote `claude_agent_sdk` out of `review_required` honestly.
**Success Criteria**: A real mixed-provider TeamRun proves assignment, delivery
into a live member, interrupt with terminal acknowledgement, plan gate, and
resume; `harness member providers --fail-on-review` passes.
**Status**: Blocked — needs a Human to name and approve **Claude Code 2.1.220**.
AGENTS.md requires naming the provider and candidate version; the install
arrived with the dependency, and "可以升级" did not name a version.

---

## Waiting on a Human

1. **Approve Claude Code 2.1.220 by name**, or pin a different one.
2. **Commit** — Stage 1 / 2 / 2.5 are complete and green but uncommitted.
   Branch `codex/claude-member-runner-v1` off `origin/master` (5b1bae5).
3. **`docs/integration/claude.md` still documents the `-p` design as V1.** It
   should be rewritten around this runner, but it is a canonical doc behind
   `check:docs-governance`, so it was left untouched rather than edited without
   the ability to run the full gate.
4. **computer-use screenshot verification** was pre-authorized but needs the
   per-app approval dialog, which requires someone at the keyboard. MCP
   `list_sessions` was used instead and is sufficient evidence for A5.
