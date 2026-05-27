# 0004: File Store Before Database

## Decision

Start with append-only file-backed storage.

## Rationale

File-backed JSONL keeps source of truth inspectable while object contracts and
query patterns are still changing.

## Consequences

Move to SQLite/Postgres only after query patterns, concurrency needs, and API
read models are stable.
