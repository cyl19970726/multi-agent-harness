# DeepSeek Harness member runner

This process is the reviewed native bridge between Star Harness and
`deepseek-ai/deepseek-harness` 0.1.1-rc.2. It owns one DSH `AgentHandle` and
one provider-native Session ID. Harness sends coordination inputs over NDJSON;
the runner emits only input receipts, terminal summaries, and lifecycle facts.
DSH's append-only Session store remains the sole transcript and tool-event
authority.

The first integration phase is `host_driven`. DSH Goal and provider-driven
continuation are deliberately not loaded. `close` disposes the live handle but
retains the exact native Session ID so a later runtime generation uses
`ctx.agents.resume`.
