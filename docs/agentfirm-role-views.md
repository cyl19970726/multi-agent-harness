# AgentFirm local RoleViews

The local product loop reads five server-built projections. They are views over
the canonical Execution Space Store; they are not new ledgers or lifecycles.

| Responsibility | Endpoint |
| --- | --- |
| Company aggregate | `GET /v1/views/company-work` |
| Team shared workspace | `GET /v1/views/team-workspace/{team_id}` |
| Host coordination | `GET /v1/views/host-console/{team_id}` |
| Member execution | `GET /v1/views/member-workbench/{member_run_id}` |
| Node operator | `GET /v1/views/operator/{node_id}` |

Every response uses `agentfirm.role_views.v1`, carries the exact source Store
identity and canonical event sequence, and exposes explicit freshness,
attention, and allowed-action records. The JSON Schemas under
`schemas/role-views/agentfirm.role_views.v1/` are the wire authority. The
versioned action mapping is `schemas/role-views/role-action-manifest.v1.json`.

## Authentication and desktop bootstrap

RoleView requests and canonical mutations use a server-resolved local
capability in `X-AgentFirm-Token`. The Dashboard keeps the injected capability
in memory only:

```js
window.__AGENTFIRM_BOOTSTRAP__ = {
  apiBase: "http://127.0.0.1:8787",
  capabilityToken: "<opaque local capability>"
};
```

Request bodies and identity-shaped headers cannot select the actor. Mutations
also require `Idempotency-Key` and exact `If-Match`. `/v1/meta` provides the
build SHA, protocol, RoleView schema, action-manifest version and auth scheme so
an embedded desktop host can fail closed on a handshake mismatch.

## Query and invalidation

Company Work accepts only the closed filter set documented by the schema. The
default order is `updated_at desc, work_id asc`; its cursor is bound to filters,
sort, and the source event sequence. SSE is invalidation-only for the canonical
trust ledger. Clients refetch the relevant RoleView instead of folding events.

## Verification

```bash
pnpm check:role-views
pnpm check:wave4-zero-match
pnpm acceptance:wave4:deterministic
```

Read builders do not initialize or reconcile a Store. The focused Rust purity
test proves an empty/nonexistent Store remains absent after a Company RoleView
read.
