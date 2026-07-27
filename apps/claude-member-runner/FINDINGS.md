# Live findings — 2026-07-27

Everything below was executed against the real provider (Claude Code 2.1.220,
bundled by `@anthropic-ai/claude-agent-sdk` 0.3.220) in this worktree. Facts
only; anything not verified is marked as such.

> Historical evidence note: the plan-gate experiments below describe an
> implementation that ADR 0039 subsequently retired. Harness now represents
> planning as ordinary correlated Markdown conversation; there is no Plan
> Mode, Plan Gate, or plan-approval message lifecycle. The old measurements are
> retained only to explain why the simpler contract was chosen.

## Verified

| # | Claim | Evidence |
| --- | --- | --- |
| A1 | A persistent member accepts messages from the real provider | `ASSIGNMENT-ACK` returned on turn 1 |
| A2 | The member survives an empty mailbox | 3 s lull, `pending=0 closed=false`, no `member_closed`; turn 2 returned `SECOND-TURN-OK` |
| A3 | Both turns live in one native session | one `session_bound`; `851b37dd-…jsonl` holds user=2 / assistant=2 |
| A4 | The provider's own registry is a sufficient member roster | `tagSession(id,"trun-live-1:mrun-RuntimeBuilder")`, then `listSessions({dir})` filtered by tag returns exactly that member |
| A5 | An SDK-created session imports into Claude Desktop | `open "claude://resume?session=<id>"` → `Imported CLI session … as Desktop session local_<id>`; MCP `list_sessions` shows `local_851b37dd-…` |
| A6 | Resume after import appends coherently — **sequentially** | transcript 19 → 27 lines, user=3 / assistant=3, same session id, no fork, no conflict entry in the desktop log |
| A7 | Unit suite green | 12/12 |

Two things worth keeping:

- **The desktop `local_` id is `local_` + the native session id, for imported
  sessions.** Desktop-*created* sessions instead get an unrelated uuid plus a
  `Mapping internal session X to CLI session Y` log line. Harness uses the
  import path, so its mapping is deterministic and needs no lookup table.
- **Import strips thinking blocks** (`Stripped thinking blocks from …jsonl
  (12 lines…)`), which is the AGENTS.md thinking policy enforced on the
  provider side for free.

## Not verified — do not claim these

- **Simultaneous writes.** A6 tested *sequential* access: the desktop had
  warmed the session but was not generating when the SDK resumed. Two writers
  producing at the same time is untested and may behave differently. Until
  someone tests it, the operating rule is: **desktop is read-only observation
  while Harness drives.**
- ~~Long-lived interrupt/steer against the real provider.~~ Run on 2026-07-27;
  it found a defect. See §E.
- **OS-level containment.** Owned-path hooks are observations, not a sandbox;
  a worktree or container is required for a real isolation boundary.

## Corrections to earlier conclusions in this repo's discussion

1. **"Desktop visibility is impossible" was wrong.** It is reachable through the
   `claude://resume?session=<uuid>` deep link, which calls the desktop's own
   `importCliSession`. Two earlier statements to the contrary in the design
   discussion should not be inherited.
2. **The published TypeScript docs' streaming-input example is wrong.** It shows
   `yield { type: "user", content: [...] }`. The runtime rejects that with
   `Expected message role 'user', got 'undefined'` and the SDK process exits 1.
   The correct shape is in the SDK's own `sdk.d.ts`:

   ```ts
   type SDKUserMessage = {
     type: 'user';
     message: MessageParam;          // { role: 'user', content: [...] }
     parent_tool_use_id: string | null;
   }
   ```

   The first version of `test/fake-sdk.mjs` was written from the same wrong
   assumption as `renderTeamMessage`, so it passed while the real call failed.
   The fake now asserts `message.role === 'user'` — a fake that shares the code's
   assumption verifies nothing.
3. **`claude auth status` reporting `loggedIn: true` does not mean the token
   works.** It checks presence, not validity; a request still returned
   `401 OAuth access token has expired`. Use a real call as the check.

## Provider versions on this machine

| Source | Version |
| --- | --- |
| standalone CLI `~/.local/bin/claude` | 2.1.181 |
| Claude Desktop bundled | 2.1.219 |
| Agent SDK bundled (this worktree) | **2.1.220** (commit 4073f595, built 2026-07-24) |

Credentials are shared through the Keychain item `Claude Code-credentials`, so
one `claude auth login` covers all three.

Per AGENTS.md, 2.1.220 arrived with the dependency install and has **not** been
named and approved by a Human. `claude_agent_sdk` stays `review_required`.


## 2026-07-27 — Stage 3 (Rust wiring)

| # | Claim | Evidence |
| --- | --- | --- |
| B1 | `claude/agent-sdk` is accepted and recorded | profile shows `execution_mode=claude_agent_sdk`, `adapter_contract_version=claude-agent-sdk-v1`, `reviewed_provider_versions=[]` |
| B2 | The harness can start a member through it | `team-run start` completed against Claude Haiku 4.5 |
| B3 | **A message arriving after the queue emptied reaches the same member** | 2 handoffs; round 2 summary `SECOND-ROUND`; one native session `52735a0a…` with `user=2 assistant=2` |
| B4 | Same, deterministically | `claude_agent_sdk_member.rs` 3/3 — the fake runner sends only *after* `turn_complete`, so the ordering is constructed, not raced |
| B5 | An unregistered mode fails explicitly | `claude/not-a-mode` is rejected rather than falling back to `claude_cli` |

### Corrections to earlier entries above

- **"`HARNESS_HOME` does not override store resolution" was too strong.** It
  does work — the integration tests isolate cleanly with `HOME` + `HARNESS_HOME`
  via `TempHome::envs()`. What actually happened is narrower: when **no central
  project is selected**, the CLI walks up from cwd and a legacy repo-local
  `.harness` wins. Running from a directory under one, as here, silently used
  the developer's real store. Isolate with both env vars *and* an inited
  project, and check the `using repo-local store …` warning on stderr.
- **`harness init` switches the global active project** as a side effect. It
  changed `ACTIVE_PROJECT` on this machine; restored with
  `harness project switch`, but the previous value was ambiguous (two projects
  tied on `last_opened_at`).
- **A pipeline's exit code is the last command's.** `cargo test … | tail -30`
  reports 0 even when tests fail, and truncates the log that would have shown
  it. Capture to a file and read `$?` from the unpiped command.

### Default mode switched (2026-07-27)

`claude_agent_sdk` is now what a member declared as plain `claude` gets;
`claude_cli` requires naming it. The argument is not that the new path is
better — it is that `claude_cli` provably cannot satisfy ADR 0037 acceptance
item 6, and a mode that ends a member on a momentarily empty queue is not a
safe default.

Two consequences to keep visible:

- The default is `review_required`. That is deliberate and honest, not an
  oversight: the profile still claims nothing beyond `claude_cli` until a live
  canary exercises interrupt, steer and a real `PreToolUse` denial.
- The default now needs `node` plus the runner's dependency, where `claude_cli`
  needed only the `claude` binary. Missing runner or missing `node` fails
  explicitly with the three ways out; it never silently falls back, because a
  silent fallback to the one-shot path is exactly the bug being removed.

Covered by `a_bare_claude_member_defaults_to_the_agent_sdk_mode`.


## C. `bypassPermissions` does not disable hooks (2026-07-27)

The runner now defaults to `permissionMode: "bypassPermissions"`, matching what
`claude_team_permission_mode()` already sent on the `claude_cli` path. An
interactive permission prompt has nobody to answer it inside an unattended
member; leaving that layer on only produces a deadlock.

The original experiment asked whether it also turns off hook execution. It
does not. The owned-path hook was run live against Claude Haiku 4.5 with
`permissionMode: "bypassPermissions"`, `allowedTools: ["Write","Read"]` and
`ownedPaths: ["owned"]` (`scripts/gate-live.mjs`):

| Case | Result |
| --- | --- |
| Write to `nototmine/should-not-exist.txt` (outside the lane) | `PreToolUse` denied; **file was never created** |
| Write to `owned/allowed.txt` (inside the lane) | succeeded, file contains `INSIDE-LANE` |

So `bypassPermissions` skips the *prompt* layer; hooks still run and a hook
`deny` still wins. The positive control matters as much as the negative one: it
rules out "everything was blocked" being mistaken for "the gate works".

### Scope of that claim, and what changed after it

The first write-up said "**hooks are the enforcement boundary for a member**".
Too strong; do not inherit it. The measurement covers **Write**, and the matcher
never saw Bash — which has no `file_path` for the check to read.

That prompted the actual decision: **there is no containment boundary, on
purpose.** Members are maximum-permission across all providers (Claude
`bypassPermissions`, Codex `danger-full-access`; Kimi's headless `-p` rejects
permission flags outright) with the full tool set, because they have to build,
test and use git.

So the owned-paths hook was changed from **deny to observe**. A cross-lane write
emits `cross_lane_write` and proceeds.

The reasoning is worth keeping, because "half a gate" is the tempting middle
option and it is the worst one: a hook that stops `Write` but not `echo >` is
not a boundary, it is a boundary-shaped thing that earns trust it cannot repay.
Blocking would also push the same edit into Bash and out of the Host's view,
whereas observing surfaces it at review time — "this member wrote outside its
lane, was that intended?" is the question that actually matters.

`owned_paths` is therefore what ADR 0033 always described: a declared lane for
coordination and acceptance. Real containment, if ever needed, has to come from
the OS — a worktree the member cannot escape, or a container.

Covered by `a cross-lane write is reported and still allowed to proceed`.


## D. Why the experimental plan gate was retired (2026-07-27)

The experiment revealed that a provider-specific plan gate required a second
state machine alongside ordinary Host/Member conversation.

`planRequired` was read as `Boolean(config.planRequired)`, and the Rust caller
never sent that field: `grep -c planRequired` over `main.rs` returned 0. So it
was permanently `false` and the gate could only ever fire in unit tests that set
it directly. A gate that cannot fire is worse than no gate: it reads as a
control in review and enforces nothing.

An intermediate implementation wired this chain:

```text
plan_request  -> gate armed   (plan_gate_armed)
plan_approval -> gate released (plan_approved)
neither       -> never gated
```

ADR 0039 later removed this chain. The Host now sends “return a plan; do not
execute”, the Member replies with Markdown, and the Host responds with an
ordinary revise-or-execute message in the same correlation. That model works
across providers and has no hidden gate state.


## E. The canary failed, and that was the point (2026-07-27)

A live probe originally covered interrupt, steer, and the experimental plan
gate against Claude Haiku 4.5.

| | Result |
| --- | --- |
| steer — `setPermissionMode("acceptEdits")` | pass, acknowledged `{"mode":"acceptEdits"}` |
| historical plan-gate experiment | inconclusive; subsequently retired by ADR 0039 |
| interrupt | **failed**, then fixed and re-verified |

### The interrupt defect

`interrupt()` returned a clean-looking receipt, `{still_queued: []}`, and then
the member was dead:

```text
PRE-INTERRUPT  turns=0
INTERRUPT      receipt {"still_queued":[]}
POST-INTERRUPT closed=false  turns=0        <- looks alive
               delivery TIMEOUT after 60s   <- is not
               member_closed never emitted
               start() neither resolved nor rejected
```

The SDK's own wording is the clue: **"Interrupts the query."** It ends the
query, not the turn. The first implementation bound one member to one query, so
interrupting ended the member — except nothing detected it. The stream stopped
yielding without ending, so `for await` blocked forever and the runner believed
the member was still alive.

This is the same mistake as the original P0, one layer down:

| | Conflated | Symptom |
| --- | --- | --- |
| `claude -p` | a **turn** with a **member** | member dies on an empty queue |
| this runner | a **query/process** with a **member** | member dies on an interrupt |

The rule that falls out: **the member's lifetime is defined by Harness — the
mailbox — and everything under it (turn, query, OS process) is disposable and
rebuildable from the native session.** The native session is the fixed point;
that is what ADR 0032's "provider-native session is execution truth" means
operationally, not just where logs live.

Fix: a member spans query *generations*. `interrupt()` takes the receipt,
`Mailbox.supersede()` retires the current consumer without closing the mailbox
or dropping queued messages, `query.return()` ends the iterator, and `start()`
opens a fresh query with `resume: <native_session_id>`. The `ede_diagnostic`
error an interrupted query throws is swallowed only when we caused it; anything
else still propagates.

Re-verified with the same probe that exposed it:

```text
POST-INTERRUPT delivery landed: true   turns=1
last reply: "ALIVE-AFTER-INTERRUPT"
member_closed seen: 1
```

Covered by `the member survives an interrupt and consumes the next message` and
`the resumed query continues the same native session`.

### Why a receipt is not an acknowledgement

`{still_queued: []}` proved only that the call returned. AGENTS.md asks for a
**terminal acknowledgement**, and this is why: the difference between the two is
exactly the bug above. `supports_cancel` should not be claimed from a return
value; it needs the member to still answer afterwards.

### The plan-gate block was not exercised

`plan_gate_blocked=0` with no file created. The innocent explanation is that the
model answered the `plan_request` with a plan and never attempted the Write, so
the gate never fired. "The file is absent" has the same shape as "the gate
worked" — the trap §C avoided with a positive control, and this probe walked
into. Not scored as a pass.
