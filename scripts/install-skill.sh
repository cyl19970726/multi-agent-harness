#!/usr/bin/env bash
# Install an optional Star Harness authoring skill into a target project
# (or your user-level library) for Claude Code and/or Codex.
#
# The default kit ships star-workflow.
#
#   Claude Code reads skills from   <base>/.claude/skills/<name>/
#   Codex      reads skills from     <base>/.agents/skills/<name>/
#
# Usage:
#   scripts/install-skill.sh [--agent claude|codex|both] [--scope project|user] \
#       [--dest <base-dir>] [--skill <name> ...]
#
#   --agent   which agent's skill dir to install into       (default: claude)
#   --scope   project = <cwd>, user = $HOME                  (default: project)
#   --dest    explicit base dir (overrides --scope)
#   --skill   install an explicit skill directory (repeatable; default: star-workflow)
#   --repo    git url to clone when run standalone           (default: this project)
#   --ref     git ref to clone                               (default: master)
#
# Run from a clone (copies the local skills) OR standalone via curl:
#   curl -fsSL https://raw.githubusercontent.com/cyl19970726/multi-agent-harness/master/scripts/install-skill.sh | bash -s -- --agent both
set -euo pipefail

# Default shipped skill; --skill may select an explicit source directory.
DEFAULT_SKILLS="star-workflow"
SKILLS=""
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
    --skill) SKILLS="${SKILLS:+$SKILLS }$2"; shift 2 ;;
    --repo)  REPO="$2"; shift 2 ;;
    --ref)   REF="$2"; shift 2 ;;
    -h|--help) sed -n '2,28p' "$0"; exit 0 ;;
    *) echo "unknown arg: $1" >&2; exit 2 ;;
  esac
done

# Default to the standalone Dynamic Workflow skill when no --skill was given.
[ -n "$SKILLS" ] || SKILLS="$DEFAULT_SKILLS"

# Base dir the skill dirs are created under.
if [ -z "$DEST" ]; then
  case "$SCOPE" in
    project) DEST="$(pwd)" ;;
    user)    DEST="$HOME" ;;
    *) echo "--scope must be project|user" >&2; exit 2 ;;
  esac
fi

# Locate the source skills root: prefer a local clone (this script lives in
# scripts/, the skills in skills/); otherwise clone the repo to a temp dir.
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]:-$0}")" 2>/dev/null && pwd || true)"
SKILLS_ROOT=""
TMP=""
if [ -n "$SCRIPT_DIR" ] && [ -f "$SCRIPT_DIR/../skills/star-workflow/SKILL.md" ]; then
  SKILLS_ROOT="$(cd "$SCRIPT_DIR/../skills" && pwd)"
else
  TMP="$(mktemp -d)"
  trap 'rm -rf "$TMP"' EXIT
  echo "fetching skills from $REPO ($REF)…"
  git clone --depth 1 --branch "$REF" "$REPO" "$TMP/repo" >/dev/null 2>&1
  SKILLS_ROOT="$TMP/repo/skills"
fi

# Copy one skill's real files into <base>/<subdir>/<name>. Deref the repo
# symlink (.agents/skills/<name> may be a symlink) with cp -RL so the install is
# always real files, never a symlink.
install_into() {
  local subdir="$1" label="$2" name="$3"
  local src="$SKILLS_ROOT/$name"
  local target="$DEST/$subdir/$name"
  [ -f "$src/SKILL.md" ] || { echo "could not find the skill source at $src" >&2; exit 1; }
  mkdir -p "$(dirname "$target")"
  rm -rf "$target"
  cp -RL "$src" "$target"
  echo "✓ installed $name for $label → $target"
}

for name in $SKILLS; do
  case "$AGENT" in
    claude) install_into ".claude/skills" "Claude Code" "$name" ;;
    codex)  install_into ".agents/skills" "Codex" "$name" ;;
    both)   install_into ".claude/skills" "Claude Code" "$name"; install_into ".agents/skills" "Codex" "$name" ;;
    *) echo "--agent must be claude|codex|both" >&2; exit 2 ;;
  esac
done

echo ""
echo "Next: build + start the harness service, then ask your agent to author a workflow."
echo "  cargo build -p harness-cli && ./target/debug/harness serve --addr 127.0.0.1:8787"
