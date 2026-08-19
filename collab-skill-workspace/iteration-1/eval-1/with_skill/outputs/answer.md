# Turn plan — MEMBER kiwi, waking on w-9

Envelope for this turn: `HARNESS_WORK_ID=w-9`, `HARNESS_WORK_VERSION=3`,
`HARNESS_TEAM_RUN_ID=tr-123`, `HARNESS_MEMBER_RUN_ID=mr-k1`. I use these exact
values and `$HARNESS_BIN` — never a guessed identity or another binary.

## Ordered actions this turn

### 1. Re-establish ground truth before any side effect (read-only)

The previous provider turn ending "completed" is a **provider-turn-lifecycle
fact**, not a Work-module fact — nothing was submitted, so I do not know the
real current phase/version of w-9 until I ask the board. I also don't trust
the env var's version blindly for a mutation later; I re-read it fresh.

```bash
"$HARNESS_BIN" team-run work show --work-id "$HARNESS_WORK_ID" --json
"$HARNESS_BIN" member inbox --all --json
"$HARNESS_BIN" team-run board-summary --id "$HARNESS_TEAM_RUN_ID"
```

`inbox --all` surfaces rex's message plus anything from the Host, with
correlation ids. `board-summary` shows W-12's actual owner/phase and every
member's idle/working/awaiting-review state — not rex's private claim about
it.

### 2. Recover what the previous turn actually did

Work ownership and the Workspace persist across restarts (shared invariant
§9) — I resume the *same* MemberRun/native session/Workspace, I don't start
fresh and I don't invent a provider receipt for what "completed" supposedly
did.

```bash
git -C <workspace-path> status
git -C <workspace-path> diff
git -C <workspace-path> log --oneline -5
```

This tells me exactly what changed in the test file, and whether it was
committed — independent of what the provider status field claims.

### 3. Resume w-9 correctly, using the version from step 1 (not the stale env var)

- If `work show` says phase is still `open` (never actually started):

```bash
"$HARNESS_BIN" member work start \
  --work-id "$HARNESS_WORK_ID" \
  --expected-version <version-from-step-1> \
  --idempotency-key "start-$(date +%s)"
```

- If it's already `active`, skip `start` — it's idempotent state, not a
  ritual — and just continue in the same Workspace/native session.

### 4. Verify the fix for real, then finish delivery mechanics

- Review the existing edit to the lease-timeout test.
- Run the flaky test repeatedly (e.g. 20–50x in a loop) to produce an actual
  check-ref, since a single green run doesn't disprove flakiness.
- Commit in my own worktree **outside** the main checkout (never
  `.worktrees/` or the shared checkout), push, open a PR if the Work's gates
  require one.

### 5. Handle rex's "consider it yours" for W-12 — without claiming or starting it inline

```bash
"$HARNESS_BIN" team-run work show --work-id w-12 --json
```

Then, regardless of what that shows:

- I do **not** run `member work claim` or `member work start` on w-12 from
  this message alone.
- I do **not** silently ignore rex either — I reply on the same
  Work-linked/peer conversation explaining that a chat message cannot
  transfer ownership, and that I'm already at capacity (one active
  `in_progress` Work = w-9; V1 forbids a second top-level cycle in this
  MemberRun/Workspace).
- If W-12 genuinely needs a new owner, that requires an explicit Work
  operation — either I `member work claim` it *after* w-9 is out of
  `active`, or the Host reassigns/rebinds it, or rex releases it back to
  `team_claim`. I optionally send the Host one decision-shaped message
  flagging that rex is overloaded on W-12, with a recommendation, rather
  than acting on the peer's informal handoff myself.

### 6. Submit w-9 once the fix is verified, with evidence

```bash
"$HARNESS_BIN" member work submit \
  --work-id "$HARNESS_WORK_ID" \
  --expected-version <latest-version-after-step-3> \
  --result-summary "## RESULT
done

## SUMMARY
Fixed lease-timeout test flakiness in <test file>.

## COVERAGE
- root cause identified and addressed
- test run 30x locally with 0 failures

## KEY DECISIONS
- <e.g. widened timeout tolerance / removed real-clock dependency>

## WORKTREE
<absolute path>, branch <name>, commit <hash>

## ARTIFACTS
- PR URL: <url>
- CI run URL: <url>" \
  --candidate-revision <exact-revision> \
  --artifact-ref <PR-URL> \
  --check-ref "<test command run 30x>: 0 failures" \
  --idempotency-key "submit-$(date +%s)"
```

This moves w-9 to `review`. It is **not** done — only explicit Host
acceptance moves it to `done`.

### 7. Before returning control

Confirm: the latest Work version/status for w-9 matches what I actually did;
rex's W-12 message was answered with a Message, not acted on as an
assignment; no duplicate side effects; native session remains the sole
transcript truth; MemberRun stays available for `request-changes`.

---

## The two traps

### Trap 1 — "Provider completed" is not "Work done"

The runtime status field from the previous turn ("completed" after editing
the test file) describes the **provider's turn lifecycle**, not the
**Work module's** state. Per the skill: *"Provider 'completed' is not Work
'done.' Submission moves Work to review; only explicit Host acceptance moves
it to done. A green fixture, a delivery receipt, or a provider completion
status alone is never acceptance."* Nothing was ever `submit`ted last turn,
so w-9 is still wherever it was before (open or active) — the edit is
uncommitted/unsubmitted work-in-progress at best.

The failure mode this sets up is two-sided: either (a) I treat the edit as
finished and stop, silently leaving w-9 stuck with no submission and the
Host seeing nothing (the "silent stall" anti-pattern), or (b) I distrust the
status and restart the whole Work from scratch, discarding a good edit and
duplicating effort, when Work ownership and the Workspace are guaranteed to
survive restarts (shared invariant §9 — "never clear ownership, duplicate
side effects, or reconstruct a session from Harness messages after a
crash"). The correct move is the middle path taken above: inspect the actual
workspace/git state to recover what was really done, verify it with a real
check (run the test repeatedly), then carry it through commit → push → PR →
`work submit` with `--artifact-ref`/`--check-ref` as the real evidence.

### Trap 2 — Assignment never travels by message

Rex's "consider it yours" for W-12 is an ordinary peer chat message. Per the
skill's shared hard invariant #1 and #4: *"There is no Assignment Message
compatibility path... TeamMessage is conversation only; never treat a
Message as responsibility, ownership, or status."* And explicitly listed as
an anti-pattern: *"Assignment by message. 'Please take W-7' in chat changes
nothing and desynchronizes the board."* No matter how much authority rex's
phrasing implies ("consider it yours"), it has zero effect on W-12's actual
owner/phase in the Works board.

Two compounding hazards if I acted on it directly: first, I'd be performing
side effects on a Work I don't provably own, which the board and the Host
would not agree with (desync). Second, even if W-12 *were* properly
reassigned to me, V1 permits only one active `in_progress` Work per Member,
and w-9 already occupies that slot in this exact MemberRun/native
session/Workspace — I must not start two top-level cycles concurrently. The
correct move is to check W-12's real state via `work show`, decline to
claim/start it from the message alone, and respond to rex (and optionally
the Host) making the ownership mechanism explicit — claim/reassign is a
Work-module operation, not a favor granted in chat.
