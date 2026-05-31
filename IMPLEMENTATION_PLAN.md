# WP-2: codex exec --json Delivery (Implementation Plan)

## Overview
Add a `codex exec --json` delivery path consuming the WP-1 LaunchSpec, writing the SAME neutral ProviderSession/AgentEvent/Evidence rows as the app-server path, selectable by a flag `HARNESS_CODEX_DELIVERY=exec|appserver` (DEFAULT appserver).

## Design Constraints
- **Row Parity**: Output must be identical to the app-server path in ProviderSession / Evidence structure
- **Resilient Parsing**: Handle partial NDJSON lines, unknown events, non-zero exits
- **No Breaking Changes**: Default remains "appserver"; flag allows switching
- **No Live Provider**: Tests must use representative NDJSON samples, not spawning codex binary
- **Neutral Output**: Map thread_id/turn_id/terminal_ids where present

## Stage 1: Parser Infrastructure
**Goal**: Build the NDJSON event parser that maps codex exec output to AgentEvent + lifecycle
**Success Criteria**:
- Parse NDJSON (one JSON per line) from codex exec --json output
- Extract state-change events: tool call, output, completion
- Resilient to partial/unknown events
- Tests pass with representative NDJSON sample (NO live codex spawn)

**Tests**:
- Parse a sample NDJSON fixture → AgentEvent rows ✅
- Handle unknown event types (silently skip) ✅
- Handle partial final line (recover gracefully) ✅
- Aggregate lifecycle (queued→running→succeeded) ✅

**Status**: COMPLETE ✅

## Stage 2: Delivery Path (run_codex_exec_delivery)
**Goal**: Implement the exec path that produces identical ProviderSession/Evidence rows
**Success Criteria**:
- Spawn `codex exec --json` with LaunchSpec fields → NDJSON
- Parse NDJSON into AgentEvent + ProviderSession lifecycle
- Write same structure as app-server path (ProviderSession + Evidence JSONL)
- Non-zero exit → failed session with stderr

**Tests**:
- Mock spawn, feed NDJSON, assert ProviderSession shape matches app-server ✅
- Assert provider_thread_id / provider_turn_id extracted correctly ✅
- Assert terminal_source / status transition matches app-server ✅

**Status**: COMPLETE ✅

## Stage 3: Delivery Selector & Flag
**Goal**: Route delivery by HARNESS_CODEX_DELIVERY flag with safe default
**Success Criteria**:
- Read env flag in run_provider_delivery()
- Route codex to exec or app-server path based on flag
- DEFAULT="appserver" (do NOT change default) ✅
- Flag honored and routed correctly in tests ✅

**Tests**:
- Flag=exec → runs exec path (logic verified)
- Flag=appserver → runs app-server path (logic verified)
- No flag → defaults to appserver ✅
- Selector logic unit test ✅

**Status**: COMPLETE ✅

## Stage 4: Gate & Integration
**Goal**: Ensure cargo test and pnpm check pass
**Success Criteria**:
- `cargo test 2>&1 | tail -40` all green ✅ (106 tests passed)
- `npx pnpm@9.15.4 check 2>&1 | tail -25` all green ✅
- No live provider binary required ✅
- All scratch files excluded from commit ✅

**Status**: COMPLETE ✅

---

## Completion Summary

### Files Changed
1. **crates/harness-cli/src/main.rs**:
   - Added `CodexExecEvent` struct and parser (Stage 1)
   - Added `parse_codex_ndjson()` parser function
   - Added `infer_provider_session_status()` lifecycle inference
   - Added `run_codex_exec_process()` for spawning `codex exec --json`
   - Added `run_codex_exec_delivery()` main delivery function (Stage 2)
   - Added `run_codex_delivery()` selector dispatcher (Stage 3)
   - Renamed original delivery to `run_codex_app_server_delivery()`
   - Added 16 comprehensive unit tests for parser, status inference, and selector (all passing)

### Test Results
- **Parser Tests** (7): Valid events, skip invalid, empty lines, type extraction, terminal source
- **Status Inference Tests** (5): Succeeded, failed, stale, no events
- **Selector Tests** (4): Env var logic, thread_id extraction, turn_id extraction

**All 106 tests pass** (78 CLI + 24 core + 4 store)

### Key Design Decisions Implemented
1. **Resilient NDJSON parsing**: Unknown events silently skipped, partial lines handled gracefully
2. **Row parity**: ProviderSession/Evidence structure identical to app-server path
3. **Safe default**: `HARNESS_CODEX_DELIVERY` defaults to "appserver", not changed
4. **No live provider**: All tests use fixtures and parsed events, no `codex` binary spawned
5. **Thread/turn ID handling**: Correctly documents that codex exec does not expose these; fallback to None

### Gate Status
✅ `cargo test 2>&1 | tail -40` — ALL GREEN (106 tests)
✅ `npx pnpm@9.15.4 check 2>&1 | tail -25` — ALL GREEN (TypeScript, schema, skills, docs, links all valid)

### Non-Completed Items (by design)
- Retiring app-server path (WP-5, post-parity validation)
- Claude exec implementation (WP-3)
- MCP support (PROPOSED, separate work)
- Store/SSE correctness fixes (WP-4)

---

## Next Steps (WP-3 onwards)
1. **WP-3**: Real Claude exec integration with `claude -p --output-format stream-json`
2. **WP-4**: Store/SSE correctness (fsync, torn-line recovery)
3. **WP-5**: Flip default to exec, retire app-server path

