# Self-Hosting MVP Runtime Hardening

This case records the self-hosting MVP hardening path as a reusable example.
It is not the current MVP spec; the current spec lives in
[../../../docs/mvp.md](../../../docs/mvp.md).

## Scenario

The goal was to make Multi-Agent Harness harder to fake. Static snapshots,
dry-run delivery, and hand-written JSONL were useful diagnostics, but they did
not prove that persistent Agent Members could receive work, produce evidence,
be reviewed, and appear in Dashboard state.

## Key Lessons

- Provider delivery must update `Message.delivery` and `ProviderSession`, not
  just print stdout.
- `thread/start` and `turn/start` responses must be parsed instead of
  fabricating thread ids.
- Review gates should reject fake evidence ids, missing source refs, failed
  checks, missing critic findings, stale failed provider sessions, and
  owned-path violations.
- Dashboard risk visibility needs failed message/session counts, provider
  session visibility, runtime health, and child-thread visibility.
- Adapter pilots should distinguish command availability from evidence-backed
  domain decisions.

## Useful Follow-Up

Future Lead Agents should reuse the pattern:

```text
static object contract
  -> real provider delivery
  -> evidence hardening
  -> Dashboard visibility
  -> external adapter pilot
  -> goal evaluation
```

When a later run claims "MVP complete", it must point to current executable
gates and fresh evidence, not this historical case.
