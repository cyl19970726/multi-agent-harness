# Kimi (Moonshot) integration

本文档定义 Star Harness 如何集成 Kimi Code（Moonshot）。重点是把
Kimi 变成 harness 里的第三个 registry-routed provider：可以创建、投递消息、
观察状态、回收运行时，并以 Kimi 原生 session 作为执行记录与 resume 真相；
Harness 只保存 session binding、跨系统协调、显式 outcome 与 artifact/check refs。

Provider-neutral runtime contracts live in ../agent-runtime.md.
This file should explain only how Kimi implements those contracts. Shared object
semantics such as `Task`, `Message`, `Evidence`, `Proposal`, and `Decision` must
not be redefined here.

## Persistent Agent Team mode

Kimi Agent Team members use only `kimi_acp`; bounded `kimi_exec` remains a
Dynamic Workflow and historical-read substrate. Planning, continuation,
requested/effective controls, busy-turn mailbox behavior, Interrupt, restart,
and native-session resume are defined in the focused
[Kimi ACP Agent Team runtime](kimi-agent-team.md) contract.

The installed Kimi Code probe is 0.31.1. Following the Human-approved upgrade,
`kimi-acp-v1` is reviewed for prompt delivery, model/`thinking` control,
same-session resume across a Supervisor generation change, next-round batched
mail, bounded full-access permission receipts, and cooperative Interrupt.
`session/cancel` is an ACP notification without a JSON-RPC request id; sending
it as a request produces method-not-found and is not a valid capability probe.

## Install and login

Operator prerequisite:

```bash
# Install Kimi Code using Moonshot's current installer/package instructions.
# The harness expects a `kimi` executable.
kimi login
kimi -p "ping" --output-format stream-json
```

Binary resolution order is implemented by `resolve_kimi_bin()`:

1. `KIMI_CODE_BIN` env override, if non-empty;
2. bare `kimi` on `PATH`;
3. default install path `~/.kimi-code/bin/kimi`;
4. bare `kimi` as the final fallback, so spawn failure is explicit.

Source: `crates/firm-cli/src/main.rs:14317-14345`.

## Bounded Workflow Delivery (Not Agent Team)

This compatibility section describes the one-shot `kimi_exec` path used by
bounded workflows and the older standalone provider-process API. It is not the
Agent Team delivery algorithm. New Team Members always use the persistent ACP
contract below; do not copy `kimi -p` behavior into Team lifecycle code.

每次投递消息时，harness 构造一个包含：

- 当前任务上下文（goal/task/evidence/decision）；
- 消息队列（inbox 消息）；
- harness developer instructions（角色、权限、安全）。

然后调用 Kimi CLI：

```bash
kimi -p "{structured_prompt}" --output-format stream-json
```

可选参数：

```bash
kimi -p "{structured_prompt}" --output-format stream-json --model <model>
kimi -p "{structured_prompt}" --output-format stream-json --session <session_id>
```

`run_kimi_exec_delivery_real` 会把 developer instructions 折叠进 prompt，因为 Kimi 没有
Claude 的 `--append-system-prompt`；resume 使用 `--session <id>`；model 使用 `--model <model>`
(`crates/firm-cli/src/main.rs:14587-14606`)。

Kimi delivery 明确不传这些 Claude-only 或非真实 headless flags：

```text
--verbose
--permission-mode
--allowedTools
--json-schema
--mcp-config
--add-dir
--effort
```

Source: `crates/firm-cli/src/main.rs:14562-14567`,
`crates/firm-cli/src/main.rs:14607-14612`.

Kimi 执行时产生 flat NDJSON transport frames，Harness 在内存归约并返回：

- `NativeSessionRef`，provider 为 `kimi`；
- 可选 resumable session id（来自 `session.resume_hint`）；
- 当前调用可消费的内存态 assistant response；
- 仅描述 delivery 成功/失败的 `DeliveryOutcome.summary`，不含 assistant content；
- no native usage/model/cost/structured frame in `-p` mode，走 degraded fallback
  (`crates/firm-cli/src/main.rs:14658-14763`)。

## Event Sources

Kimi 产生的事件通过以下源进来：

1. **Kimi stdout flat NDJSON** — 直接解析 `kimi -p --output-format stream-json` 输出：
   - assistant reply frame: `{"role":"assistant","content":"..."}`
   - resume hint frame:
     `{"role":"meta","type":"session.resume_hint","session_id":"...","command":"kimi -r ..."}`
   - no Claude `system.init`
   - no Claude terminal `result`
   - no `usage` / `model` frame in `-p` mode

2. **Native session binding** — Harness 只记录 provider/mode/session id、
   adapter/provider version、availability 与 resume capability。

3. **No automatic Evidence ingest** — native Session id is provider provenance on
   the Delivery/AgentSession binding, not Harness Evidence. Provider output cannot
   fabricate Evidence or an authored report Message; those require explicit
   canonical collaboration writes.

Source: `crates/firm-cli/src/main.rs:14687-14733`.

## Reducer Mapping

Kimi 事件 -> harness objects：

```text
(provider = "kimi")
  role=="assistant"              -> transient native activity + outcome candidate
  type=="session.resume_hint"    -> NativeSessionRef.native_session_id
  other/unknown frame            -> transient native activity only
```

Kimi uses kimi-native parsing:

- `parse_kimi_frames` parses one JSON frame per non-empty NDJSON line;
- `extract_kimi_reply_text` concatenates every assistant frame's content;
- `extract_kimi_session_id` reads `session_id` from `type=="session.resume_hint"`;
- `infer_kimi_status` treats clean exit with frames as success, clean empty output as stale,
  and non-zero exit as failed.

Source: `crates/firm-cli/src/main.rs:14360-14430`.

Kimi frames are reduced in memory to a delivery result and a mode-aware native
session binding. There is no durable Kimi stream-ingest ledger; chat, tool,
command, file, and turn detail remains in Kimi's native session store.

Queue discipline（来自 harness，不由 provider 定义）：

- 投递前：消息锁定在 `delivery_status = queued`
- 投递中：更新为 `delivery_status = in_progress`
- 投递后：若成功则 `delivery_status = delivered`；若失败重试或 `failed`
- claim/lease 原子性由 firm-store 负责，provider adapter 只执行 delivery

## Permission Model

Kimi interactive CLI exposes standalone mode flags:

```text
--plan
--auto
-y / --yolo
```

These are execution modes, not a native sandbox contract. The real `kimi -p`
headless delivery path does **not** use them. Kimi v0.18 rejects permission
flags combined with `--prompt` / `-p`, so `spawn_kimi_ephemeral` and `run_kimi_exec_delivery_real`
pass no permission flag. This means kimi has **no read-only mode at all**: a leaf the workflow
declares read-only can still edit the live tree (observed in dogfooding — a read-only kimi leaf
edited two checked-in docs).

Writable vs read-only boundaries are therefore enforced **structurally by the harness**, not by a
Kimi CLI flag. Kimi declares `enforces_read_only = false` in `ProviderCapabilities::kimi_exec()`
(unlike codex `--sandbox read-only` and claude's `Read,Grep,Glob` tool allowlist), and the workflow
leaf runner reads that capability: a read-only leaf whose provider can't enforce read-only is run in
a throwaway git worktree anyway, so any writes land in a discardable checkout instead of the live
repo (`provider_enforces_read_only` / `step_needs_isolation`,
`crates/firm-cli/src/main.rs`). On a non-git project there is no worktree to isolate into, so the
leaf degrades to the shared cwd with a printed warning that its writes are not contained.

The persistent Agent Team `kimi_acp` path is stricter: because ACP likewise
exposes no provable read-only or workspace-only sandbox, `map_permission`
rejects `ReadOnly` and `WorkspaceWrite` at Session admission. Only a frozen
`FullAccess` AgentSession may start. Permission callbacks can then select only
an exact provider intent of `allow_once` or `allow_always`; option ids and
labels never grant permission. This is the current trusted-development policy,
not a claim that Kimi can enforce a narrower ceiling.

Source: `crates/firm-cli/src/main.rs:14471-14478`,
`crates/firm-cli/src/main.rs:14607-14612`.

Provider config remains provider-neutral:

```json
{
  "provider": "kimi",
  "provider_config": {
    "approval_policy": "none" | "prompt_required",
    "workspace_policy": "workspaceWrite" | "readOnly",
    "service_tier": "free" | "pro" | "team"
  }
}
```

## ACP Session Driver (Agent Team v0)

For Agent Team (ADR
[0025](../../decisions/0025-agent-team-run-control-plane.md)) the kimi member drive surface is the
ACP (Agent Client Protocol) JSON-RPC session over stdio, not one-shot print mode:

`initialize -> session/new|session/resume -> session/prompt`; `session/load` is
the compatibility fallback when an older server rejects `session/resume`.
`session/cancel` requires a separately reviewed capability for the detected
version.

- The ACP `sessionId` is stored through the mode-aware
  `MemberRun.native_session` and reused for follow-up rounds. The locator,
  detected Kimi version, adapter contract version, availability, and resume
  support are explicit; Harness does not persist a second transcript or tool
  stream.
- `session/update` message, thought, and tool frames stream during the turn.
  Thought is sanitized into transient live display only. Tool calls remain in
  Kimi's native session and feed only an ephemeral activity projection; they
  are not converted into provider-derived MemberAction rows.
- `session/request_permission` is implemented as a reverse-RPC bridge. For a
  trusted full-access Member, an ordinary tool request with an exact provider
  intent of `allow_always` or `allow_once` receives an immediate provider-control receipt without
  creating a question Message. Harness writes one bounded
  `provider_control` acknowledgement without command or prompt content; it
  does not mark the Member waiting or manufacture a permission workflow.
- `AskUserQuestion` and Plan Review route to Lead as correlated
  `provider_interaction_request` Messages and resume only after the exact
  causation-linked `provider_interaction_response` with selected ACP `optionId`.
  Tool requests outside the
  frozen AgentSession ceiling fail closed. Company-level legal, financial, permission,
  and organization effects remain subject to their native Human Approval
  contract and are never converted into ordinary full-access tool receipts.
- Cancellation is execution-mode **and reviewed-version specific**. The
  reviewed Kimi ACP 0.31.0 path sends `session/cancel` as a JSON-RPC
  notification, waits for terminal `stopReason=cancelled`, and only then
  returns the MemberRun to `idle`. An earlier canary incorrectly sent the
  notification as a request and received `-32601 Method not found`; that was a
  Harness framing defect, not evidence that 0.31.0 lacked cancellation.
  Explicit Host Close instead latches runtime-shutdown intent and terminates
  the Harness-owned ACP process; it does not claim a provider-native close or
  cancellation receipt. Reopen starts a higher adapter generation and resumes
  the exact recorded ACP session.
  Kimi ACP still does not support same-turn steer, so
  ordinary Message is queued for the next provider round. An attempted Steer
  fails rather than being silently converted. Close records `stopped`; Reopen
  returns the same MemberRun to a new active runtime generation.
- The target NodeDaemon under the current durable Team Supervisor generation
  atomically claims one queued CanonicalMessageDelivery before
  `session/prompt`, resolves and freezes its exact recipient AgentSession, and
  first proves the ACP transport is live. Failed preflight leaves mail queued
  and reconnects the recorded session. `provider_received` is recorded only
  after ACP returns its native request/session receipt. An uncertain post-crash claim requires explicit
  reconciliation and is never blindly replayed. A delivered trigger without a
  Work submission resumes the same native session with a recovery prompt that
  asks the Member to inspect its native state, workspace, and latest Work
  version; Work is not replayed as a new attempt. Explicit Close is durably latched before process
  teardown.
- Client FS and terminal reverse-RPC are not advertised. Unknown client methods
  fail closed with `methodNotFound`.
- Kimi-native Agent/AgentSwarm/background-task and hook events are not yet
  reduced into DelegationRun. The provider may use them internally, but Harness
  does not claim child lifecycle control or complete observation.

The authoritative mode snapshot is `MemberRun.provider_profile` with
`execution_mode=kimi_acp`; it must not be inferred from the older
`ProviderCapabilities::kimi_exec()` headless-delivery preset.

## Native session storage and workspace

Kimi owns its native session history and resume data. Harness stores only the
session binding and coordination above it. Process transport is short-lived:

```text
{harness_root}/runtimes/{member_id}/
{harness_root}/runtimes/deliveries/{delivery_id}/  # removed after reduction
```

`run_kimi_delivery` does not retain Kimi stdout/stderr/NDJSON as Harness history.

## Native Goal and multi-Agent features

Kimi Code 0.31 exposes persistent Goals, built-in `coder`, `explore`, and
`plan` agents, background/nested subagents, Markdown custom agents at user,
project, and Plugin scopes, hooks, session recovery, context compaction, MCP,
modes, and model/thinking configuration. That provider-native inventory is not
the same as current Adapter coverage.

Doctrine:

> Child threads stay under the parent member, not promoted to members.

For the current `kimi_acp` Team Member mode:

- native subagents remain implementation details of the invoking MemberRun;
- a native Goal may help the Member continue internally across turns, but ACP
  does not yet give Harness a reviewed inspect/replace/cancel/terminal contract
  for that Goal, so Kimi remains `host_driven`;
- no native child is promoted into a MemberRun;
- no lifecycle control is claimed without a provider child identifier and
  tested interrupt/resume/close path;
- hook/background/session files may contain prompts, command output, paths, and
  credentials and must not be copied into public evidence without redaction;
- Kimi plan updates are provider-native execution aids. A Host-requested plan
  is still an ordinary correlated Markdown conversation, not a Plan Gate;
  provider thinking remains transient-only.

## Evidence and Report Extraction

Kimi output contains flat assistant text plus optional meta frames. The adapter
reads these from Kimi native storage and projects them without copying:

```text
Kimi native session
  ├─ role=="assistant" content    -> ephemeral activity / explicit outcome on promotion
  ├─ session.resume_hint          -> NativeSessionRef
  ├─ tool and status frames       -> ephemeral activity
  └─ provider errors              -> native detail + Harness lifecycle summary when needed
```

Harness may explicitly promote an outcome, handoff, artifact reference, check,
or governed decision. It does not capture raw Kimi NDJSON/stderr as a parallel
evidence store. Native-session export, if later offered, is an explicit
redacted user operation under ADR 0032.

`spawn_kimi_ephemeral` sets `tokens`, `model`, `structured`, and `cost_usd` to `None`
because Kimi `-p` stream-json carries no usage/model/cost frame
(`crates/firm-cli/src/main.rs:14497-14516`).

## Dashboard Health Signals

Dashboard reads `runtime_health` / session records computed by firm-cli:

```json
{
  "endpoint": {
    "status": "pass" | "fail" | "unknown",
    "message": "kimi binary resolved" | "kimi binary unavailable"
  },
  "session": {
    "status": "pass" | "warn" | "fail",
    "message": "last Kimi session succeeded" | "no sessions yet" | "last Kimi session failed"
  },
  "delivery": {
    "status": "pass" | "warn" | "fail",
    "message": "last message delivered" | "delivery pending"
  },
  "checked_at": "2026-06-20T00:00:00Z"
}
```

Codex has process/socket/protocol/delivery. Kimi has no persistent process and no socket protocol,
so the meaningful layers are binary endpoint, session, and delivery.

### Account capacity is honestly unknown

The reviewed ACP surface is `initialize` and
`session/{new,resume,load,set_config_option,prompt,cancel,update,request_permission}`.
None of them reports account quota, usage, or rate limits, so
`firm member preflight --provider kimi` returns
`state: unknown, evidence_source: not_exposed` with an empty `windows` list.
No percentage may be approximated from local logs.

Kimi capacity has **no** observable source today, including after a failure.
ACP has no HTTP-status error channel: a quota `403` arrives as free-form
JSON-RPC error text, indistinguishable at the protocol level from any other
error or a plain hang, and a real terminal failure is journalled as
`action_type=error` by `journal_member_failure`, not as a structured
`provider_error`. Harness therefore never infers Kimi capacity from that text —
doing so would let arbitrary substrings, including a member's own words, gate a
start. Kimi stays `unknown` until Moonshot exposes a reviewed quota or
structured-error API. See [provider-capacity.md](provider-capacity.md).

## Capabilities and Cost

Kimi capability preset:

```text
streaming         true
resume            false (degraded/unknown)
mid_turn_approval false
subagents         false
mcp               false
hooks             false
schema            false (text-extract fallback)
cost              false (token-estimate fallback)
```

Source: `ProviderCapabilities::kimi_exec` in
`crates/firm-core/src/lib.rs`.

The registry tests assert that Kimi is registered, reports `ProviderCapabilities::kimi_exec()`,
uses `kimi.stream-json.ndjson`, and keeps schema/cost/resume false until proven
(`crates/firm-cli/src/main.rs:17029-17049`).

`provider_price_per_mtok("kimi")` currently returns placeholder estimate `(0.60, 2.50)`.
The source warns this is only a workflow spend bound, not billing truth, and must be confirmed
against Moonshot pricing or a future live usage frame before spend decisions are trusted
(`provider_price_per_mtok` in `crates/firm-core/src/lib.rs`).

## Fallback Modes

若 Kimi CLI 不可用或失败：

1. **No fallback to Moonshot HTTP API** — V1 keeps the CLI provider shape. HTTP API fallback would
   need a separate adapter/work package.

2. **Message queueing on delivery failure** — 消息留在 `delivery_status = failed` 或
   `delivery_status = queued`，下次 `agent deliver` 重试。

3. **Health downgrade** — 若 `resolve_kimi_bin()` 找不到 runnable binary，endpoint health
   降级，Dashboard 显示 Kimi unavailable。

4. **Schema fallback** — schema-mode nodes consume the assistant reply through harness text extraction,
   because Kimi `-p` has no `--json-schema` support in the implemented surface.

5. **Cost fallback** — cost uses harness token-estimate and placeholder price bounds because Kimi
   `-p` stream-json has no usage/cost frame.

6. **Resume fallback** — only a parsed `session.resume_hint.session_id` is exposed as resumable.
   Synthetic fallback session ids are not surfaced as resume tokens
   (`crates/firm-cli/src/main.rs:14675-14685`).

7. **Reconciliation hook** — 可通过 `agent reconcile` 手工修复状态（与 Codex / Claude 同）。

## Unsupported or Risky Surfaces

相比 Codex 和 Claude：

1. **No permission flags with `-p`** — Kimi v0.18 rejects permission flags with prompt mode.
   Harness must enforce boundaries through worktree/task ownership.

2. **No native schema frame** — no Kimi `--json-schema` equivalent is passed in V1.

3. **No native cost/usage frame** — `tokens`, `model`, and `cost_usd` are `None` in Kimi delivery.

4. **Resume is degraded** — `session.resume_hint` exists in the flat stream, but core capability still
   marks resume false until the end-to-end resume contract is proven.

5. **No MCP/hooks/subagents claim** — capability preset marks them false.

6. **Stale comments must not override parser truth** — any old comment claiming Claude-shaped Kimi
   output is superseded by the live v0.18 parser tests and kimi-native reducer. The regression test
   proves Claude reply extraction fails on real Kimi frames
   (`crates/firm-cli/src/main.rs:16945-16980`).

## Validation Gates

实现 Kimi 集成的 validation 清单：

- [ ] `kimi` installed and `kimi login` completed for the operator account.
- [ ] `kimi -p "ping" --output-format stream-json` emits flat NDJSON with assistant content.
- [ ] `agent create --provider kimi --start` creates a runtime with provider `kimi`.
- [ ] `agent deliver` spawns `kimi -p --output-format stream-json`.
- [ ] No permission flags are passed on the `-p` path.
- [ ] Optional `--model` is passed when launch spec has a model.
- [ ] Optional `--session` is passed only when a real resume id is available.
- [ ] Kimi transport frames are reduced in memory and no Harness transcript or
      NDJSON mirror is retained.
- [ ] Assistant reply extraction works for string content and array block content.
- [ ] ACP `sessionId` is bound to a mode-aware `NativeSessionRef` and native
      activity is read from Kimi's own session store.
- [ ] ACP reattachment prefers `session/resume`; method-not-found falls back to
      `session/load`, drains replayed history, and keeps schema/cost explicitly
      degraded where the selected Kimi mode does not expose them.
- [ ] `supported_provider_names()` includes `kimi`.
- [ ] Codex and Claude paths remain regression-clean.

## Sequencing with Other Work Packages

Kimi is the third registry-routed provider after Codex and Claude. The relevant sequencing is:

1. **Provider registry** — Kimi is registered through `provider_registry()` and resolved through
   `provider_adapter(name)`, not hard-coded dispatch (`crates/firm-cli/src/main.rs:14905-14915`).

2. **Kimi-native parser** — flat NDJSON parser and reducer are required because Claude parser cannot
   extract Kimi replies (`crates/firm-cli/src/main.rs:14349-14417`,
   `crates/firm-cli/src/main.rs:16968-16980`).

3. **Delivery implementation** — `run_kimi_delivery` binds NativeSessionRef and records only
   delivery status, explicit outcome, and promoted evidence under provider `kimi`.

4. **Capability honesty** — core keeps Kimi degraded except streaming until live behavior proves
   resume/schema/cost/MCP/hooks/subagents
   (`ProviderCapabilities::kimi_exec` in `crates/firm-core/src/lib.rs`).

5. **Future hardening** — once Kimi live usage, schema, resume, or tool control become stable,
   update `ProviderCapabilities::kimi_exec()`, parser tests, integration docs, and dashboard health
   expectations together.
