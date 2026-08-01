# Provider Capacity and Authentication Preflight

Capacity answers one question: **can this provider account execute a turn right
now, in this execution mode?** It is not adapter compatibility, and it is not a
binary-presence check.

This document is canonical for the capacity contract, the per-provider truth
matrix, the start guard, and the CLI/Dashboard projection. The provider-neutral
runtime substrate stays in [../agent-runtime.md](../agent-runtime.md); each
provider's own adapter document keeps implementing its execution mode.

## Why This Exists

Live Wave 2 evidence proved adapter compatibility is not runtime availability:

- Kimi returned a quota `403` while its adapter was reviewed and current.
- Claude's local auth metadata reported logged-in while the standalone SDK
  returned `403 Request not allowed`, because the Harness process had no
  `HTTP(S)_PROXY` and this host's direct egress to the API is blocked. The
  identical request succeeded through the proxy
  (`apps/claude-member-runner/FINDINGS.md` §F).

Both members were "compatible" and neither could work. A member that consumes
its Assignment and then cannot execute burns the Assignment, not just the turn.

## The Two Axes Must Not Merge

| Axis | Question | Record | Moves when |
| --- | --- | --- | --- |
| Compatibility | Is this adapter reviewed against the installed provider version? | `ProviderIntegrationProfile.compatibility_status` | The provider binary/SDK version changes |
| Capacity | Can this account execute now? | `ProviderCapacitySnapshot` | The account's quota, credential, or runtime context changes |

They are siblings everywhere they are reported. A reviewed-`current` adapter
with an `exhausted` account is a normal, expressible state; so is an `unknown`
adapter with an `available` account. Neither axis may be derived from the other,
and `ProviderCapacitySnapshot` carries no compatibility field.

## The Snapshot

`ProviderCapacitySnapshot` (`crates/harness-core/src/lib.rs`) is
provider-neutral and execution-mode-specific:

| Field | Meaning |
| --- | --- |
| `provider` / `execution_mode` | Which product this claim is about. `codex_exec` and `codex_app_server` are different products. |
| `account` | The account/source boundary: `chatgpt`, `api_key`, `amazon_bedrock`, `oauth_credentials_file`, `signed_out`, `unknown`, plus a non-secret identifier and plan when the provider returns them. |
| `state` | `available` \| `limited` \| `exhausted` \| `unauthorized` \| `unknown` |
| `observed_at` / `observed_unix_ms` | When the observation was made. Staleness is computed from the millisecond stamp, so a stored snapshot cannot look fresh. |
| `reset_at` | When the blocking window reopens, only when the provider says so and only for a state a reset would clear. |
| `evidence_source` | `provider_quota_api` \| `auth_metadata` \| `execution_canary` \| `provider_error` \| `not_exposed` \| `probe_failed` \| `none` |
| `confidence` | `observed` \| `inferred` \| `unknown` |
| `windows` | Provider-reported usage windows only. An adapter never synthesises `used_percent`. |
| `diagnosis` | Why a failure happened when the cause is runtime context rather than an account limit. |
| `runtime_context` | Non-secret environment facts. Credential keys record presence only; proxy and base URLs are reduced to `scheme://host:port`, because a corporate proxy routinely embeds `user:password@` and a gateway URL can carry a token. |

### Honesty Rules

1. `unknown` is the default and never means available.
2. An absent snapshot is not availability; it is the absence of an observation.
3. No adapter invents a percentage, a reset, or a plan the provider did not
   report.
4. Local auth metadata proves a credential exists, never that a request would
   succeed.
5. A missing proxy is diagnosed as missing proxy, not as an exhausted or
   unauthorized account.

## Provider Truth Matrix

| Provider · execution mode | Reviewed capacity source | Default state | Reports usage numbers | Notes |
| --- | --- | --- | --- | --- |
| `codex` · `codex_app_server` | `account/read` + `account/rateLimits/read` over the app-server stdio protocol | `available` \| `limited` \| `exhausted` \| `unauthorized` from the provider answer | Yes — every metered bucket in `rateLimitsByLimitId`, keyed by provider `limit_id` | Both RPCs are reads. The preflight completes `initialize` + `initialized` and stops: no `thread/start`, `thread/resume`, or `thread/name/set`, so no rollout and no billable turn. |
| `claude` · `claude_agent_sdk` | Local auth metadata, observed runtime context, and an opt-in real request | `unknown` without `--canary`; `available` only from a canary that actually succeeded | **No.** Anthropic does not permit third-party products to surface claude.ai rate limits without prior approval (`apps/claude-member-runner/README.md`), so `windows` stays empty by contract. | The canary is a bounded `claude -p` request. It shares credentials and HTTP egress with the Agent SDK mode but is not the SDK runtime, and the snapshot's `detail` says so. |
| `kimi` · `kimi_acp` | None | `unknown`, `evidence_source: not_exposed` — always | No | The reviewed ACP surface is `initialize` and `session/{new,resume,load,set_config_option,prompt,cancel,update,request_permission}`. None reports quota. ACP also has no HTTP-status error channel, so a 403 is indistinguishable from any other JSON-RPC error and MUST NOT be inferred from its text. Kimi capacity has no observable source today. |
| any other provider | None | `unknown`, `evidence_source: not_exposed` | No | An unregistered provider never inherits another provider's answer. |

### Codex Classification

A live payload carries several metered buckets (`codex`, `codex_bengalfox`, …).
Buckets are **not** interchangeable, so every state signal is read from ONE
bucket: the account-level `rateLimits` mirror, or the single keyed bucket when
that is all the provider returned. Several buckets with no account mirror are
not attributable and stay `unknown`. All buckets still appear in `windows` so
an operator can see which one is hot.

Within that bucket, in order: a non-null `rateLimitReachedType`, then
`spendControlReached`, then the highest `usedPercent` (`>= 100` is `exhausted`,
`>= 90` is `limited`). A bucket with no parsable window stays `unknown` — no
window is not proof of headroom. A signed-out account while `requiresOpenaiAuth`
is `unauthorized`, never `exhausted`. `reset_at` is the time the LAST
constraining window reopens, because the account is usable again only then.

Only the reviewed Agent Team mode is probed, so `--execution-mode` accepts only
that mode; asking for another is refused rather than answered with this mode's
observation under a different label.

### Terminal Provider Errors as Evidence

A recorded terminal failure can make an account known-unavailable, but **only
from provider-STRUCTURED metadata** — never from prose.

The recorded `provider_error` action carries the transport's own fields in
`MemberAction.provider_status` as `provider_terminal:<reason>:<http status>`.
The classifier reads only that token: an exact HTTP status integer (401/403 →
`unauthorized`, 429 → `exhausted`) and a closed reason vocabulary
(`rate_limit`, `rate_limit_reached`, `usage_limit_reached`, `quota_exceeded`,
`credits_depleted`, `auth_error`, `authentication_error`, `forbidden`,
`unauthorized`). Anything else stays unclassified rather than becoming a gate.

The action `summary` is never scanned. It embeds the MEMBER's own first line, so
a member writing "fixed the 403 handler" would otherwise mark its account
unauthorized; substring matching also cannot tell `403` from `1403`.

Which modes can produce one:

| Execution mode | Structured terminal metadata | Can gate a start |
| --- | --- | --- |
| `claude_agent_sdk` | Yes — `terminal_reason` + `api_error_status` from the SDK result | Yes |
| `kimi_acp` | **No.** A 403 arrives as free-form JSON-RPC error text with no status field, and a real terminal failure is journalled as `action_type=error`, not `provider_error`. | No — Kimi capacity is always `unknown` |
| `codex_app_server` | No — adapter error strings. It does not need one: it has a reviewed quota API. | No (uses the quota API instead) |

The search walks backwards past rows it cannot classify, so a silent-round row
sitting on top never buries a real 403 recorded moments earlier. Only failures
newer than the TTL count.

## Start Guard

Before an Agent Member claims its Assignment, `team-run start` observes capacity
and decides:

```text
block  <=> the snapshot is FRESH and its state is exhausted or unauthorized
proceed <=> anything else (no snapshot, unknown, available, limited, stale)
```

Freshness uses `PROVIDER_CAPACITY_DEFAULT_TTL_MS` (5 minutes), overridable with
`HARNESS_CAPACITY_TTL_MS`. A future-dated or unstamped observation is treated as
unknown, never fresh.

The block is the product fact and the journal is its record, not its gate: if a
ledger write fails after the decision, the member is still refused and the
failure is appended to the reason. Failing open there would let a store hiccup
start the member on the very account the gate just refused.

A blocked member:

- runs the guard **before** the adapter claims anything, so its Assignment stays
  `queued`, `attempt: 0`, with no `claim_id` and no `provider_receipt_id`, and
  is still deliverable after the provider recovers;
- records a failed `provider_unavailable` MemberAction naming the execution
  mode, state, evidence source, confidence, and any diagnosis;
- folds `provider_unavailable` onto the MemberRun and TeamRun event log;
- becomes `blocked`, never `completed`, and emits no Handoff;
- never opens a native session.

`HARNESS_CAPACITY_PREFLIGHT=off` disables only the probe. That produces no
snapshot, and no snapshot never blocks — the honest-unknown semantics are
unchanged.

## No Empty Completed Handoff

A round that ends without an agent message is a provider failure wearing a
different mask. `parse_round_result("")` reads as `Done`, so an unclassified
terminal provider error would otherwise publish an empty Handoff and a
`completed` action no member ever wrote.

Both a classified provider error and an empty final report now record a failed
`provider_error` MemberAction and no Handoff. The MemberRun stays `idle` and
reconstructable: MemberRun id, Standing Agent link, correlation, Workspace
snapshot, and the resumable `NativeSessionRef` are all preserved, so the Host
can re-deliver rather than re-create.

The silence rule fires at the ONE place Harness would mint a handoff, so it
cannot mislabel the two legitimate text-less rounds: a round deferred behind
newer inbound mail still records `continued`, and a member that published its
own Handoff through `team-run send` still records `completed` and keeps its
evidence refs.

## CLI

```bash
harness member preflight --json
harness member preflight --provider codex --json
harness member preflight --provider claude --canary --timeout-s 120 --json
harness member preflight --json --fail-on-unavailable
```

With `--json` each row reports `capacity`, `capacity_freshness`,
`start_decision`, and `compatibility` as **siblings**; without it the same
fields print as one line per provider, still in separate columns. Freshness is
read from the clock AFTER the probe, so a probe that takes seconds never
reports its own answer as stale.

`--fail-on-unavailable` exits non-zero when any provider returns a fresh
known-unavailable state; it never fails on `unknown`.

`harness member providers` remains the adapter-compatibility inventory and is
unchanged.

## Dashboard Projection

The snapshot is persisted on `MemberRun.provider_capacity`, so it reaches the
Dashboard through the existing `member_runs` projection with no new endpoint.
The Member Run Focus `RuntimeSummary` module shows capacity beside — never
merged into — resume/version compatibility, and must render `unknown` as
unknown rather than as healthy.

## Tests

| Concern | Test |
| --- | --- |
| Snapshot/state/freshness/decision rules | `crates/harness-core/src/lib.rs` (`capacity_*`, `fresh_known_unavailable_capacity_blocks_start`, `unknown_absent_and_stale_capacity_never_block_and_never_claim_available`) |
| Codex payload parsing, thresholds, signed-out, no invented numbers | `crates/harness-cli/src/codex_app_server.rs` tests |
| Claude proxy diagnosis, Kimi/Codex never fabricating capacity, structured-only classification, member text never classifying, URL redaction, TTL expiry, silence | `crates/harness-cli/src/main.rs` tests |
| End-to-end preflight, start guard, queued-Assignment preservation, capacity-vs-compatibility separation | `crates/harness-cli/tests/provider_capacity_preflight.rs` |

## Known Limits

- The Claude canary exercises the bundled CLI path, not the Agent SDK runtime.
  It is honest about that in `detail`; a true SDK-runtime canary would need a
  runner entry point that does not create a native session.
- Codex capacity is read for the signed-in account of the Harness process. A
  member pinned to a different account is not yet modelled.
- Kimi stays `unknown` until Moonshot exposes a reviewed quota API. Do not
  approximate it from local logs.
