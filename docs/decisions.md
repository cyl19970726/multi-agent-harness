# Decisions

This file records early product decisions. Split into ADR files only when this
file becomes too long or a decision needs separate review.

## 0001: Rust Backend

Use Rust for the backend because the core is an event system, state machine, and
audit ledger. Rust is a good fit for append-only storage, concurrent agent
writes, permission gates, and typed lifecycle transitions.

## 0002: Message-First Task System

Task assignment and task reports should flow through `Message`. A message can
later materialize into `Task`, `Evidence`, or `Decision`.

## 0003: Minimal First Types

The first version should center on:

```text
AgentMember
Task
Message
Evidence
Decision
```

Other concepts such as Skill, ToolAdapter, ProviderSession, and Dashboard can
start as configuration or views until the core loop works.

## 0004: File Store Before Database

Start with append-only file-backed storage. Move to SQLite/Postgres after the
object model and query patterns are stable.
