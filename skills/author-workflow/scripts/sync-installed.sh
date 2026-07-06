#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source_skill="$(cd "$script_dir/.." && pwd)"

codex_home="${CODEX_HOME:-$HOME/.codex}"
targets=(
  "$codex_home/skills/author-workflow"
  "$HOME/.agents/skills/author-workflow"
)

validator="${SKILL_VALIDATOR:-$HOME/.codex/skills/.system/skill-creator/scripts/quick_validate.py}"

for target in "${targets[@]}"; do
  mkdir -p "$target"
  rsync -a --delete "$source_skill/" "$target/"
  diff -qr "$source_skill" "$target"
  if [[ -x "$validator" || -f "$validator" ]]; then
    python3 "$validator" "$target"
  fi
done

if [[ -x "$validator" || -f "$validator" ]]; then
  python3 "$validator" "$source_skill"
fi
