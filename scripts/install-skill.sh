#!/usr/bin/env bash
# Install the multi-agent-harness `author-workflow` skill into a target project
# (or your user-level library) for Claude Code and/or Codex.
#
#   Claude Code reads skills from   <base>/.claude/skills/<name>/
#   Codex      reads skills from     <base>/.agents/skills/<name>/
#
# Usage:
#   scripts/install-skill.sh [--agent claude|codex|both] [--scope project|user] [--dest <base-dir>]
#
#   --agent   which agent's skill dir to install into       (default: claude)
#   --scope   project = <cwd>, user = $HOME                  (default: project)
#   --dest    explicit base dir (overrides --scope)
#   --repo    git url to clone when run standalone           (default: this project)
#   --ref     git ref to clone                               (default: master)
#
# Run from a clone (copies the local skill) OR standalone via curl:
#   curl -fsSL https://raw.githubusercontent.com/cyl19970726/multi-agent-harness/master/scripts/install-skill.sh | bash -s -- --agent both
set -euo pipefail

SKILL_NAME="author-workflow"
AGENT="claude"
SCOPE="project"
DEST=""
REPO="https://github.com/cyl19970726/multi-agent-harness.git"
REF="master"

while [ $# -gt 0 ]; do
  case "$1" in
    --agent) AGENT="$2"; shift 2 ;;
    --scope) SCOPE="$2"; shift 2 ;;
    --dest)  DEST="$2"; shift 2 ;;
    --repo)  REPO="$2"; shift 2 ;;
    --ref)   REF="$2"; shift 2 ;;
    -h|--help) sed -n '2,20p' "$0"; exit 0 ;;
    *) echo "unknown arg: $1" >&2; exit 2 ;;
  esac
done

# Base dir the skill dirs are created under.
if [ -z "$DEST" ]; then
  case "$SCOPE" in
    project) DEST="$(pwd)" ;;
    user)    DEST="$HOME" ;;
    *) echo "--scope must be project|user" >&2; exit 2 ;;
  esac
fi

# Locate the source skill: prefer a local clone (this script lives in scripts/,
# the skill in skills/); otherwise clone the repo to a temp dir.
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]:-$0}")" 2>/dev/null && pwd || true)"
SRC=""
TMP=""
if [ -n "$SCRIPT_DIR" ] && [ -f "$SCRIPT_DIR/../skills/$SKILL_NAME/SKILL.md" ]; then
  SRC="$(cd "$SCRIPT_DIR/../skills/$SKILL_NAME" && pwd)"
else
  TMP="$(mktemp -d)"
  trap 'rm -rf "$TMP"' EXIT
  echo "fetching $SKILL_NAME from $REPO ($REF)…"
  git clone --depth 1 --branch "$REF" "$REPO" "$TMP/repo" >/dev/null 2>&1
  SRC="$TMP/repo/skills/$SKILL_NAME"
fi
[ -f "$SRC/SKILL.md" ] || { echo "could not find the skill source at $SRC" >&2; exit 1; }

install_into() {
  local subdir="$1" label="$2"
  local target="$DEST/$subdir/$SKILL_NAME"
  mkdir -p "$(dirname "$target")"
  rm -rf "$target"
  cp -R "$SRC" "$target"
  echo "✓ installed $SKILL_NAME for $label → $target"
}

case "$AGENT" in
  claude) install_into ".claude/skills" "Claude Code" ;;
  codex)  install_into ".agents/skills" "Codex" ;;
  both)   install_into ".claude/skills" "Claude Code"; install_into ".agents/skills" "Codex" ;;
  *) echo "--agent must be claude|codex|both" >&2; exit 2 ;;
esac

echo ""
echo "Next: build + start the harness service, then ask your agent to author a workflow."
echo "  cargo build -p harness-cli && ./target/debug/harness serve --addr 127.0.0.1:8787"
