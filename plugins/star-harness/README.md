# Star Harness Plugin

This is the unified, provider-neutral distribution package for Codex and Kimi.
It installs experience adapters only:

- generated mirrors of the canonical Host and Member skills;
- optional Harness MCP registration over the same application services as CLI;
- Mission/Team shortcuts and historical Kimi command aliases;
- fail-open lifecycle telemetry plus bounded SessionStart/UserPromptSubmit
  active-run and Host Inbox orientation; and
- Dashboard deep-link guidance.

Product architecture, messages, Mission/Wave state, TeamRun lifecycle, and
provider capability review remain in Harness. Provider transcripts, tool
activity, and subagent history remain in provider-native sessions.

The skill directories under `plugins/star-harness/skills/` are generated. Edit
their canonical sources under `skills/`, then run:

```bash
node scripts/sync-star-harness-plugin-skills.mjs
node scripts/sync-star-harness-plugin-skills.mjs --check
```

This repository package does not change a user's plugin installation,
Marketplace, MCP configuration, or provider version. Installation and provider
upgrades are separate, explicitly confirmed operations.

Kimi namespaces plugin commands with the plugin id. After installing this
unified package, the commands are:

```text
/star-harness:mission-new
/star-harness:team-start
/star-harness:team-status
/star-harness:new-run
/star-harness:status
/star-harness:dashboard
```

The last three preserve the historical command basenames. The retired
`agent-team` namespace cannot remain registered by the new `star-harness`
plugin id without installing a second compatibility plugin, which this package
intentionally avoids so Skills and hooks have one owner.
