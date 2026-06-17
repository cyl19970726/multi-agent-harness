#!/usr/bin/env bash
# Deterministic FAKE provider shim generator for the multi-project verify demo.
#
# The harness spawns providers by BARE NAME (`codex` / `claude`), so prepending a
# dir holding executable shims of those names to PATH intercepts the spawn — no
# real codex/claude (and no network) is ever invoked. Each shim records the cwd
# it ran in and, if present, copies an `AGENTS.md` / `CLAUDE.md` marker from that
# cwd, then emits one harmless line so the harness reader has something to read.
#
# This mirrors crates/harness-cli/tests/fake_provider/mod.rs (the unit-test shim)
# so the live verify-fixes.sh demo proves the SAME cwd-routing guarantee end to
# end through the real binary.
#
#   install_fake_providers <bin_dir> <cwd_marker_file> <capture_marker_file>
#
# Writes `<bin_dir>/codex` and `<bin_dir>/claude`. On every spawn each shim:
#   - writes `pwd -P` to <cwd_marker_file>            (proves the spawn cwd)
#   - copies <cwd>/AGENTS.md (then CLAUDE.md) to <capture_marker_file> if present
#   - emits a single provider-shaped line and exits 0
#
# `pwd -P` resolves symlinks so the recorded path matches a canonicalized root.
# The claude shim emits a stream-json `result` line (claude --output-format
# stream-json) and the codex shim a bare JSON object (codex --json); both are
# enough to keep the respective reader happy.
#
# This file is meant to be SOURCED (it only defines a function); it deliberately
# does NOT run `set -e`, so a caller that relies on continue-on-error accounting
# (scripts/verify-fixes.sh) keeps its own shell options.

install_fake_providers() {
  local bin_dir="$1" cwd_marker="$2" cap_marker="$3"
  mkdir -p "$bin_dir"

  # --- claude shim: used by the writable WORKFLOW leaf (current_dir == worktree).
  cat >"$bin_dir/claude" <<EOF
#!/bin/sh
pwd -P > '$cwd_marker'
if [ -f AGENTS.md ]; then cat AGENTS.md > '$cap_marker';
elif [ -f CLAUDE.md ]; then cat CLAUDE.md > '$cap_marker'; fi
printf '%s\\n' '{"type":"result","subtype":"success","result":"ok"}'
exit 0
EOF

  # --- codex shim: used by the persistent MEMBER delivery (current_dir == root).
  cat >"$bin_dir/codex" <<EOF
#!/bin/sh
pwd -P > '$cwd_marker'
if [ -f AGENTS.md ]; then cat AGENTS.md > '$cap_marker';
elif [ -f CLAUDE.md ]; then cat CLAUDE.md > '$cap_marker'; fi
printf '%s\\n' '{"type":"fake"}'
exit 0
EOF

  chmod +x "$bin_dir/claude" "$bin_dir/codex"
}
