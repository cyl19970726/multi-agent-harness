# ADR 0055: Remote Node Fabric and sole cross-Node route truth

> Successor (DOC-16 row, DEV-40 flip 2026-08-18): [DOC-106](https://app.notion.com/p/3be49a4fa3798126a598e634ed5d0807) + [AF-ADR-014 (Accepted)](https://app.notion.com/p/3be49a4fa3798172a8d6c1074e2e1a67).

> **Naming partially superseded by DOC-108 (2026-08-18).** The decision was
> recorded with "Company Control Plane" / "CompanyNode" naming; DOC-16
> supersedes only that naming (now Fabric Control Plane / ExecutionNode,
> same rows and rules). The route-truth decision itself remains current.

**Date:** 2026-08-11
**Status:** accepted, implemented; Company naming superseded (DOC-108)
**Canonical contract:**
[Remote Node Fabric](../current/architecture/remote-node-fabric.md)

## Decision

1. AgentFirm has one logical Fabric Control Plane. Each machine retains the
   existing immutable ExecutionNode identity and one current NodeDaemonLease.
   The former `CompanyNode` name is retired: it was always the same row, under
   the rule `CompanyNode.id == ExecutionNode.id`. NodeGatewayLease is only a
   child of the exact NodeDaemonLease generation.
2. Nodes initiate outbound TLS 1.3 mTLS WSS to the Control Plane. They expose
   no inbound collaboration listener and use no peer-to-peer route.
3. Firm-scoped FabricStore `RoutedOperation`, `RouteAttempt`, and
   `RouteReceipt` are the sole cross-Node route truth. MessageRouteJournal may
   only project FabricStore read-only for cross-Node delivery.
4. RouteAttempt is transport evidence only. Only a generation-fenced target
   result and canonical receipt assert `not_applied`, `applied`, or `unknown`.
   Unknown forbids blind replay.
5. A message route contains the immutable canonical Message envelope or an
   authenticated content-addressed reference; the target persists it before
   MessageDelivery. A runtime route contains the immutable canonical command
   envelope and settles from the existing RuntimeCommand truth after
   NodeDaemon dispatch.
6. Source authority is closed to `node | control_plane`. Wire bytes use frozen
   protocol, schema, and deterministic canonical-JSON versions.
7. `firm-store` owns Fabric Control Plane and machine-local Fabric roots.
   Local pre-acceptance outbox, target inbox, and application result do not
   compete with FabricStore route truth.

## Context

Wave 4C established machine-owned AgentSession, RuntimeCommand, Message,
Delivery and provider-adapter authority. Multi-machine execution must extend
those contracts without letting a transport layer fabricate identity,
application success, Message content or provider effects. A daemon per Team or
an inbound listener per Node would also conflict with the one-machine/one-
NodeDaemon operational model.

## Consequences

- Offline source queue replay first reconciles through the current gateway.
  Only a Control Plane absence proof allows a pre-acceptance generation rebind.
- Store and Node-local journals need crash/torn-write tests and exact replay
  fingerprints. RecoveryRequired is an operator state, not retry permission.
- Host APIs use authenticated server-built authority and snapshot-opaque
  pagination. Browser and frame bodies cannot select actor or machine identity.
- macOS production credentials come from Keychain; CI uses strict file-backed
  credentials. An explicitly user-approved development dogfood may use
  mode-`0600` Node-local files but is labeled as a development exception and
  never counts as Keychain evidence. Control Plane backup/restore is digest-bound and never
  overwrites existing authority.
- Release acceptance includes a real three-process Control Plane + two Node
  mTLS/WSS journey and secret-free evidence, plus real two-machine dogfood when
  both machines and credentials are available.
