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
**Goal**: `run_claude_team_member` no longer has to be the only Claude path.
**Done**: three additive edits plus one new driver; `claude_cli` is untouched.

- `parse_team_member_spec` accepts `claude/agent-sdk`
- `(provider, execution_mode)` allowlist gains `("claude", "claude_agent_sdk")`
- `team_member_provider_profile_for_mode` gains a `claude_agent_sdk` profile with
  `reviewed_provider_versions` deliberately **empty**, so the mode reports as
  review_required rather than silently compatible
- `run_claude_agent_sdk_team_member` spawns the Node runner and drives NDJSON;
  `record_member_round` is shared with the `claude_cli` path so the two modes
  cannot drift apart in what they write to the ledger

The behavioural change is the termination condition:

```rust
// claude_cli
if queued.is_empty() { break; }                    // member dies on an empty queue

// claude_agent_sdk
if queued.is_empty() && since.elapsed() >= grace { close }  // member outlives it
```

`HARNESS_CLAUDE_AGENT_SDK_IDLE_GRACE_MS` tunes the window (default 3s).

**Tests**: `crates/harness-cli/tests/claude_agent_sdk_member.rs`, 3 cases. The
load-bearing one is ADR 0037 acceptance item 6: the fake runner sends a
TeamMessage *after* emitting `turn_complete`, so "arrives once the queue was
already empty" is constructed rather than raced. Also verified live end to end
against Claude Haiku 4.5 through `harness team-run start`: 2 handoffs, second
round reports SECOND-ROUND, one native session (`user=2 assistant=2`).
**Status**: Complete

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

1. **Finish the canary.** Steer and interrupt now pass live (interrupt only
   after the fix in FINDINGS §E). Still unproven: a `PreToolUse` *block* against
   the real provider — the probe never got the model to attempt the gated write.
   Until that lands, `interaction_mode`, `plan_mode`, `supports_cancel` and
   `reviewed_provider_versions` stay unclaimed and
   `member providers --fail-on-review` keeps reporting the mode.
2. **Stage 4** — turn ADR 0037 items 5 and 6 into checks joined into
   `pnpm check`, so this class of drift cannot recur silently.
3. **Two stray TeamRuns** were written to the developer's real store while
   working out isolation (`team-run-1785086504477-p98466-0`, never started, and
   `team-run-1785087229492-p4132-0`, started once). Append-only; not rewritten.
4. **`ACTIVE_PROJECT` was changed** by a `harness init` here and restored with
   `harness project switch multi-agent-harness`. The prior value was ambiguous —
   `new-day-wanchengwanling` tied on `last_opened_at`. Re-switch if that was it.
