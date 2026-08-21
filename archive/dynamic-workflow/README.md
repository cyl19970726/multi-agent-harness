# Dynamic Workflow archive

Dynamic Workflow was retired by DEV-56. This directory preserves selected
source assets only so an auditor can reproduce the accepted retirement
inventory and compare the historical files with the repository revision bound
by `specs/retirement/dynamic-workflow-bound-register.v1.json`.

Nothing here is an executable product surface:

- package scripts, CI, installers, plugins, runtime commands, APIs, MCP, and the
  Dashboard must not reference this directory;
- `.star`, evaluator, adapter, and authoring-skill files are historical inputs,
  not supported examples or installable Skills;
- restoration requires a new accepted product decision and a normal reviewed
  implementation Task. Copying a file out of this directory does not restore
  any authority or compatibility promise.

`specs/retirement/dynamic-workflow-completion.v1.json` records why each group
was retained and its deterministic tree hash. The completion check also maps
the archived files back to the per-file Git blob ids in the accepted 189-row
register.
