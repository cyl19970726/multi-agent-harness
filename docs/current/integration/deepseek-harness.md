# DeepSeek Harness Provider

DeepSeek Harness is a current managed Agent Team provider. The canonical
provider id is `deepseek_harness`; its only Team execution mode is
`deepseek_sdk`. It is host-driven and does not create another orchestration,
Work, Message, or identity authority.

## Reviewed binding

| Fact | Accepted value |
| --- | --- |
| Upstream | `deepseek-ai/deepseek-harness` |
| Package version | `0.1.1-rc.2` |
| Reviewed source revision | `b150a551b8d465e31e418e1b2eaf5e79bbb7d28e` |
| Native protocol | `deepseek-harness-native/v1` |
| Protocol fingerprint | `deepseek-harness-native/v1@dsh-0.1.1-rc.2+b150a551+session-events-v1` |
| Cordis composition fingerprint | `sha256:333c529f67aa2237096dd5191cfd4c46842d14eed786669b9be18b9cc4e2401f` |
| Native session locator | `deepseek_harness_session` |
| Execution driver | `host_driven` |

The integration follows DSH's “Everything is Plugin” model. Star Harness boots
a reviewed Cordis composition and uses the native `ctx.agents` service. It does
not fork DSH core, wrap the interactive CLI, or reconstruct a provider session
from Harness events.

The composition includes the official DeepSeek LLM, Agent spine and loop,
JSONL Session persistence and checkpoints, sandbox policy, local sandbox,
filesystem and bash sandbox tools, subprocess ownership, token metering and
compaction plugins. DSH Goal plugins are deliberately not loaded. A change to
the plugin tree, exact versions, or configuration changes the reviewed
composition and requires the provider upgrade gate.

Before spawning Node or loading Cordis, the Rust adapter verifies the exact
runner dependency manifest, every installed reviewed `@deepseek-ai/dsh-*`
package version, and the SHA-256 of `cordis.yml`. The native `session_bound`
handshake then independently revalidates provider version, reviewed upstream
source revision, and composition fingerprint before an AgentSession can bind.
Missing or conflicting evidence fails closed.

## Authority and lifecycle

`ctx.agents.create` creates the durable provider-native Session;
`ctx.agents.resume` must return the exact requested Session id. Work and
ordinary Messages cross the provider boundary through `Agent.followup`. The
matching `agent/inbox/spliced` message id is the durable input-acceptance
receipt. `Agent.whenIdle` plus the native `turn/end` reason defines the cycle
boundary; provider completion is never Host acceptance or semantic success.

Interrupt uses `Agent.cancel`, waits for idle, flushes the Session store, and
retains the exact native Session id. Close disposes the owned AgentHandle,
reaps the runner process group and retains the Session id for an explicit
higher-generation Reopen. NodeDaemon, AgentSession, Supervisor generation and
RuntimeCommand fences remain the only lifecycle authority.

A managed Host issues Interrupt through its Supervisor-bound CLI capability:
`firm member runtime interrupt --member-run-id <dsh-member-run> --expected-version <n> --reason <text>`.
The operator-equivalent `firm team-run interrupt-member` reaches the same live
Supervisor and provider-control path. Neither command writes the Store or the
DSH Session directly.

Provider transcripts, tool output and reasoning stay in the DSH Session store
under `DSH_SESSION_ROOT`. Harness receives only coordination receipts and
transient activity projections; it does not mirror the native transcript.
Historical Agent Workspace reads invoke the reviewed official JSONL persistence
package, which owns zstd decoding and exact SessionId lookup. Harness consumes
only a bounded logical JSONL response in memory and never stores it. Live
Cordis events expose generic thinking/response/tool phases to the exact owner;
raw reasoning, arguments, output, and filesystem paths remain absent.

## Permissions and honest gaps

The frozen AgentSession ceiling compiles into the shared DSH sandbox policy:

| Harness ceiling | DSH policy mode |
| --- | --- |
| `read_only` | `read-only` |
| `workspace_write` | `workspace-write` |
| `full_access` | `danger-full-access` |

The policy is consumed by both filesystem and bash sandbox plugins with the
bound Project workspace as root. Missing or mismatched exact packages fail
before a provider Session is admitted.

Current limitations are explicit: no current-turn injection, native queue
claim, Goal continuation control, effect inspection/reconciliation, or
standalone Node AgentSession. Strong `quiesce` and `release` remain degraded
because the composition cannot yet prove process-independent durable flush and
writable-child drain. Narrow Team Close remains supported.

## Acceptance evidence

DEV-63 live acceptance created and resumed the same native Session
`star-3b69a281-44a0-4068-87a6-02d355f434d9`, received matching
`agent/inbox/spliced` receipts for `dev63-proxy-input-1` and
`dev63-proxy-input-2`, and completed real DeepSeek turns. A separate busy bash
cycle `dev63-final-interrupt-input` on Session
`star-dc77b84b-fa18-48e6-9d00-6f45e175137c` was cancelled and returned idle on
the same Session without a fabricated `turn_complete`.

Repository acceptance includes the runner protocol tests, exact-version
profile admission, closed five-provider registry and dispatch tests, runtime
control replay safety, Dashboard/MCP exposure, formatting, clippy, full
workspace tests, documentation governance and the clean-archive gate. A
release claim additionally requires a post-merge real Agent Team canary driven
by the NodeDaemon, with exact MemberRun, AgentSession, RuntimeCommand, delivery,
receipt, submit and Host acceptance evidence.
