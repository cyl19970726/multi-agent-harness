#!/usr/bin/env bash
# Acceptance for the current Star Harness skill installer. It proves the
# collaboration package installs atomically and the retired star-workflow
# package fails before either agent target is touched.
set -uo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
pass=0
fail=0
ok()  { echo "  ✓ $1"; pass=$((pass + 1)); }
bad() { echo "  ✗ $1"; fail=$((fail + 1)); }

work="$(mktemp -d)"
cleanup() { rm -rf "$work"; }
trap cleanup EXIT

echo "== A1: default collaboration install is complete =="
project="$work/project"
mkdir -p "$project"
if bash "$repo_root/scripts/install-skill.sh" --agent both --dest "$project" >/dev/null 2>&1; then
  ok "default install succeeded"
else
  bad "default install failed"
fi
for name in collaborate-as-agent-team-member shared-references; do
  for root in .claude/skills .agents/skills; do
    if [ -f "$project/$root/$name/SKILL.md" ] && [ ! -L "$project/$root/$name" ]; then
      ok "$root/$name installed as real files"
    else
      bad "$root/$name missing or a symlink"
    fi
  done
done

echo "== A2: retired explicit Skill fails without partial writes =="
retired_project="$work/retired-project"
mkdir -p "$retired_project"
retired_output="$(bash "$repo_root/scripts/install-skill.sh" \
  --agent both --dest "$retired_project" --skill star-workflow 2>&1)"
retired_status=$?
if [ "$retired_status" -ne 0 ] && [[ "$retired_output" == *"star-workflow is retired"* ]]; then
  ok "--skill star-workflow fails explicitly"
else
  bad "--skill star-workflow was not rejected as retired"
fi
if [ ! -e "$retired_project/.claude/skills" ] && [ ! -e "$retired_project/.agents/skills" ]; then
  ok "retired Skill rejection leaves both targets untouched"
else
  bad "retired Skill rejection partially wrote a target"
fi

echo "== A3: multi-Skill preflight is atomic =="
missing_repo="$work/missing-repo"
missing_project="$work/missing-project"
mkdir -p "$missing_repo/scripts" "$missing_repo/skills" "$missing_project"
cp "$repo_root/scripts/install-skill.sh" "$missing_repo/scripts/install-skill.sh"
cp -R "$repo_root/skills/." "$missing_repo/skills"
mv "$missing_repo/skills/shared-references" "$work/withheld-shared-references"
if bash "$missing_repo/scripts/install-skill.sh" --agent both \
  --dest "$missing_project" --suite collaboration >/dev/null 2>&1; then
  bad "collaboration suite accepted a missing delegated Skill"
else
  ok "collaboration suite rejects a missing delegated Skill"
fi
if [ ! -e "$missing_project/.claude/skills" ] && [ ! -e "$missing_project/.agents/skills" ]; then
  ok "failed suite preflight leaves both targets untouched"
else
  bad "failed suite preflight partially wrote a target"
fi

echo "== A4: Kimi guidance is version-bound and does not install =="
kimi_dest="$work/kimi-dest"
kimi_home="$work/kimi-home"
mkdir -p "$kimi_dest/.agents/skills" "$kimi_home/skills"
printf '%s\n' 'preserve shared package' > "$kimi_dest/.agents/skills/sentinel"
printf '%s\n' 'preserve Kimi package' > "$kimi_home/skills/sentinel"
cp -R "$kimi_dest" "$work/kimi-dest-before"
cp -R "$kimi_home" "$work/kimi-home-before"
for scope in project user; do
  kimi_output="$(KIMI_CODE_HOME="$kimi_home" bash "$repo_root/scripts/install-skill.sh" \
    --agent kimi --scope "$scope" --dest "$kimi_dest" 2>&1)"
  kimi_status=$?
  if [ "$kimi_status" -eq 0 ] \
    && [[ "$kimi_output" == *"Kimi Code 0.39.0"* ]] \
    && [[ "$kimi_output" == *'$KIMI_CODE_HOME/skills'* ]] \
    && [[ "$kimi_output" == *"~/.agents/skills"* ]] \
    && [[ "$kimi_output" == *"Project skills take precedence over user skills"* ]] \
    && [[ "$kimi_output" == *"--skills-dir <path> replaces default user/project discovery"* ]] \
    && [[ "$kimi_output" == *"--agent codex --scope user --suite collaboration"* ]]; then
    ok "Kimi $scope guidance describes verified discovery and shared installation"
  else
    bad "Kimi $scope guidance is incomplete or failed"
  fi
  if diff -r "$work/kimi-dest-before" "$kimi_dest" >/dev/null \
    && diff -r "$work/kimi-home-before" "$kimi_home" >/dev/null; then
    ok "Kimi $scope guidance leaves destination and Kimi home untouched"
  else
    bad "Kimi $scope guidance wrote skill packages"
  fi
done

echo ""
echo "acceptance: $pass passed, $fail failed"
[ "$fail" = "0" ]
