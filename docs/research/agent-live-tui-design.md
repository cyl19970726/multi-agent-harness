## Live Agent Activity View (Claude-Code-TUI style)

### Goal
Let the operator watch an agent turn unfold in real time — thinking → tool_use → tool_result → assistant text → result — rendered like the Claude Code TUI, reusing the existing `TurnDrillIn` / `summarizeRawEvent` / `GET /v1/provider-sessions/{id}/events` machinery. Ship live in two stages, lowest-risk first.

---

### Stage A — incremental write + poll-while-running (PRIMARY)

#### Backend (Rust, all in `crates/harness-cli/src/main.rs`, zero schema change)

**A1. Write NDJSON incrementally as parsed (replaces the end-of-turn batch).**
- `parse_claude_stream_json` (main.rs:7215-7224) today collects into a `Vec` and returns it. Add a sink-aware variant `parse_claude_stream_json_to(reader, sink: &mut impl Write) -> Vec<ClaudeStreamEvent>`: inside the existing `for line in reader.lines()` loop, after `ClaudeStreamEvent::parse_line` succeeds, do `writeln!(sink, "{}", serde_json::to_string(&event.payload)?)?; sink.flush()?;` then still `push` to the Vec and return it. Status inference (`infer_claude_session_status`), reply-text extraction (`extract_claude_reply_text`), evidence, and the terminal row are all unchanged because the returned Vec is unchanged.
- `run_claude_exec_delivery_real` (main.rs:8252): pass `session_dir` (currently `_session_dir`) through, compute `ndjson_ref = session_dir.join("claude.stream-json.ndjson")`, open it with `OpenOptions::new().create(true).append(true)` wrapped in a `BufWriter`, and replace `let events = parse_claude_stream_json(reader);` at line 8356 with the sink-aware call. The existing "drain stdout line-by-line, then read stderr" ordering already streams in real time, so each flush makes a line visible to a poller within tens of ms.
- `run_claude_delivery` (main.rs:8100-8134): the file is now already complete on disk, so **delete the end-of-turn batch** `fs::write(&ndjson_ref, &ndjson_content)` (lines 8129-8134). The terminal `ProviderSession` row keeps `jsonl_ref: Some(ndjson_ref...)` (line 8201) unchanged.

**A2. Give the RUNNING claim row a `jsonl_ref` + pre-create the empty file.**
- The path is deterministic: `<store.root()>/provider-sessions/<delivery_id>/claude.stream-json.ndjson` — the same path A1 writes. At claim time, `fs::create_dir_all(session_dir)` and create an empty `claude.stream-json.ndjson` so the first poll (before any line is written) returns `{events: [], truncated: false}` instead of the route's "session has no recorded event stream" error.
- `build_claimed_provider_session` (main.rs:4637, `jsonl_ref: None` at 4663): add a `jsonl_path: Option<String>` param and set `jsonl_ref`. Thread it from `claim_message_for_delivery` (main.rs:4429). Only set it for claude (codex claim row gets `None` until the codex parity WP). No store change needed — `claim_queued_message_delivery` (harness-store/src/lib.rs:146-189) appends whatever `ProviderSession` it is handed.
- Why the events route resolves correctly without change: `read_provider_session_events` → `latest_provider_session` (main.rs:6821-6830) is keyed by `id`, and BOTH the claim row and the terminal row share the same `delivery_id` as their `id`. During the turn the claim row (now with `jsonl_ref`) wins; after, the terminal row (same `jsonl_ref`) wins. Both point at the same growing/complete file. `read_provider_session_events` (main.rs:6838) needs **no change** — it re-reads each call and already skips a partial trailing line.

No new route, no SSE frame, no `SseEventFrame` variant, no watcher wiring, no migration.

#### Frontend (`apps/agent-dashboard/src/surfaces/Surfaces.tsx`, plus one type tweak)

**A3. Make `TurnDrillIn` live while running** (Surfaces.tsx:3503-3557). Replace the one-shot `toggle()` fetch with a `useEffect` keyed on `[open, session.status, session.id, apiUrl]`:
- When `open && session.status === "running" && apiUrl`: run `fetchEvents()` immediately and `setInterval(fetchEvents, 1000)`; clear on cleanup. Guard overlapping fetches with an `inFlight` ref. Backend file only grows, so naive `setEvents(data.events ?? [])` replace is correct and idempotent — no diffing.
- When status flips to terminal (the snapshot SSE already pushes the `provider_session` status change → `running`→`succeeded`/`failed`): do one final fetch, then stop polling. This terminal frame is the existing, already-wired "stop" signal — no new plumbing.
- Auto-open when live: in `CurrentWorkBanner`'s `running` branch (Surfaces.tsx:2923-2945), render `<TurnDrillIn session={running} apiUrl={apiUrl} defaultOpen autoLive />` so the operator SEES the turn without clicking. Add a `defaultOpen?: boolean` prop initializing `open`.
- Live affordance in the toggle button: when `session.status === "running"` show a pulsing red dot + "LIVE" + rolling `events.length`; else the existing `· N events · turn`.
- Note: `ProviderSessionStatus` serializes snake_case → `"running"` (confirmed in harness-core), so the existing lowercase `status === "running"` comparisons are correct.

**A4. Upgrade `summarizeRawEvent` into a real TUI renderer** (Surfaces.tsx:3576-3622). See the event-shape table below. Key changes: add the missing `thinking` case (it currently falls through to raw JSON); pretty-print tool_use args; match `tool_result.tool_use_id` back to `tool_use.id` and render indented under the call; render assistant text as Markdown (reuse the `Markdown` component already used by `ChatBubble` at Surfaces.tsx:3478); add a result footer chip (subtype + duration + `total_cost_usd` + tokens). Grouping consecutive `assistant` chunks by `message.id` (the captured sample shows ~10 chunks sharing one id) is optional polish — the 1:1 row stream reads fine in the scroll pane and is lowest-risk for v1.

**A5. Placement.** One component, two mount points, identical rendering:
- Live: auto-opened `TurnDrillIn` inline in `CurrentWorkBanner` (visible above the Composer on every tab).
- Recorded: the same `TurnDrillIn` as the per-reply drill-in on each agent bubble in `ConversationStream` (Surfaces.tsx:3484) — unchanged except it now also auto-renders the rich layout.

---

### Stage B — SSE per-event push (OPTIONAL latency upgrade, after A)

Reality: deliver and serve are separate processes, so "backend emits SSE" is implemented as "delivery tees each parsed event to ONE shared append-only file; the existing 150ms file-tailing watcher broadcasts it."

- **B1** `parse_claude_stream_json_to` already exists from A1. Add a *second* optional sink: also append `{"session_id": <delivery_id>, "event": <payload>}` + flush to `<store_root>/provider_turn_events.jsonl` (a transient tee, gitignored, truncate/rotate on serve start).
- **B2** `sse.rs`: add `SseEventFrame::ProviderTurnEvent(serde_json::Value)` (enum at sse.rs:17); add `"provider_turn_events.jsonl"` to the seed list (sse.rs:98-109) and the poll loop (sse.rs:114+) with `|line| serde_json::from_str::<Value>(line).ok().map(SseEventFrame::ProviderTurnEvent)`. The torn-line offset machinery (`check_and_broadcast_appends`) handles partial writes already. Seeding at EOF means a serve started mid-delivery won't replay; the catch-up fetch covers that.
- **B3** `handle_sse_stream` match (main.rs ~2307): add `SseEventFrame::ProviderTurnEvent(v) => write_sse_frame(&mut stream, "provider_turn_event", &v)`.
- **B4** Frontend `api.ts`: extend `SseFrame` (api.ts:53) with `| { kind: "provider_turn_event"; sessionId: string; event: RawTurnEvent }`; add the `addEventListener("provider_turn_event", …)` in `openEventStream` (after api.ts:106); add an `applyFrame` case (api.ts:120) appending into a new `liveTurnEvents: Record<string, RawTurnEvent[]>` field (capped ~2000). `TurnDrillIn` reads `liveTurnEvents[session.id]` when present instead of polling; on running→terminal it does ONE catch-up fetch of the events route (the durable per-session file is the source of truth) to reconcile any dropped/seeded-at-EOF frame.
- **Graceful degrade**: if SSE is down (`sseMode !== "sse"`), `TurnDrillIn` falls back to the Stage A poll loop — zero extra backend code.

---

### Event-shape rendering table (exact field paths, verified against captured `claude.stream-json.ndjson`)

| Event (`type`) | Detect | Read these exact paths | TUI render |
|---|---|---|---|
| `system` / `subtype:"init"` | `type==="system"` | `event.session_id`, `event.model`, `event.cwd` | dim header: `system/init  model {model} · cwd {cwd}` |
| `system` / `subtype:"thinking_tokens"` | `type==="system"` | `event.subtype` | dim meta line (or hide) |
| `assistant` → thinking | block `b.type==="thinking"` in `event.message.content[]` | `b.thinking` (EMPTY by API design), `b.signature` | `✻ thinking` muted/italic badge; show `(sig {n}b · encrypted)` from signature length; never print body |
| `assistant` → tool_use | block `b.type==="tool_use"` | `b.id`, `b.name`, `b.input` → `input.command` (Bash), `input.file_path` (Read/Edit/Write), else compactJson | `⏺ {name}({command\|file_path})` call card, keyed by `b.id` |
| `assistant` → text | block `b.type==="text"` | `b.text` | assistant prose rendered as Markdown |
| `assistant` grouping | all chunks share `event.message.id` | `event.message.id`, `event.uuid` (per-chunk), `event.parent_tool_use_id` | optional: collapse chunks of one `message.id` into one block |
| `user` → tool_result | block `b.type==="tool_result"` in `event.message.content[]` | `b.tool_use_id`, `b.content` (string or array), `b.is_error` | `⎿ {content}` indented under matching `tool_use.id`; red if `is_error`; truncate + expand long output |
| `result` / `subtype:"success"` | `type==="result"` | `event.subtype`, `event.result`, `event.num_turns`, `event.ttft_ms`, `event.duration_ms`, `event.total_cost_usd`, `event.usage`, `event.modelUsage[model]` (`cache_creation_input_tokens`, `cache_read_input_tokens`) | footer chip: `result/{subtype} · {duration} · ${cost} · {tokens}` |
| `rate_limit_event` | `type==="rate_limit_event"` | `event.rate_limit_info.status`, `.resetsAt` | small warn chip when throttled |
| (codex parity) `item` events | `event.item.type` present | `item.type`, `item.text`, `item.command` (existing branch at Surfaces.tsx:3580) | unchanged existing path |

---

### Live mechanism

- **Stage A**: claim row carries `jsonl_ref` + empty file at delivery start → `TurnDrillIn` polls `GET /v1/provider-sessions/{id}/events` every 1s while `status==="running"`; backend flushes each parsed NDJSON line to that file in real time. Stop signal = the already-wired `provider_session` SSE status frame flipping off `running`. Latency ≈ 1s (poll) + tens of ms (flush). MAX_EVENTS=1000 cap returns `truncated:true` → TUI shows "…+more".
- **Stage B**: same incremental write also tees to one shared `provider_turn_events.jsonl`; existing 150ms watcher broadcasts `provider_turn_event` frames; frontend appends push-style; one catch-up fetch on turn end reconciles. Latency ≈ 150ms.

---

### ASCII mockup (live, Stage A)

```
┌─ hermes · backend-agent ──────────────────────────────────────────┐
│ ● LIVE  Implement lifecycle-gated autonomy runner   claude · 0:14 ▾│
├────────────────────────────────────────────────────────────────────┤
│ system/init    model claude-opus-4-8 · cwd /multi-agent-harness     │
│                                                                      │
│ ✻ thinking     (sig 512b · encrypted)                               │
│                                                                      │
│ ⏺ assistant    I'll locate the gate check first.                    │
│                                                                      │
│ ⏺ Bash         grep -n "autonomy_gate" crates/harness-cli/src       │
│   ⎿ tool_result  main.rs:4422:  if autonomy_gate_open(member) {     │
│                  main.rs:8051:  gate = require_autonomous_gate()     │
│                                                                      │
│ ⏺ Read         crates/harness-cli/src/main.rs  (8040-8080)          │
│   ⎿ tool_result  fn run_generated_round(...) { let gate = ...       │
│                                                                      │
│ ⏺ Edit         main.rs  +  if !gate { return Ok(Skipped) }          │
│   ⎿ tool_result  Applied 1 edit to main.rs                          │
│                                                                      │
│ ⏺ Bash         cargo test gate                                  ⠋   │
│   ⎿ (running…)                                                       │
│                                                                      │
│ ░ polling every 1s · 11 events · jsonl_ref live                     │
└────────────────────────────────────────────────────────────────────┘
   on turn end → ● LIVE goes solid grey, header: "claude · 0:31 · 14 events"
   result footer appends: result/success · 0:31 · $0.04 · 26.6k cw / 17.4k cr
```

---

### Sequenced Work Packages

1. **WP-A1 (backend, claude incremental write)** — `parse_claude_stream_json_to` + sink threading in `run_claude_exec_delivery_real`; delete end-of-turn batch in `run_claude_delivery`. Tests: assert file grows mid-turn (write events, read between writes). Files: `crates/harness-cli/src/main.rs`.
2. **WP-A2 (backend, claim-row jsonl_ref + pre-create file)** — add `jsonl_path` to `build_claimed_provider_session`, thread from `claim_message_for_delivery`, `create_dir_all` + empty file at claim. Test: claim row carries `jsonl_ref`; events route returns `{events:[]}` before first line. Files: `crates/harness-cli/src/main.rs`.
3. **WP-A3 (frontend, poll-while-running)** — `TurnDrillIn` useEffect poll loop + `defaultOpen`/`autoLive` props + LIVE chip; auto-open in `CurrentWorkBanner`. Files: `apps/agent-dashboard/src/surfaces/Surfaces.tsx`.
4. **WP-A4 (frontend, TUI renderer)** — extend `summarizeRawEvent`/`RawEventRow`: thinking case, tool_use pretty-print, tool_result match-by-id + indent, markdown assistant text, result footer. Files: `apps/agent-dashboard/src/surfaces/Surfaces.tsx`.
5. **WP-A5 (backend, codex parity — optional)** — apply WP-A1/A2 shape to `run_codex_exec_process`/`run_codex_exec_delivery`/codex claim row. Files: `crates/harness-cli/src/main.rs`.
6. **WP-B1 (backend, SSE tee + frame)** — second sink to shared `provider_turn_events.jsonl`; `SseEventFrame::ProviderTurnEvent`; watcher entry; match arm; gitignore + truncate-on-serve. Files: `crates/harness-cli/src/main.rs`, `crates/harness-cli/src/sse.rs`, `.gitignore`.
7. **WP-B2 (frontend, SSE consume + reconcile)** — `SseFrame` variant, listener, `applyFrame` `liveTurnEvents` map; `TurnDrillIn` consumes live map with poll fallback + catch-up-on-stale. Files: `apps/agent-dashboard/src/api.ts`, `apps/agent-dashboard/src/surfaces/Surfaces.tsx`.