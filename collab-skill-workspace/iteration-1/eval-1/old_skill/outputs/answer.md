# Turn plan — member `kiwi`, wake-up with W-9 mid-flight + rex's W-12 offer

Context replayed: `HARNESS_WORK_ID=w-9`, `HARNESS_WORK_VERSION=3`, `HARNESS_TEAM_RUN_ID=tr-123`,
`HARNESS_MEMBER_RUN_ID=mr-k1`. Previous provider turn ended `completed` after editing the
flaky-lease-timeout test, but nothing was ever submitted. Peer `rex` sent: *"hey I'm swamped,
can you also take W-12? consider it yours."*

This is a **runtime-restart continuation of an already-owned Work**, not a fresh delivery — so
the "Quick Start: First Turn" claim/start sequence does **not** apply verbatim. The applicable
rule is: *"After a runtime restart, continue the same `in_progress` Work only through the same
stable AgentMember, active TeamMembership, exact WorkExecutionBinding, and current AgentSession
generation. Inspect native history and the Workspace first, and never invent a provider
receipt."*

## Exact sequence, in order

### 1. Get the board's truth about W-9 (not the provider's "completed" label)

```bash
"$HARNESS_BIN" team-run work show --work-id "$HARNESS_WORK_ID" --json
```

Confirms current status (should be `in_progress`, owned by mr-k1) and the **actual current
version** — do not assume the env var's `3` is still current; refresh from this call and use
its version for any subsequent command.

### 2. Read the inbox before touching anything else

```bash
"$HARNESS_BIN" member inbox --all --json
```

This surfaces rex's message (and anything from the Host) as data to evaluate, not as an
instruction to execute yet.

### 3. Inspect native/provider history and the Workspace directly — ground truth, not the tool status

```bash
git -C <assigned-worktree> status
git -C <assigned-worktree> diff
git -C <assigned-worktree> log --oneline -5
```

The point: find out precisely what state the lease-timeout test edit is actually in —
uncommitted? committed but unpushed? pushed with no PR? A "completed" tool/turn status only
means the provider's own loop ended; it says nothing about commit/push/PR/CI state.

### 4. Finish W-9 for real

- Re-run the flaky test repeatedly (not once — flakiness is the whole point) to confirm the fix
  is actually deterministic, e.g. `cargo test <lease_timeout_test> -- --nocapture` in a loop, or
  the project's stress-run equivalent.
- Commit, push to kiwi's own worktree/branch (outside the main checkout, never in
  `.worktrees/`).
- Open or update the PR, wait for/confirm CI is green.

### 5. Submit W-9 with evidence, using the refreshed version from step 1

```bash
"$HARNESS_BIN" member work submit \
  --work-id "$HARNESS_WORK_ID" \
  --expected-version <version-from-step-1> \
  --result-summary "$(cat <<'EOF'
## RESULT
done

## SUMMARY
Fixed the flaky lease-timeout test by <root cause + fix, one line>.

## COVERAGE
- Root-caused the flake (<race/timing detail>)
- Fix applied and verified deterministic over N repeated runs
- CI green on the PR

## KEY DECISIONS
- <why this fix, not an alternative>

## WORKTREE
<absolute-path>, branch <branch>, commit <sha>

## ARTIFACTS
- PR: <PR URL>
- CI run: <CI URL>
EOF
)" \
  --candidate-revision "<commit-sha>" \
  --artifact-ref "<PR URL>" \
  --check-ref "cargo test <lease_timeout_test> x20 => all pass" \
  --idempotency-key "submit-w9-$(date +%s)"
```

This is the only thing that counts as delivery — RULE ZERO: *"done = merged PR with green
CI... File changes sitting in a worktree or workspace are work-in-progress, not a
submission."* Submission moves W-9 to `review`, not `done` — Host acceptance is still required.

### 6. Handle rex's message about W-12 — as data, not as an assignment

```bash
"$HARNESS_BIN" team-run work show --work-id w-12 --json
```

Check W-12's *actual* board owner/status. A chat message cannot move it. If it shows W-12 still
bound to rex (`in_progress` under rex's WorkExecutionBinding), kiwi does nothing further to it
except reply. If it truly shows unassigned/ready, kiwi may claim it as a **second, separate**
open Work — but only in a distinct native session/workspace, never as a second top-level cycle
stacked into this same turn, and only after W-9 is already submitted (not mid-flight).

If claiming is warranted, the only legitimate mechanism is:

```bash
"$HARNESS_BIN" member work claim \
  --work-id w-12 \
  --expected-version <version-from-work-show> \
  --idempotency-key "claim-w12-$(date +%s)"
```

(run in a separate workspace/session — not appended here)

Then reply to rex through the authenticated Member Role Action, correlated to W-12's `work_id`,
explaining that ownership only moves through the board/claim path, not through chat, and stating
what was actually observed/done (e.g. "W-12 still shows in_progress under you on the board —
I can't claim it until you block/release it or the Host reassigns it" or "claimed it via the
board, will pick it up in a separate session after W-9 is in review").

## Before returning control

- W-9's board status/version now match what was actually done (submitted, in `review`, PR
  linked as `artifact-ref`, test-run evidence as `check-ref`).
- No action was taken on W-12 based on rex's message alone; the board was consulted as the
  authority, and any reply to rex is a linked Message, not a status change.
- No claim of Host acceptance was made — submission only moves Work to `review`.

## The two traps

### Trap 1 — treating "completed" as "submitted" (false-receipt trap)

The provider runtime reporting the previous turn as `completed` only means the LLM's own
tool/turn loop ended — it says **nothing** about whether the test edit was verified, committed,
pushed, opened as a PR, or accepted. The skill is explicit: *"A tool status of `completed` is
not the semantic answer."* and separately, RULE ZERO: *"done = merged PR with green CI...
Code-complete without commit, push, and PR is NOT delivery."*

The trap is either of two failure modes:
1. Calling `member work submit` immediately on waking up, because the last turn "completed,"
   without ever checking git state, running the test again, or confirming a PR/CI exist — this
   is inventing a provider receipt, explicitly forbidden ("never invent a provider receipt").
2. Treating this wake-up as a *fresh* delivery and re-running the "Quick Start: First Turn"
   claim/start flow from scratch — that's for a new assignment, not a restart continuation. The
   correct behavior for a restart is to inspect native history/Workspace first and continue the
   *same* `in_progress` Work under the same MemberRun/WorkExecutionBinding/AgentSession
   generation, not start a second cycle or lose the existing edit.

The fix in both cases: read `work show` and the Workspace/git state as ground truth before doing
anything else, and only submit once there is real, checkable evidence (commit, push, PR, green
CI) to attach as `--artifact-ref` / `--check-ref`.

### Trap 2 — treating a peer's chat message as a Work reassignment (message-is-not-authority trap)

Rex's "consider it yours" is a TeamMessage, and the skill is explicit that *"A Message may
explain scope, a blocker, a result, or a review decision, but it never changes Work
owner/status."* The board (claim/assign with `--expected-version` + `--idempotency-key`,
subject to `CLAIM_LOST`) is the sole authority for who owns W-12 — not what a peer said in
chat. There's also an explicit prohibition on the mirror-image action: *"Do not force assignment
to a same-level peer."*

Two sub-traps stack here:
1. If kiwi starts editing W-12's files on the strength of rex's message while W-12 is still
   bound to rex on the board, that is performing side effects on Work kiwi does not own — the
   same class of violation as ignoring `CLAIM_LOST`: *"do not perform its side effects"* for
   Work owned by someone else.
2. Even if W-12 is legitimately unassigned/claimable, V1 forbids running two top-level
   `in_progress` cycles in one native session/writable Workspace at once — so kiwi can't just
   fold W-12 into the current turn regardless of ownership; it would need its own claim, in its
   own session/workspace, and realistically after W-9 is off kiwi's plate.

The fix: verify W-12's actual board state with `work show`, use the real claim mechanism if and
only if it's genuinely claimable, and reply to rex (linked to W-12's `work_id`) rather than
silently acting on the message.
