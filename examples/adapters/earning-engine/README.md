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
