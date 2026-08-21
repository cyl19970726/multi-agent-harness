# A one-node writable workflow leaf for the multi-project verify demo.
#
# `writable = True` isolates the worker into a throwaway git worktree UNDER the
# selected project's project_root (<project_root>/.harness/worktrees/...), and
# the claude worker is spawned with that worktree as its cwd. Run against a
# git-backed project A while the harness process cwd is DELIBERATELY elsewhere,
# this proves the leaf's cwd derives from the SELECTED project — not the harness
# process cwd — and that the worker reads project A's AGENTS.md marker. With the
# fake provider shim on PATH this needs no real claude and no network.
workflow("mp-leaf", "a writable leaf roots its worktree at the selected project, reading that project's AGENTS.md marker")
phase("edit")
agent("read AGENTS.md and report", provider = "claude", writable = True, label = "leaf")
