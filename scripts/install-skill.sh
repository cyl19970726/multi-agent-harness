#!/usr/bin/env bash
# Install current Star Harness collaboration skills into a target project
# (or your user-level library) for Claude Code and/or Codex.
#
# The default kit is the collaboration suite. Dynamic Workflow and its
# star-workflow authoring Skill are retired and cannot be installed.
#
#   Claude Code reads skills from   <base>/.claude/skills/<name>/
#   Codex      reads skills from     <base>/.agents/skills/<name>/
#
# Usage:
#   scripts/install-skill.sh [--agent claude|codex|both|kimi] [--scope project|user] \
#       [--dest <base-dir>] [--skill <name> ...] [--suite <name> ...]
#
#   --agent   which agent's skill dir to install into       (default: claude)
#             claude | codex | both  → copy skills to the respective fixed dirs.
#             kimi                   → prints the Kimi Code skill model and exits;
#                                      Kimi loads skills from cwd/--skills-dir, not
#                                      a fixed install directory.
#   --scope   project = <cwd>, user = $HOME                  (default: project)
#   --dest    explicit base dir (overrides --scope)
#   --skill   install an explicit current skill directory (repeatable)
#   --suite   install a named skill suite (repeatable; currently: collaboration)
#   --repo    git url to clone when run standalone           (default: this project)
#   --ref     git ref to clone                               (default: master)
#
# Run from a clone (copies the local skills) OR standalone via curl:
#   curl -fsSL https://raw.githubusercontent.com/cyl19970726/multi-agent-harness/master/scripts/install-skill.sh | bash -s -- --agent both
#
# The copy is a snapshot: it does not track the repository. Re-run this script
# after pulling, or (inside this repository) use the .agents/skills symlinks,
# which always resolve to the current source. A copy
# without a references/ directory beside SKILL.md predates the two-role
# contract and shadows the current skill until refreshed.
set -euo pipefail

# Default shipped skills; --skill may select another current source directory.
DEFAULT_SKILLS="collaborate-as-agent-team-member shared-references"
SKILLS=""
SUITES=""
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
    --suite) SUITES="${SUITES:+$SUITES }$2"; shift 2 ;;
    --repo)  REPO="$2"; shift 2 ;;
    --ref)   REF="$2"; shift 2 ;;
    -h|--help) sed -n '2,30p' "$0"; exit 0 ;;
    *) echo "unknown arg: $1" >&2; exit 2 ;;
  esac
done

expand_suite() {
  case "$1" in
    # The company-os suite was retired by DOC-108; its operator skills live
    # only in git history (ADR 0063) and are no longer installable.
    collaboration)
      echo "collaborate-as-agent-team-member shared-references"
      ;;
    *)
      echo "unknown suite: $1 (available: collaboration)" >&2
      exit 2
      ;;
  esac
}

for suite in $SUITES; do
  SKILLS="${SKILLS:+$SKILLS }$(expand_suite "$suite")"
done

# Default to the current collaboration skills when no selection was given.
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
if [ -n "$SCRIPT_DIR" ] && [ -f "$SCRIPT_DIR/../skills/collaborate-as-agent-team-member/SKILL.md" ]; then
  SKILLS_ROOT="$(cd "$SCRIPT_DIR/../skills" && pwd)"
else
  TMP="$(mktemp -d)"
  trap 'rm -rf "$TMP"' EXIT
  echo "fetching skills from $REPO ($REF)…"
  git clone --depth 1 --branch "$REF" "$REPO" "$TMP/repo" >/dev/null 2>&1
  SKILLS_ROOT="$TMP/repo/skills"
fi

# Validate the complete selection before writing either target. This keeps a
# suite with a missing delegated Skill from being installed only partially.
for name in $SKILLS; do
  case "$name" in
    star-workflow)
      echo "star-workflow is retired and is not installable" >&2
      exit 1
      ;;
    *[!A-Za-z0-9_-]*|"")
      echo "invalid skill name: $name" >&2
      exit 2
      ;;
  esac
  [ -f "$SKILLS_ROOT/$name/SKILL.md" ] \
    || { echo "could not find the skill source at $SKILLS_ROOT/$name" >&2; exit 1; }
done

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

if [ "$AGENT" = "kimi" ]; then
  cat >&2 <<'KIMIEOF'
Kimi Code skill model (divergence from Claude Code / Codex)
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
Kimi CLI does not currently expose a generic plugin-management command.
It discovers skills from one of two locations:

  1. The current working directory (cwd) when a session starts.
  2. An explicit --skills-dir <path> argument.

There is no fixed per-agent install directory like ~/.kimi/skills/.
To use Star Harness skills with Kimi:

  • Place the skill directories directly in your project root.
  • Start a Kimi Code session inside that directory.
  • Or pass --skills-dir <path> pointing at the skills root.

Star Harness ships no Kimi plugin (ADR 0063 retired the plugin package).
Copy or symlink skills/collaborate-as-agent-team-member and
skills/shared-references into the directory you start Kimi from, or pass
--skills-dir pointing at a checkout's skills/ directory.
KIMIEOF
  exit 0
fi

for name in $SKILLS; do
  case "$AGENT" in
    claude) install_into ".claude/skills" "Claude Code" "$name" ;;
    codex)  install_into ".agents/skills" "Codex" "$name" ;;
    both)   install_into ".claude/skills" "Claude Code" "$name"; install_into ".agents/skills" "Codex" "$name" ;;
    *) echo "--agent must be claude|codex|both|kimi" >&2; exit 2 ;;
  esac
done

echo ""
echo "Next: build + start the harness service, then use the installed collaboration skills with an Agent Team."
echo "  cargo build -p firm-cli && ./target/debug/firm serve --addr 127.0.0.1:8787"
