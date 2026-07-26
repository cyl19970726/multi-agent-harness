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
- **Gates against the real provider.** The owned-paths and plan-approval denials
  are unit-tested; no live run has exercised a real `PreToolUse` denial, because
  the live runs used `allowedTools: []`.

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
