# ADR 0069: Retire the AgentIdentity Projection and the Dead Message-Plane Records

```text
status: accepted; implemented by the 2026-09-14 B1 cleanup
date: 2026-09-14
amends: AF-ADR-014 same-ID identity cutover; ADR 0039 durable mailbox delivery; ADR 0055 remote node fabric
canonical_for: whether an AgentIdentity type/projection/schema exists; whether SubscriptionCursor and MessageRouteJournal exist
```

## Context

Three records outlived their purpose, in the same way and for the same reason:
each was kept as a *compatibility* or *planned* shape, and each then failed to
acquire the one thing that would have made it real — a writer, a reader, or
both.

### AgentIdentity

AF-ADR-014 made `AgentMember` the sole durable agent identity and demoted
`AgentIdentity` to a deprecated same-ID read-only projection. The demotion was
honest but incomplete: the name survived as a `struct`, two Store projection
functions, an always-`Err` `create_agent_identity`, a
`migrate_legacy_agent_identity_same_id` writer used only as a test fixture, a
JSON Schema with fixtures, a required `runtime_fabric.agent_identities` RoleView
field, a dashboard `AgentIdentity` interface, and two gate pins.

Every one of those was a projection of `AgentMember`:

| Fact | Evidence at `584c8cc5` |
| --- | --- |
| No AgentIdentity writer exists | `create_agent_identity` returns `AGENT_IDENTITY_READ_ONLY` unconditionally |
| No AgentIdentity ledger exists | zero `agent_identities.jsonl` under `~/.harness/`, the `.visual-evidence/` stores, or the tree |
| Every projected row was an AgentMember | `fabric_agent_identities` maps `trust_agent_members` field for field |
| The only Rust caller of the migration writer was test setup | 64 call sites, all under `crates/firm-store/src/trust_kernel_tests/` |
| Nothing read the RoleView field | zero readers of `runtime_fabric.agent_identities` or `snapshot.agent_identities` in the dashboard |

The one load-bearing reader was the StartSession permission-ceiling fence,
which resolved a projected `AgentIdentity` only to read
`permission_ceiling` — a field it can read from `AgentMember` directly.

Pre-cutover stores do hold history under the old name: 37 trust-journal
envelopes with `"aggregate_kind":"agent_identity"` in five old Execution
Spaces, plus rows that spell the foreign key `agent_identity_id`. That history
is why the serde aliases stay.

### SubscriptionCursor

`SubscriptionCursor` was written on every ack — and could never be read back.
Both writers committed it as an **immutable side record** of the
`message_delivery_ack` / `external_message_delivery_ack` aggregate, then
tried to read the previous value with
`latest_trust_envelopes_unlocked(space, "subscription_cursor")`, which filters
by *aggregate kind*. No envelope has that kind, so `current_cursor` was always
`None` and every ack in the history of the product wrote the same row:
`(1, 1, 1, revision 1)`. The live stores agree — all 41 persisted cursor side
records carry `"cursor_revision":1`.

Nothing read the cursor: not the inboxes (they project
`CanonicalMessageDelivery`), not the RoleViews, not the Dashboard, not the CLI.
`docs/current/architecture/agent-runtime.md` claimed "Current inboxes use
`CanonicalMessageDelivery` and `SubscriptionCursor`". They do not.

Repairing the read was considered and rejected. The three counters are named
`last_visible_store_sequence`, `last_delivered_store_sequence` and
`last_read_store_sequence`, but the code increments each by one per ack: they
are an ack count, not a store sequence. A repaired cursor would advance
*and still be wrong*. The honest fix for a record with no reader and wrong
arithmetic is to delete it, not to make it move.

### MessageRouteJournal

`MessageRouteJournal` and `RouteJournalStatus` had no writer and no reader at
all. ADR 0055 already ruled that `FabricStore` operations, attempts and
receipts are the sole cross-Node route truth, and
`crates/firm-cli/src/remote_fabric.rs` states in a comment that the journal is
not written there. Zero rows exist in any store. It was a name in a module.

## Decision

1. **`AgentIdentity` is retired, not deprecated.** The struct, both Store
   projection functions, `create_agent_identity`,
   `migrate_legacy_agent_identity_same_id`, `schemas/agent-identity.schema.json`
   with its fixtures, the `runtime_fabric.agent_identities` RoleView field, the
   Dashboard `AgentIdentity` interface and snapshot field, and the two gate
   pins are deleted. `AgentMember` is the only identity root on every surface.

2. **The StartSession permission-ceiling fence keeps its exact semantics on
   `AgentMember`.** It now reads `trust_agent_members` and refuses with
   `StartSession cannot widen the frozen AgentMember permission ceiling`. The
   gate still pins that string, and a Store test pins it verbatim alongside the
   zero-side-effect assertions. Retiring a name never softens a fence.

3. **`SubscriptionCursor` is deleted**: type, validation, both write sites, the
   schema and its fixtures, the parity assertion, the gate entries, and the two
   doc claims. Recipient progress is `CanonicalMessageDelivery` status, which
   is the record the inboxes actually project.

4. **`MessageRouteJournal` and `RouteJournalStatus` are deleted** with their
   schema, fixtures and gate entry. This is a dead type in
   `firm-core/src/agentfirm_api/messaging.rs`, not Fabric code: everything under
   `crates/firm-fabric`, `crates/firm-cli/src/fabric_runtime/`,
   `crates/firm-store/src/collaboration/` and `schemas/collaboration/` is
   untouched, and `scripts/acceptance-remote-fabric.mjs` keeps its negative
   assertion that no writable cross-node journal authority exists.

5. **Two persisted string formats are frozen, not renamed.** The StartSession
   HTTP route derives the session id from a fingerprint whose key is
   `"identity"`, and stamps
   `permission_envelope_ref = "agent-identity:{id}:permission:v{version}"`.
   Both values are inside durable `AgentSession` rows and inside the
   `RuntimeCommand` payload fingerprint, so renaming them would change the
   derived session id and the fingerprint of an otherwise identical request.
   They keep their historical spelling exactly, the same treatment ADR 0067 and
   ADR 0068 gave retired RuntimeCommand kinds.

## Read tolerance

Retirement never costs a store its history.

- **Five serde aliases stay** and are now covered by name:
  `AgentSession.agent_member_id`, `TeamMembership.agent_member_id`,
  `WorkExecutionBinding.agent_member_id`,
  `MessageRecipientKind::AgentMember` and
  `ProviderNativeEventRecord.agent_member_id` each keep
  `#[serde(alias = "agent_identity…")]`.
  `crates/firm-core/tests/legacy_identity_aliases.rs` and one case in
  `crates/firm-provider-events/tests/persisted_readers.rs` prove every one of
  them decodes a legacy row **and** re-serializes only the canonical
  `agent_member` spelling, so no writer can reintroduce the retired name.
- **Historical envelopes are left exactly as written.** The 37
  `agent_identity` aggregate envelopes and the 41 cursor side records stay in
  the trust journal byte for byte. No reader folded them before this ADR — the
  cursor read was structurally incapable of matching, and nothing ever folded
  an `agent_identity` aggregate into identity state — so no operator loses
  history and no export path is added, because there is nothing an export could
  contain that the journal does not already hold.
- **The Stage A legacy export is untouched.** The 13 legacy member rows in
  `members.jsonl` remain the `legacy_member_identity` section of
  `harness legacy-company-os export|verify`; `crates/firm-cli/src/legacy_company_os.rs`
  keeps its `AgentIdentityCompatibility` ledger section, its
  `agent-identity-compat-ledger-v1` digest and its `agent_identities.jsonl`
  ledger name, because those are the names of *historical* evidence.
- **The Dashboard keeps one legacy fallback.** `TeamMembership.agent_identity_id`
  stays optional in `apps/agent-dashboard/src/types.ts` as the browser-side
  twin of the serde alias; `AgentSession` and `WorkExecutionBinding` had the
  field spelled `agent_identity_id` as *required*, which never matched what the
  server emits, and are corrected to `agent_member_id`.

## RoleView schema version

`runtime_fabric.agent_identities` is removed from
`schemas/role-views/agentfirm.role_views.v1/common.schema.json` **within v1**,
with no version bump. That follows the repository's own convention rather than
inventing one: `agentfirm.role_views.v1` has existed since `5c0258fa` and has
never been bumped, and required properties have been removed from it in place
before — `3f9b69ec`, `56dd0d4a` and `27a275ad` each did exactly this. The
contract is local and shipped in lockstep: the server constant
(`role_views_api.rs::SCHEMA_VERSION`), the Dashboard types, the six
`wave4-local-agentfirm-v1` fixtures and the contract check all move in one
commit, and `/v1/meta` still lets an embedded desktop host fail closed on a
handshake mismatch.

## Consequences

- One identity name on every surface. A reader of AGENTS.md, the runtime doc,
  the data model, the mental model, the collaboration Skill, the schemas or the
  Dashboard now finds `AgentMember` and nothing else.
- The message plane no longer advertises two records it cannot produce. What is
  left — `Message`, `MessageSubscription`, `CanonicalMessageDelivery` — is the
  set that actually has writers and readers.
- Deleting rather than reserving means a document carrying `AgentIdentity`,
  `SubscriptionCursor` or `MessageRouteJournal` as a *typed* object now fails
  decoding instead of implying a record no writer produces. Rows that merely
  spell a field `agent_identity_id` still decode, by design.
