#!/usr/bin/env bash
set -euo pipefail

MODE="check"
if [[ "${1:-}" == "--apply" ]]; then
  MODE="apply"
elif [[ -n "${1:-}" && "${1:-}" != "--check" ]]; then
  echo "usage: $0 [--check|--apply]" >&2
  exit 2
fi

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
USER_HOME_DIR="${HOME:?HOME is required}"
PLUGIN_SELECTOR="star-harness@multi-agent-harness"
MARKETPLACE_REPO="cyl19970726/multi-agent-harness"
INSTALL_BASE="${STAR_HARNESS_INSTALL_ROOT:-${USER_HOME_DIR}/.local/lib/star-harness}"
BIN_LINK="${STAR_HARNESS_BIN_LINK:-${USER_HOME_DIR}/.local/bin/harness}"
STATE_BASE="${STAR_HARNESS_STATE_ROOT:-${USER_HOME_DIR}/.local/state/star-harness}"
APPLY_IN_PROGRESS="false"
PREVIOUS_BIN=""
STATE_FILE=""

rollback_on_error() {
  local exit_status=$?
  if [[ "${exit_status}" -eq 0 || "${APPLY_IN_PROGRESS}" != "true" ]]; then
    return
  fi
  if [[ -n "${PREVIOUS_BIN}" ]]; then
    ln -sfn "${PREVIOUS_BIN}" "${BIN_LINK}"
    echo "restored Harness link to ${PREVIOUS_BIN}" >&2
  else
    unlink "${BIN_LINK}" 2>/dev/null || true
    echo "removed incomplete Harness link ${BIN_LINK}" >&2
  fi
  if [[ -n "${STATE_FILE}" ]]; then
    node - "${STATE_FILE}" "${VERSION:-unknown}" "${REPO_ROOT}" "${PREVIOUS_BIN}" <<'NODE' || true
const fs = require("node:fs");
const [path, version, sourceRoot, rollbackBinary] = process.argv.slice(2);
fs.writeFileSync(path, `${JSON.stringify({
  schema_version: 1,
  status: "failed_and_binary_rolled_back",
  version,
  source_root: sourceRoot,
  rollback_harness_binary: rollbackBinary || null,
  failed_at: new Date().toISOString(),
}, null, 2)}\n`);
NODE
    echo "failure state: ${STATE_FILE}" >&2
  fi
}
trap rollback_on_error EXIT

for command_name in node cargo codex claude; do
  if ! command -v "${command_name}" >/dev/null 2>&1; then
    echo "missing required command: ${command_name}" >&2
    exit 1
  fi
done

VERSION="$(
  node -e \
    'console.log(JSON.parse(require("node:fs").readFileSync(process.argv[1], "utf8")).version)' \
    "${REPO_ROOT}/plugins/star-harness/.codex-plugin/plugin.json"
)"

echo "Star Harness source: ${VERSION} (${REPO_ROOT})"
(
  cd "${REPO_ROOT}"
  node scripts/sync-star-harness-plugin-skills.mjs --check
  node scripts/check-star-harness-plugin.mjs
  node scripts/check-star-harness-hook.mjs
)

echo
echo "Current Harness binary:"
if [[ -x "${BIN_LINK}" ]]; then
  ls -l "${BIN_LINK}"
  "${BIN_LINK}" member providers --json || true
else
  echo "not installed at ${BIN_LINK}"
fi

echo
echo "Current Codex Star Harness installation:"
CODEX_PLUGINS_BEFORE="$(codex plugin list)"
grep -E 'star-harness@(personal|multi-agent-harness)' <<<"${CODEX_PLUGINS_BEFORE}" || true

echo
echo "Current Claude Star Harness installation:"
CLAUDE_PLUGINS_BEFORE="$(claude plugin list)"
grep -A3 -B1 'star-harness@multi-agent-harness' <<<"${CLAUDE_PLUGINS_BEFORE}" || true

echo
echo "Kimi integration boundary:"
echo "Kimi Code has no generic plugin-management command in the reviewed CLI."
echo "Agent Team Kimi members use kimi_acp plus the Harness collaboration envelope"
echo "and skills discovered from their explicit cwd/skills directories."

if [[ "${MODE}" == "check" ]]; then
  echo
  echo "Check-only mode; run with --apply to publish this accepted source locally."
  exit 0
fi

echo
echo "Building Harness..."
(
  cd "${REPO_ROOT}"
  cargo build -p harness-cli
)

VERSION_DIR="${INSTALL_BASE}/${VERSION}"
VERSION_BIN="${VERSION_DIR}/harness"
if [[ -L "${BIN_LINK}" ]]; then
  PREVIOUS_BIN="$(readlink "${BIN_LINK}")"
fi
mkdir -p "${VERSION_DIR}" "$(dirname "${BIN_LINK}")" "${STATE_BASE}/installations"
INSTALLED_AT="$(date -u '+%Y-%m-%dT%H:%M:%SZ')"
STATE_FILE="${STATE_BASE}/installations/${INSTALLED_AT//:/-}-${VERSION}.json"
APPLY_IN_PROGRESS="true"
install -m 0755 "${REPO_ROOT}/target/debug/harness" "${VERSION_BIN}"

if [[ -e "${BIN_LINK}" && ! -L "${BIN_LINK}" ]]; then
  echo "refusing to replace non-symlink ${BIN_LINK}" >&2
  exit 1
fi
ln -sfn "${VERSION_BIN}" "${BIN_LINK}"

echo
echo "Refreshing Codex marketplace and installing one canonical owner..."
CODEX_MARKETPLACES="$(codex plugin marketplace list)"
if grep -q '^multi-agent-harness[[:space:]]' <<<"${CODEX_MARKETPLACES}"; then
  codex plugin marketplace upgrade multi-agent-harness
else
  codex plugin marketplace add "${MARKETPLACE_REPO}"
fi
if grep -q 'star-harness@personal[[:space:]].*installed' <<<"${CODEX_PLUGINS_BEFORE}"; then
  REMOVE_PERSONAL_AFTER_INSTALL="true"
else
  REMOVE_PERSONAL_AFTER_INSTALL="false"
fi
if grep -q 'star-harness@multi-agent-harness[[:space:]].*installed' <<<"${CODEX_PLUGINS_BEFORE}"; then
  codex plugin remove "${PLUGIN_SELECTOR}"
fi
if ! codex plugin add "${PLUGIN_SELECTOR}"; then
  echo "canonical Codex Plugin installation failed" >&2
  if [[ "${REMOVE_PERSONAL_AFTER_INSTALL}" == "false" ]]; then
    echo "reinstall the previous marketplace snapshot before retrying" >&2
  fi
  exit 1
fi
if [[ "${REMOVE_PERSONAL_AFTER_INSTALL}" == "true" ]]; then
  codex plugin remove star-harness@personal
fi

echo
echo "Refreshing Claude marketplace and plugin..."
CLAUDE_MARKETPLACES="$(claude plugin marketplace list)"
if grep -A1 'multi-agent-harness' <<<"${CLAUDE_MARKETPLACES}" | grep -q 'Source: Directory'; then
  claude plugin marketplace remove multi-agent-harness --scope user
  claude plugin marketplace add "${MARKETPLACE_REPO}" --scope user
elif grep -q 'multi-agent-harness' <<<"${CLAUDE_MARKETPLACES}"; then
  claude plugin marketplace update multi-agent-harness
else
  claude plugin marketplace add "${MARKETPLACE_REPO}" --scope user
fi
if grep -q 'star-harness@multi-agent-harness' <<<"${CLAUDE_PLUGINS_BEFORE}"; then
  claude plugin update "${PLUGIN_SELECTOR}" --scope user
else
  claude plugin install "${PLUGIN_SELECTOR}" --scope user
fi

node - "${STATE_FILE}" "${VERSION}" "${REPO_ROOT}" "${VERSION_BIN}" "${PREVIOUS_BIN}" "${INSTALLED_AT}" <<'NODE'
const fs = require("node:fs");
const [path, version, sourceRoot, binary, previousBinary, installedAt] = process.argv.slice(2);
fs.writeFileSync(path, `${JSON.stringify({
  schema_version: 1,
  version,
  source_root: sourceRoot,
  harness_binary: binary,
  rollback_harness_binary: previousBinary || null,
  codex_plugin: "star-harness@multi-agent-harness",
  claude_plugin: "star-harness@multi-agent-harness",
  kimi_boundary: "kimi_acp + collaboration envelope + explicit cwd skill discovery",
  installed_at: installedAt,
}, null, 2)}\n`);
NODE
APPLY_IN_PROGRESS="false"

echo
echo "Installed Star Harness ${VERSION}."
echo "State: ${STATE_FILE}"
echo "Start a new Codex task and a new Claude session to load the updated plugin."
