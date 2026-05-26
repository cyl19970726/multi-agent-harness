# Earning Engine Adapter Example

This example shows how a project can expose its tools to the generic
Multi-Agent Harness without coupling the generic core to project code.

The adapter supplies:

- tool descriptors for project CLI commands;
- evidence policy for project artifacts;
- Dashboard deep-link templates;
- permission policy for live/order/wallet actions;
- skills or prompts that teach Agent Members how to use the tools.

The descriptor in `tool-descriptors/strategy-harness-status.json` is copied
from the current Earning Engine integration plan.

## MVP Pilot

For the MVP, this adapter is the first real project pilot. It should let the
generic harness drive one bounded strategy iteration:

```text
hypothesis task
  -> strategy tool descriptor
  -> backtest/live artifact evidence
  -> critic review
  -> decision: keep / refine / kill / run bounded live
```

Strategy logic, market-specific judgment, wallet handling, and live execution
remain in LetMeTry / Earning Engine. The generic harness only owns task
coordination, evidence references, permission boundaries, and decisions.
