# Host runbook: `--json` flag on `harness export`, driven from `builders` (tr-123)

Role: Host (TeamMembership role=Host on Team `builders`, team-run `tr-123`).
Roster: Member `kiwi` (Kimi ACP, member-run `mr-k1`, currently idle).
Skill followed: `collaborate-as-agent-team-member` (Part I shared model, Part II
Host loop, `shared-references` hard invariants). No polling loops, no
assignment-by-message, no accepting on provider-completion alone.

---

## 0. Pre-flight (before touching the board)

I don't re-decompose the roster or worktrees — the Team already exists and
`kiwi` is already an Active membership — but I do confirm the things the Host
loop requires me to know before I create Work:

- `kiwi` runs `kimi_acp`, one of the three executable Team modes, and its
  permission ceiling was frozen at session start — good enough for editing a
  CLI crate and running `cargo test`.
- `kiwi` has (or I set) a disjoint owned path / worktree covering the CLI
  crate, e.g. `--member-owned-path kiwi:crates/firm-cli`, so nothing else on
  the Team can clobber it mid-edit. (Single-member Team here, but I still name
  the boundary explicitly — that boundary becomes the Work's Boundary field.)
- I know the evidence I will require before I ever create the Work: a named
  test command with an exit code, and a PR URL. If I can't name that evidence
  yet, per the Host loop the Work isn't bounded — I make sure I can before
  step 1.

I do **not** message `kiwi` first ("hey, can you take this?") — assignment
never travels by message; it happens as a Work operation in step 1.

## 1. Decompose into one bounded Work

This task is small enough to stay a single Work rather than an epic split
into "implement" + "test" + "PR" sub-Works — the Host loop's guidance to
prefer several bounded Works over one epic is about *unrelated* bounded
responsibilities, not about splitting one atomic change with its own test
into artificial pieces that would just re-serialize through the same board.
I write the four required fields so `kiwi` never has to ask what "done"
means:

- **What**: Add a `--json` output flag to the `harness export` subcommand
  (`crates/firm-cli`), with an automated test proving it.
- **Mental model**: `export` currently writes a fixed human-readable/plain
  output; other subcommands in the same binary (e.g. `harness governance
  check --json`) already use a boolean `--json` flag that switches the
  printer while leaving default output untouched — mirror that existing
  pattern rather than inventing a new one. JSON output must be a strict
  superset of information already computed for the human path (no new data
  sources), emitted as one parseable document to stdout.
- **Boundary**: touch only `crates/firm-cli/src/main.rs` (export subcommand
  arg parsing + dispatch) and its export command module/tests; do not touch
  unrelated subcommands, do not change the default (no-flag) output bytes.
- **Gates/Evidence** (verbatim, becomes `--completion-criteria`): `harness
  export --json` prints a single well-formed JSON document to stdout with the
  same underlying fields as the current default output; `harness export`
  (no flag) output is unchanged; a new automated test exercises `--json` and
  asserts the output parses as JSON and contains the expected keys; the named
  test command exits 0; PR opened with a diff scoped to the boundary above;
  CI green.

## 2. Create the Work (assignment as a Work operation, not a message)

```bash
harness team-run work create \
  --team-run-id tr-123 \
  --title "Add --json output flag to harness export, with test" \
  --context "harness export (crates/firm-cli) currently has no machine-readable output mode. Mirror the existing boolean --json pattern used by harness governance check. Boundary: crates/firm-cli/src/main.rs export dispatch + its command module/tests only. Do not change default (no-flag) output bytes. Owned path: crates/firm-cli." \
  --completion-criteria "harness export --json emits one well-formed JSON document to stdout carrying the same fields as the current default output; harness export (no flag) output is byte-for-byte unchanged; a new automated test invokes --json and asserts valid, expected-shape JSON; the named test command exits 0; PR opened scoped to the boundary; CI green." \
  --claim-mode host_assign \
  --owner-member-run-id mr-k1 \
  --idempotency-key builders-export-json-w1
```

This is the only assignment action I take. `kiwi` is currently idle, so this
`WorkDelivery` reaches it and its NodeDaemon wakes it at its next safe
provider cycle — I don't send a "please pick this up" message on top of it.

## 3. Wait without polling

I hand the returned dashboard URL to the user, then watch via cursors/events
instead of a sleep loop:

```bash
harness team-run board-summary --id tr-123
harness team-run work list --team-run-id tr-123 --brief
```

I record the cursor from that call and, on the next check (triggered by an
event/notification, not a timer), advance from it:

```bash
harness team-run work list --team-run-id tr-123 --since <cursor>
```

I do not re-issue `board-summary` in a loop — it's a status snapshot to read
at my own safe boundaries (e.g. when a notification arrives), not something
to spin on.

## 4. Member-side (what I expect `kiwi` to do, for context — not my action)

`kiwi` wakes with `HARNESS_WORK_ID` set, runs `work show` to read
What/Mental-model/Boundary/Gates, then `work start --expected-version <v>` to
move the Work to `Active`. Per the plan-first habit in the skill, I expect one
correlated `response-required` message before real edits start, e.g. "plan:
add `--json` bool flag parsed alongside existing export flags, branch printer
after the existing summary is computed, new test asserts
`serde_json::from_str` succeeds and checks 2–3 key fields. OK?" — I do not
have to chase this; it arrives as a delivery to my Host inbox.

## 5. Answer the correlated question, on the same correlation

I read my inbox at a safe boundary (not mid-reasoning):

```bash
harness team-run host-inbox --team-run-id tr-123
```

If `kiwi`'s plan message is a provider-pausing interaction, I resolve it on
the **exact same correlation id** — never a fresh uncorrelated reply, which
would strand the pause:

```bash
harness team-run resolve-interaction \
  --team-run-id tr-123 \
  --member-run-id mr-k1 \
  --interaction-id <interaction-id-from-inbox> \
  --option approve \
  --note "proceed; keep default output byte-identical, JSON path additive only"
```

If instead it's an ordinary informational aside (e.g. "found the governance
--json precedent, reusing its printer helper"), I don't need to answer at
all — informational intent doesn't require a response-required round trip
back, so I just let it sit as read.

## 6. Wait again; handle side-discoveries and blocks honestly

Back to step 3's cadence (`board-summary` / `work list --since <cursor>`),
event-driven, no timer loop. Two things I watch for without acting
prematurely:

- **A new open Work appears** (e.g. `kiwi` notices the plain-text export path
  also needs a `--pretty`/format doc update and opens an unassigned
  `team_claim` follow-up instead of scope-creeping the current Work). I leave
  it open for now — I don't silently assign it to `kiwi` mid-flight; that's a
  separate decision after this Work closes.
- **`condition` flips to `Blocked`** with a reason (e.g. "existing export
  summary struct isn't `Serialize` yet, needs a field-mapping decision"). I
  do not let that sit — I read the reason via `work show`/inbox and send one
  decision-shaped, `response-required` message resolving it, then confirm the
  condition clears back to `Normal`.

## 7. Submission arrives

`kiwi` submits (member-side action, shown for completeness of the sequence I
am watching for):

```bash
harness team-run work submit \
  --team-run-id tr-123 \
  --work-id <work-id> \
  --expected-version <v> \
  --result-summary "## RESULT\nAdded --json bool flag to export dispatch; JSON printer reuses existing ExportSummary fields via serde. Default text output unchanged (diffed byte-for-byte against fixture).\n## TEST\ncrates/firm-cli/tests/export_json.rs — asserts output parses + has expected keys." \
  --artifact-ref <PR URL> \
  --check-ref "cargo test -p firm-cli export_json -- exit 0"
```

This moves the Work to `review` — per the shared invariants, that's a request
for my judgment, not a result I can wave through.

## 8. What I check before accepting

I do not accept on the submission's own "done" language, the delivery
receipt, or a green check-ref string alone. I open the evidence and walk the
completion criteria line by line:

1. **Open the PR diff** (`--artifact-ref`) and confirm it is scoped to the
   declared Boundary — only the export dispatch/command module and its test
   changed; nothing in unrelated subcommands moved.
2. **Rerun the named check myself**, not trust the reported exit code:
   `cargo test -p firm-cli export_json` — confirm it actually exits 0 in a
   clean checkout of the PR branch, not just read the pasted transcript.
3. **Run `harness export --json` by hand** against a sample project and pipe
   it through a JSON parser (e.g. `| jq .`) to confirm it's one well-formed
   document, and eyeball that it carries the same underlying fields as the
   plain path.
4. **Run `harness export` (no flag)** and diff its output against the
   pre-change baseline to confirm byte-for-byte no regression — this was an
   explicit criterion, not an assumption.
5. **Confirm the test is real**, not a tautology — it must fail if `--json`
   were removed or produced malformed output (read the assertion, don't just
   see a green count).
6. **Confirm CI is green** on the PR, not only my local rerun.
7. Re-read the four completion-criteria clauses from step 1 against 1–6,
   line by line, and only then decide.

## 9a. Accept

If all six checks pass:

```bash
harness team-run work accept \
  --team-run-id tr-123 \
  --work-id <work-id> \
  --expected-version <v+1> \
  --note "Verified: PR scoped to export boundary; cargo test -p firm-cli export_json passes locally + CI; --json output parses and matches default-path fields; default output byte-identical to baseline. Accepted."
```

Work closes, resolution `Accepted`.

## 9b. Request changes (if any check fails)

If, say, the default (no-flag) output changed by even one byte, or the test
is a tautology:

```bash
harness team-run work request-changes \
  --team-run-id tr-123 \
  --work-id <work-id> \
  --expected-version <v+1> \
  --reason "harness export (no --json) output differs from baseline by <diff>; must stay byte-identical. Please gate the new printer strictly behind the flag."
```

Work returns to `Active`. I do **not** spawn a new member or a fresh session
for the fix — `kiwi` continues in the same `mr-k1` MemberRun, same worktree,
same native session, so it keeps all the context from steps 4–7. I loop back
to step 6 (wait, event-driven) for the resubmission, then repeat step 8.

## 10. Recovery contingency (only if needed)

If `kiwi`'s Kimi ACP runtime dies mid-Work (not the normal path, but the
Host loop requires I know it before starting), I do not treat that as lost
work or spawn a replacement member:

```bash
harness team-run close-member --team-run-id tr-123 --member-run-id mr-k1
# ...later, once the runtime is available again...
harness team-run reopen-member --team-run-id tr-123 --member-run-id mr-k1
```

`reopen-member` resumes the exact native session under a new Supervisor
generation with the Work still owned by `mr-k1` — I only do this if the board
shows the Work stuck `Active`/`RecoveryRequired` with no forward progress; I
never report the task done on an unproven recovery.

## 11. Teardown / final state

After acceptance in step 9a, I confirm no other non-terminal Work is
outstanding on `tr-123` (e.g. any side-discovery Work from step 6 must be
closed, reassigned, or explicitly left for a later run — TeamRun completion
would otherwise reject it atomically):

```bash
harness team-run work list --team-run-id tr-123 --brief
```

If this run existed solely for this one task and the board is clean, I can
complete the run; the durable Team `builders` and `kiwi`'s membership/session
survive into the next TeamRun regardless. If more work is expected soon, I
simply leave the TeamRun open and `kiwi` idle — no action required to "end"
anything.

I then report back to the user: PR URL, the exact test command and its
result, confirmation the default output is unchanged, and the Work's
`Accepted` state — evidence, not a status adjective.

---

### Anti-patterns I deliberately avoided (per the skill)

- No `sleep`+status polling loop — cursor/event-driven checks only (steps 3, 6).
- No "please take this" message — assignment happened only via `work create
  --claim-mode host_assign` (step 2).
- No accept on `kiwi`'s "done" wording or the check-ref string alone — I
  reran the test and diffed the default output myself (step 8).
- No fresh member/session spawned on request-changes or on a dead runtime —
  same `mr-k1` MemberRun resumed in both cases (steps 9b, 10).
- No second private inbox/ledger — everything above reads the board/inbox
  projections (`board-summary`, `work list`, `host-inbox`), never a memory of
  "what I think kiwi is doing."
