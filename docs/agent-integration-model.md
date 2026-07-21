# Agent Integration Model

This is the canonical answer to one question: **to integrate a new agent,
provider, or platform into Star Harness, what do you have to define?**

It sits above the provider runtime implementation reference in
[agent-runtime.md](agent-runtime.md) and the execution boundary in
[execution-foundation.md](company-os/execution-foundation.md), and above the concrete
provider implementations in [integration/codex.md](integration/codex.md),
[integration/claude.md](integration/claude.md), and
[integration/kimi.md](integration/kimi.md). It does not redefine Mission/Wave,
executor-native records, WorkItems, Approvals, or organization authority.

Integration is organized around **three pillars** plus a single
**provider-neutral launch spec** that maps uniformly onto every platform:

```text
Pillar 1  Base configuration   prompt, skills, capabilities, model/profile
Pillar 2  Environment          workspace (worktree, owned paths), MCP, resources
Pillar 3  Platform adaptation  AgentProvider + EventReducer + capability decl.
---------------------------------------------------------------------------
Launch spec                     one normalized per-turn request, mapped onto
                                codex exec / claude -p / future platforms
```

The execution transport that binds the three pillars is **headless exec-stream**
or an explicitly selected interactive mode:
`codex exec --json`, `claude -p --output-format stream-json`, and
`kimi -p --output-format stream-json`, normalized into
an in-memory neutral projection. The provider-native session store remains the
sole durable transcript/tool/turn history and resume source; Harness retains a
mode-aware native session binding, not a second event store. The transport
decision is recorded
in [decisions/0018-exec-stream-primary-substrate.md](decisions/0018-exec-stream-primary-substrate.md).

## Why Three Pillars

An `AgentMember` (see [agent-control-plane.md](company-os/execution-foundation.md)) is a
durable identity. To make that identity *executable on a given platform* you
must answer three independent questions, and the launch spec is how the harness
hands a single turn to whatever platform sits behind the member:

| Pillar | Question | Where it lives today |
| --- | --- | --- |
| 1 Base configuration | What does this agent *know and is allowed to be*? | `prompt_ref`, `skill_refs`, `capabilities`, `model`, `profile` on `AgentMember` |
| 2 Environment | What can it *touch*? | `worktree_ref`, `runtime_workspace_roots`, `workspace_policy`; MCP via `AgentProviderConfig.mcp` |
| 3 Platform adaptation | How does the harness *drive* the platform, resolve its native session, read it, and resume it? | `AgentProvider` / provider adapter, native-session resolver, ephemeral reducer, `ProviderCapabilities` |

The pillars are deliberately separable: changing the platform (Pillar 3) must
not require rewriting the prompt stack (Pillar 1) or the workspace contract
(Pillar 2). That separability is the whole point of ADR 0011.

---

## Pillar 1 — Base Configuration (prompt, skills, capabilities)

Base configuration is the durable, platform-independent description of *what
the agent is*. It is written at member-create time and inspectable in the
Dashboard; it is not rewritten per task.

### Prompt: the `prompt_ref` resolution contract

`AgentMember.prompt_ref` is a nullable string. It is **a reference to a durable
prompt artifact, not inline chat text**. The contract:

- A `prompt_ref` points to a file or addressable prompt fragment that supplies
  the member's **role-specific** layer (what it owns, refuses, reviews,
  escalates, reports). It must be durable when it affects acceptance — role
  prompts that change permissions or evidence policy must be files/refs, not
  hidden chat context.
- The role prompt is **one layer** in the documented prompt stack from
  [agent-control-plane.md](company-os/execution-foundation.md). The harness composes the
  full system prompt per delivery from this stack:

```text
harness base system prompt          (Mission/Wave, honest execution records, gates)
  -> repository / adapter rules      (project constraints, commands, safety)
  -> role-specific prompt            (prompt_ref → this member's responsibility)
  -> execution context               (Mission, Wave, executor, run and assignments)
  -> optional company context        (WorkItem, source Document, Actors, approval policy)
  -> delivery envelope               (current executor-owned assignment or host request)
  -> permission and evidence policy   (allowed tools, approval, report format)
```

The launch spec's `prompt_ref` field carries the *composed* system/developer
instructions to the platform. Task-specific content does **not** go into
`prompt_ref`; it arrives as `message_content` (the turn input), so the member
identity is never rewritten per turn.

### Skills: the skill contract (WP-6 — implemented)

`AgentMember.skill_refs` is a `string[]`. The harness implements the following
contract for resolving and injecting skills:

- **Location.** A skill lives at `.agents/skills/<id>/SKILL.md` with YAML
  frontmatter carrying `name` (matching the folder) and a complete `description`
  (enforced by the `skills` gate in `harness governance check`).
- **Resolution.** A `skill_ref` is the skill `<id>`. The harness resolves it via
  the [`skill_resolver` module](../crates/harness-core/src/lib.rs): read
  `.agents/skills/<id>/SKILL.md` (and any files it links). The ref is durable
  and inspectable; it is not a copy.
- **Discovery.** The harness can enumerate `.agents/skills/*/SKILL.md`. A member
  declares which skills apply via `skill_refs`; the harness does not force a
  model to self-search for skills.
- **Validation.** The `skills` gate (`harness governance check`) validates that
  any `skill_ref` in a member JSON resolves to an existing skill directory.
  Dangling refs fail fast with a clear error message.
- **Injection.** A provider injects resolved skills as **explicit turn input**,
  not as ambient context. On exec-stream this is part of the composed prompt /
  developer instructions (Pillar 1 prompt stack), referenced in the launch spec
  via `skill_refs`. Codex passes a skill input item on `turn/start`; Claude
  injects via the system prompt. Either way the rule is the same: the harness
  chooses skills, the platform consumes them.
- **Kinds.** Two skill kinds are recognized: a **generic harness capability**
  (how to use Mission/Wave and the selected executor honestly) and a
  **project/adapter skill** (how to use a project's CLI, Dashboard, acceptance
  evidence, and safety boundaries). Skills are optional tools, never product
  authority.

### Capabilities: the vocabulary

`AgentMember.capabilities` is a free `string[]`. It declares what the *member*
is meant to do (e.g. `code`, `review`, `research`, `live_ops`). It is
member-level intent, distinct from the **provider capability declaration**
(Pillar 3), which states what the *platform* can technically support
(streaming, resume, mid-turn approval, subagents, MCP, hooks). The harness/UI
should reconcile the two: a member may *want* a capability the platform cannot
provide, and that gap must be shown honestly (see Pillar 3 and invariant 4 in
[integration/README.md](integration/README.md)).

### Model / profile selection

`AgentMember.model` and `AgentMember.profile` are nullable strings. `model`
selects the platform model (mapped to `--config model=` for Codex, `--model`
for Claude). `profile` selects a named provider configuration profile where the
platform supports one. Both are part of base configuration because they affect
behavior and cost but not *what the agent is allowed to touch*.

---

## Pillar 2 — Environment & Resources (workspace, MCP)

Pillar 2 describes what the agent can touch. It is enforced by the harness, not
by prompt text alone (invariant: `PermissionProfile` "refuses prompt-only
safety", per [agent-runtime.md](agent-runtime.md)).

### Workspace contract

The `WorkspaceProvider` interface from [agent-runtime.md](agent-runtime.md)
owns workspace lifecycle:

```text
WorkspaceProvider
  prepare_workspace(task)        # create/attach cwd, worktree, branch
  attach_branch_or_pr(task)
  inspect_changed_paths(task)    # what did the turn actually write?
  cleanup_or_archive(task)
```

The fields that bind a member to a workspace:

| Field | Meaning |
| --- | --- |
| `worktree_ref` | The git worktree / branch this member operates in. |
| `runtime_workspace_roots` | Roots the runtime is allowed to read/write. |
| `owned_paths` (per task) | Paths a specific task may modify; basis for diff gating. |
| `workspace_policy` | `read-only` vs writable; the abstract permission posture. |

The launch spec's `workspace` field carries the resolved cwd / worktree root to
the platform (`cwd` for Codex exec, `--add-dir` / process cwd for Claude).

### MCP integration (WP-6 — implemented)

The harness now implements MCP server attachment via a neutral contract. Both
target platforms consume MCP servers uniformly.

A neutral `mcp` block on the launch spec, sourced from `AgentProviderConfig.mcp`:

```text
mcp:
  servers:
    - id: <stable id>
      transport: stdio | http | sse
      command: [<argv for stdio>]     # argv array for a local server
      url: <for http/sse>             # endpoint for a remote server
      allowed_tools: [<tool id>, ...] # allowlist; omit = all tools on server
```

How each platform consumes the same neutral block:

| Neutral `mcp` element | Codex exec mapping | Claude -p / SDK mapping |
| --- | --- | --- |
| `servers[]` | `--config mcp_servers.<id>...` (Codex MCP config) | `--mcp-config <file>` / SDK `mcp_servers` |
| `transport` | `stdio` / streamable-http per Codex MCP config | `stdio` / `http`/`sse` per Claude MCP config |
| `command` / `url` | server launch entry in Codex config | server entry in the `--mcp-config` JSON |
| `allowed_tools` | tool allowlist on the member permission profile | `--allowedTools mcp__<server>__<tool>` |

**Implementation:** A member declares MCP servers via `AgentProviderConfig.mcp`
(additive field, defaults to None). The `build_launch_spec` function carries it
to the neutral launch spec. Providers map the spec onto their own MCP config
format (Codex `--config`, Claude `--mcp-config`). See
[`skill_resolver` module](../crates/harness-core/src/lib.rs) for the
`LaunchMcp` / `LaunchMcpServer` types.

### Declaring resource requirements

A provider declares resource needs through `provider_config` and the launch
spec, not through prompt text. Today `provider_config.runtime_workspace_roots`
and `environment_id` cover the workspace/environment surface. Any future
resource a platform needs (network egress posture, secret refs, compute tier)
should be added additively and surfaced through the provider capability
declaration (Pillar 3) so the Dashboard can show whether the environment can
satisfy the member.

---

## Pillar 3 — Platform Adaptation (Codex, Claude, low-code, OpenCloud/Hermit)

Pillar 3 is how the harness *drives* a platform and reads it back into neutral
state. A new platform is integrated by implementing four things.

### 1. The `AgentProvider` interface

From [agent-runtime.md](agent-runtime.md), reduced to the four verbs an
exec-stream integration must implement (the model uses **start / deliver /
probe / ingest** as the canonical names; they map onto the runtime interface):

| Verb | Runtime interface | Responsibility |
| --- | --- | --- |
| **start** | `create_runtime(member, workspace, permissions)` | Launch the platform for this member (process or session handle). |
| **deliver** | `deliver(message, context)` + `MessageDelivery` | Build the launch spec from the claimed `Message` and run one turn. |
| **probe** | `health(runtime)` | Report runtime health signals (below). |
| **read/resume** | native-session adapter | Resolve, project, and resume provider-owned session state without copying it. |

Delivery must respect the harness claim/lease: no platform side effect before
the latest queued `Message` is atomically claimed (see
[agent-runtime.md](agent-runtime.md) "Delivery claims happen before provider
side effects").

### 2. The native session projector

The adapter maps platform-native records to an ephemeral neutral projection
and promotes only explicit coordination boundaries:

```text
provider event
  -> NativeActivityProjection   (not persisted)
  -> PendingInteraction         (only when authority/routing crosses systems)
  -> explicit TeamMessage / outcome / artifact ref (only on promotion)
```

Rule: browser code never reads private native files directly. The provider
adapter owns format/version differences and returns a sanitized projection;
Harness does not turn it into a second ledger.

### 3. Health-signal contract

Health has layers, and **the layer set can differ per platform** — that is
allowed, as long as protocol/delivery health is real:

| Platform | Layers |
| --- | --- |
| Codex (app-server fallback) | process / socket / protocol / delivery |
| Codex (exec) / Claude (exec) | process(per-turn) / session / delivery |

A platform that has no persistent process (exec-stream) reports `not
applicable` for the process layer rather than faking it. The Dashboard must not
present process health as execution readiness when delivery health is unknown.

### 4. Permission mapping + fallback/unsupported declaration

A platform must map the **neutral permission** (Pillar 2 / launch spec) onto its
own controls and **declare what it cannot do**. Unsupported surfaces must be
explicit so the Dashboard shows honest capability state (invariant 4,
[integration/README.md](integration/README.md)). Example: Claude exec has no
mid-turn `turn/interrupt`; that is a declared unsupported surface, not a silent
gap.

### Provider capability declaration (WP-6 — implemented)

The harness now provides a neutral way for platforms to declare what they
technically support. The `ProviderCapabilities` struct in
[harness-core](../crates/harness-core/src/lib.rs) carries this:

```rust
pub struct ProviderCapabilities {
    pub streaming: bool,          // incremental event stream during a turn
    pub resume: bool,             // session resume (--session / --resume)
    pub mid_turn_approval: bool,  // approve/deny a tool call mid-turn
    pub subagents: bool,          // native child threads
    pub mcp: bool,                // MCP server attachment
    pub hooks: bool,              // lifecycle hook surface
    pub schema: bool,             // native structured-output / JSON-schema flag
    pub cost: bool,               // provider reports billed USD in terminal frame
    pub enforces_read_only: bool, // can physically run a read-only (non-mutating) leaf
}
```

Each provider implements a static method to declare its capabilities. Current
values per implementation:

| Capability | Codex exec | Claude -p / SDK | Kimi exec |
| --- | --- | --- | --- |
| streaming | yes (`--json` NDJSON) | yes (`stream-json`) | yes (`--output-format stream-json`) |
| resume | yes (`--session`) | yes (`--resume`) | no (unverified — degraded) |
| mid_turn_approval | **no** (policy pre-approve) | no (Tier-3 only) | no (unverified) |
| subagents | yes (observed) | yes (observed) | no (unverified) |
| mcp | yes (config) | yes (`--mcp-config`) | no (unverified) |
| hooks | no (limited) | no | no (unverified) |
| schema | yes (`--output-schema`) | yes (`--json-schema`) | no (text-extract fallback) |
| cost | no (token usage only) | yes (`result.total_cost_usd`) | no (token-estimate fallback) |
| enforces_read_only | yes (`--sandbox read-only`) | yes (read-only tool allowlist `Read,Grep,Glob`) | **no** (`kimi -p` rejects every permission flag) |

**Implementation:** `ProviderCapabilities::codex_exec()`,
`ProviderCapabilities::claude_exec()`, and `ProviderCapabilities::kimi_exec()`
return the columns above (see
[crates/harness-core/src/lib.rs](../crates/harness-core/src/lib.rs)). Most of the
`kimi_exec()` row is intentionally conservative — every axis except `streaming`
is `false` = degraded-until-verified against the live binary, not a positive
claim of absence. The snapshot can include these capabilities so the Dashboard
shows honest per-provider support.

`enforces_read_only` is different: it is a **VERIFIED** `false` for kimi, not a
TBD. The live `kimi -p` rejects every permission flag (`-y`/`--auto`/`--plan`)
and has no tool allowlist, so it has NO read-only mode — a leaf the workflow
declares read-only can still edit the live tree (observed in dogfooding: a
read-only kimi leaf edited two checked-in docs). Since #190 this is
**capability metadata**, not isolation routing: read-only workflow leaves run
in the selected project root regardless of provider — only `writable` leaves
and explicit `isolation="worktree"` opt-ins get a throwaway git worktree
(`step_needs_isolation` in
[crates/harness-cli/src/main.rs](../crates/harness-cli/src/main.rs)); a provider
capability gap must not silently turn a read-only scan/review into a
git-worktree requirement. The default-trait and unknown-provider values are
`false` (assume-unenforceable), surfaced as honest Dashboard capability state.

### Execution-mode profile and interaction truth

The legacy `ProviderCapabilities` booleans describe a broad technical preset;
they are not sufficient to claim an Agent Team integration. Every MemberRun now
snapshots `ProviderIntegrationProfile`, which names the concrete mode
(`codex_exec`, `kimi_acp`), interaction contract, tool/artifact event fidelity,
cancel/resume support, native-child observation, and transient-thinking policy.

Provider requests that require an answer are `PendingInteraction` rows rather
than hidden adapter callbacks. Questions, tool approvals, and plan reviews keep
the exact provider option ids and route to Lead, Policy, or Human. The
PendingInteraction/control acknowledgement records provider and semantic
resolution; ordinary provider tool lifecycle remains in the native session.

See [ADR 0030](decisions/0030-provider-interaction-contract.md) and
[ADR 0032](decisions/0032-provider-native-session-is-execution-truth.md). New provider
integrations must audit execution modes, reverse RPC, lifecycle, errors,
permissions, subagents, background work, context/compaction, native-store
discovery/read/resume, artifacts, auth/quota, and privacy in addition to
enumerating tools.

### The adapter boundary (generalized from earning-engine)

The earning-engine example
([adapter.json](../examples/adapters/earning-engine/adapter.json))
shows the generic split: an adapter supplies project tools, evidence policy,
dashboard links, permission policy, and skills, while the generic harness owns
coordination. Generalized:

| Generic harness owns | Adapter / platform owns |
| --- | --- |
| Mission/Wave joins, agent messages, role assignment | domain tool descriptors |
| evidence references, review gates, decisions | project dashboard, artifacts |
| member identity, prompt/skill refs, permissions | domain logic, live execution, secrets |
| the neutral launch spec and event reduction | platform-native CLI/SDK call shape |

A platform integration (Pillar 3) and a project adapter are orthogonal: a
provider teaches the harness *how to drive a runtime*; an adapter teaches the
harness *what tools and evidence a project exposes*. Both plug into the same
`AgentMember` without changing core object semantics.

### Substrate decision

Per [decisions/0018-exec-stream-primary-substrate.md](decisions/0018-exec-stream-primary-substrate.md):
**headless exec-stream is the primary integration substrate.** A new platform
should be integrated via its documented exec/stream-json mode first. The
persistent app-server (WebSocket-over-UDS) path is retained only as an
**optional fallback** for members that genuinely require live mid-turn approval.

---

## The Provider-Neutral Launch Spec

The launch spec is one normalized per-turn request. The harness builds it from
the member (Pillars 1–2) and the claimed `Message`, and each platform adapter
(Pillar 3) maps it onto its own call. This is the seam that keeps the operator
composer and Dashboard uniform across Codex, Claude, and future platforms.

| Neutral field | Meaning | Codex `exec` mapping | Claude `-p` / SDK mapping | Low-code / OpenCloud / Hermit |
| --- | --- | --- | --- | --- |
| `prompt_ref` | composed system/developer instructions (Pillar 1 stack), read as artifact | developer instructions / `--config` input | `--append-system-prompt` / SDK system prompt | adapter-provided |
| `message_content` | the turn input (the claimed `Message` envelope + content) | exec prompt arg / stdin | `-p "<content>"` / SDK `prompt` | adapter-provided |
| `model` | model selection | `--config model=` | `--model` / SDK `model` | adapter-provided |
| `permission` | **neutral permission enum** (`read_only` / `workspace_write` / `full_access`) | sandbox/approval flags | `--permission-mode` | adapter-provided |
| `writable_roots` | paths the turn may write | sandbox writable roots / `cwd` | `--add-dir` | adapter-provided |
| `tools` | abstract allowed-tool set | approval policy / tool config | `--allowedTools` / SDK `allowed_tools` | adapter-provided |
| `workspace` | cwd / worktree root | `cwd` | `--add-dir` / process cwd | adapter-provided |
| `mcp` | neutral MCP block (WP-6, implemented) | `--config mcp_servers.*` | `--mcp-config` / SDK `mcp_servers` | adapter-provided |
| `skill_refs` | skills to inject (Pillar 1 contract) | explicit skill input item | system-prompt injection / SDK | adapter-provided |
| `session` / `resume` | resume an existing session | `--session <id>` | `--resume <id>` / SDK `resume` | adapter-provided |
| `output` | provider-native session + ephemeral projection | `--json` / native rollout | stream-json / native session | adapter-provided |

### The Codex-vocabulary leak this spec abstracts

Today `AgentProviderConfig`
([crates/harness-core/src/lib.rs](../crates/harness-core/src/lib.rs)) and
[schemas/agent-member.schema.json](../schemas/agent-member.schema.json) carry
fields that are **Codex `app-server` parameter names mapped 1:1** into the
supposedly neutral core:

- `approval_policy`, `approvals_reviewer`
- `sandbox_policy` (with `dangerFullAccess` / `readOnly` / `workspaceWrite` values)
- `service_tier`
- `collaboration_mode`
- `developerInstructions` (Codex developer-instructions vocabulary)
- `permission_profile` of shape `{type:"profile"}`

The neutral launch spec **does not reuse these names**. It abstracts them into a
`permission` enum plus `writable_roots`, so Claude's `--permission-mode` /
`--allowedTools` and a future platform's controls map cleanly without inheriting
Codex's wire vocabulary. The operator composer and Dashboard should bind to the
neutral launch spec; each provider adapter translates to its platform. (Schema
abstraction of these fields is additive future work per ADR 0017; this document
only specifies the neutral spec it should produce.)

---

## How To Integrate a NEW Agent / Provider — the Checklist

A future implementer integrates a new platform by following these steps. This
is the concrete "define X, Y, Z" deliverable.

1. **Define Pillar 1 (base configuration).** Decide the role `prompt_ref`
   artifact and how it slots into the prompt stack; list the `skill_refs`
   (honoring the proposed skill contract); set member `capabilities`; choose
   `model` / `profile`.
2. **Define Pillar 2 (environment).** Specify `worktree_ref`,
   `runtime_workspace_roots`, `owned_paths` per task, and `workspace_policy`. If
   the platform uses MCP, write the neutral `mcp` block and how the platform
   consumes it.
3. **Implement Pillar 3 via exec-stream.** Implement `AgentProvider`
   (start / deliver / probe / ingest) over the platform's documented
   exec/stream mode; write the `EventReducer` mapping; define the health-signal
   layers; map the neutral `permission` enum; declare unsupported surfaces.
4. **Map the launch spec.** Fill the platform's column of the launch-spec table:
   how each neutral field becomes a concrete CLI flag / SDK argument. Do not
   leak platform wire vocabulary back into the neutral spec.
5. **Declare provider capabilities.** Implement the `ProviderCapabilities`
   declaration (streaming, resume, mid-turn approval, subagents, mcp, hooks,
   schema, cost, enforces_read_only) so the harness/UI can adapt and the
   Dashboard shows honest state.
6. **Write `docs/integration/<provider>.md`** from the provider template in
   [integration/README.md](integration/README.md). Answer every section:
   capability summary, runtime model, message delivery, claim/retry, event
   sources, reducer mapping, queue constraints, context packaging, permission
   model, workspace model, native multi-agent features, evidence/report
   extraction, dashboard health signals, fallback modes, unsupported surfaces,
   and validation gates. Register the new doc in
   [registry.json](registry.json) and link it from
   [integration/README.md](integration/README.md).
7. **Pass the validation gates.** `npx pnpm@9.15.4 check` must be green
   (`validate:json`, `check:schema-fixtures`, `check:tool-descriptors`,
   `check:dashboard`) and so must `harness governance check` (the doc/skill
   gates: links, registry, size, skills). Live acceptance follows the provider
   doc's own gates (e.g. Codex MVP acceptance in
   [integration/codex.md](integration/codex.md)).

## Open Gaps Flagged by This Model

| Gap | Status | Where addressed |
| --- | --- | --- |
| Skill contract (resolve / discover / inject) | WP-6: Implemented | Pillar 1, `skill_resolver` module |
| MCP neutral config shape | WP-6: Implemented | Pillar 2, `LaunchMcp` / `LaunchMcpServer` on `AgentProviderConfig` |
| Provider capability declaration | WP-6: Implemented | Pillar 3, `ProviderCapabilities` struct |
| `AgentProviderConfig` leaks Codex vocabulary | documented; abstraction is additive future work | Launch Spec |

The first three gaps are now closed. The `AgentProviderConfig` vocabulary
abstraction remains additive future work under ADR 0017.

## Non-Goals

- Do not redefine Mission/Wave, executor-native records, WorkItem, Approval or
  organization authority here; those stay in their owning contracts.
- Do not let one platform's wire vocabulary become the neutral spec.
- Do not treat provider-native subagents as durable members unless promoted.
- Do not evolve the implemented contracts (skills, MCP, capability declaration)
  by editing this document alone; changes land in code/schemas first.
