# Live findings — 2026-07-27

Everything below was executed against the real provider (Claude Code 2.1.220,
bundled by `@anthropic-ai/claude-agent-sdk` 0.3.220) in this worktree. Facts
only; anything not verified is marked as such.

## Verified

| # | Claim | Evidence |
| --- | --- | --- |
| A1 | A persistent member accepts messages from the real provider | `ASSIGNMENT-ACK` returned on turn 1 |
| A2 | The member survives an empty mailbox | 3 s lull, `pending=0 closed=false`, no `member_closed`; turn 2 returned `SECOND-TURN-OK` |
| A3 | Both turns live in one native session | one `session_bound`; `851b37dd-…jsonl` holds user=2 / assistant=2 |
| A4 | The provider's own registry is a sufficient member roster | `tagSession(id,"trun-live-1:mrun-RuntimeBuilder")`, then `listSessions({dir})` filtered by tag returns exactly that member |
| A5 | An SDK-created session imports into Claude Desktop | `open "claude://resume?session=<id>"` → `Imported CLI session … as Desktop session local_<id>`; MCP `list_sessions` shows `local_851b37dd-…` |
| A6 | Resume after import appends coherently — **sequentially** | transcript 19 → 27 lines, user=3 / assistant=3, same session id, no fork, no conflict entry in the desktop log |
| A7 | Unit suite green | 9/9 |

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
- **Long-lived interrupt/steer against the real provider.** `q.interrupt()` and
  `q.setPermissionMode()` are covered by unit tests against the fake only.
- **Plan-approval gate against the real provider.** Unit-tested only. (The
  owned-paths gate has since been verified live — see §C.)

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


## C. `bypassPermissions` does not disable the gates (2026-07-27)

The runner now defaults to `permissionMode: "bypassPermissions"`, matching what
`claude_team_permission_mode()` already sent on the `claude_cli` path. An
interactive permission prompt has nobody to answer it inside an unattended
member; leaving that layer on only produces a deadlock.

The obvious worry is that it also turns off the owned-paths and plan gates.
It does not. Both controls were run live against Claude Haiku 4.5 with
`permissionMode: "bypassPermissions"`, `allowedTools: ["Write","Read"]` and
`ownedPaths: ["owned"]` (`scripts/gate-live.mjs`):

| Case | Result |
| --- | --- |
| Write to `nototmine/should-not-exist.txt` (outside the lane) | `PreToolUse` denied; **file was never created** |
| Write to `owned/allowed.txt` (inside the lane) | succeeded, file contains `INSIDE-LANE` |

So `bypassPermissions` skips the *prompt* layer; hooks still run and a hook
`deny` still wins. **Hooks — not permission mode — are the enforcement boundary
for a member.** The positive control matters as much as the negative one: it
rules out "everything was blocked" being mistaken for "the gate works".

One combination is genuinely unbounded: prompts off **and** `ownedPaths: []`
(the documented "no restriction" value). Each half is defensible alone; together
nothing constrains the member's writes. The runner now emits
`unbounded_write_scope` in that case, so it is announced rather than silent.
Covered by `an unbounded member is announced rather than silently allowed`.
