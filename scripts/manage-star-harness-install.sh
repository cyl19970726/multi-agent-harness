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
PREVIOUS_BIN_PRESENT="false"
PREVIOUS_BIN_IDENTITY=""
BIN_LINK_PUBLICATION_ARMED="false"
PUBLISHED_BIN_TARGET=""
PUBLISHED_BIN_IDENTITY=""
ROLLBACK_BINARY_STATUS="failed_before_binary_publication"
STATE_FILE=""
BIN_LINK_LOCK_DIR="${BIN_LINK}.star-harness-install.lock"
BIN_LINK_LOCK_OWNED="false"
BIN_LINK_LOCK_STATUS="not_acquired"

acquire_bin_link_lock() {
  mkdir -p "$(dirname "${BIN_LINK}")"
  if ! mkdir "${BIN_LINK_LOCK_DIR}" 2>/dev/null; then
    echo "Harness binary publication is already owned by another installer: ${BIN_LINK_LOCK_DIR}" >&2
    return 1
  fi
  BIN_LINK_LOCK_OWNED="true"
  BIN_LINK_LOCK_STATUS="held"
}

release_bin_link_lock() {
  if [[ "${BIN_LINK_LOCK_OWNED}" == "true" ]]; then
    if ! rmdir "${BIN_LINK_LOCK_DIR}"; then
      BIN_LINK_LOCK_STATUS="release_failed"
      echo "failed to release Harness binary publication lock: ${BIN_LINK_LOCK_DIR}" >&2
      return 1
    fi
    BIN_LINK_LOCK_OWNED="false"
    BIN_LINK_LOCK_STATUS="released"
  fi
}

bin_link_identity() {
  # The template literal is evaluated by Node, not the shell.
  # shellcheck disable=SC2016
  node -e '
    const fs = require("node:fs");
    const stat = fs.lstatSync(process.argv[1], { bigint: true });
    process.stdout.write(`${stat.dev}:${stat.ino}`);
  ' "$1"
}

prepare_bin_link_publication() {
  if [[ -e "${BIN_LINK}" && ! -L "${BIN_LINK}" ]]; then
    echo "refusing to replace non-symlink ${BIN_LINK}" >&2
    return 1
  fi
  if [[ -L "${BIN_LINK}" ]]; then
    PREVIOUS_BIN="$(readlink "${BIN_LINK}")"
    PREVIOUS_BIN_IDENTITY="$(bin_link_identity "${BIN_LINK}")"
    PREVIOUS_BIN_PRESENT="true"
  else
    PREVIOUS_BIN=""
    PREVIOUS_BIN_IDENTITY=""
    PREVIOUS_BIN_PRESENT="false"
  fi
}

publish_bin_link() {
  local target=$1
  local current_identity=""
  if [[ "${PREVIOUS_BIN_PRESENT}" == "true" ]]; then
    if [[ -L "${BIN_LINK}" ]]; then
      current_identity="$(bin_link_identity "${BIN_LINK}" 2>/dev/null || true)"
    fi
    if [[ "${current_identity}" != "${PREVIOUS_BIN_IDENTITY}" || "$(readlink "${BIN_LINK}" 2>/dev/null || true)" != "${PREVIOUS_BIN}" ]]; then
      echo "refusing to publish over changed Harness link ${BIN_LINK}" >&2
      return 1
    fi
  elif [[ -e "${BIN_LINK}" || -L "${BIN_LINK}" ]]; then
    echo "refusing to publish over newly occupied Harness path ${BIN_LINK}" >&2
    return 1
  fi

  PUBLISHED_BIN_TARGET="${target}"
  BIN_LINK_PUBLICATION_ARMED="true"
  ln -sfn "${target}" "${BIN_LINK}"
  PUBLISHED_BIN_IDENTITY="$(bin_link_identity "${BIN_LINK}")"
}

rollback_published_bin_link() {
  local current_identity=""
  if [[ "${BIN_LINK_PUBLICATION_ARMED}" != "true" ]]; then
    ROLLBACK_BINARY_STATUS="failed_before_binary_publication"
    return
  fi
  if [[ -L "${BIN_LINK}" ]]; then
    current_identity="$(bin_link_identity "${BIN_LINK}" 2>/dev/null || true)"
  fi
  if [[ "${current_identity}" != "${PUBLISHED_BIN_IDENTITY}" || "$(readlink "${BIN_LINK}" 2>/dev/null || true)" != "${PUBLISHED_BIN_TARGET}" ]]; then
    ROLLBACK_BINARY_STATUS="failed_after_binary_publication_link_changed"
    echo "preserved changed Harness path ${BIN_LINK}; it is no longer owned by this install" >&2
    return
  fi
  if [[ "${PREVIOUS_BIN_PRESENT}" == "true" ]]; then
    if ! ln -sfn "${PREVIOUS_BIN}" "${BIN_LINK}"; then
      ROLLBACK_BINARY_STATUS="failed_after_binary_publication_restore_failed"
      echo "failed to restore Harness link ${BIN_LINK}; published link remains residual" >&2
      return 1
    fi
    ROLLBACK_BINARY_STATUS="failed_and_previous_binary_restored"
    echo "restored Harness link to ${PREVIOUS_BIN}" >&2
  else
    if ! unlink "${BIN_LINK}"; then
      ROLLBACK_BINARY_STATUS="failed_after_binary_publication_remove_failed"
      echo "failed to remove published Harness link ${BIN_LINK}; link remains residual" >&2
      return 1
    fi
    ROLLBACK_BINARY_STATUS="failed_and_created_binary_link_removed"
    echo "removed incomplete Harness link ${BIN_LINK}" >&2
  fi
}

rollback_binary_after_error() {
  local exit_status=$1
  if [[ "${exit_status}" -eq 0 || "${APPLY_IN_PROGRESS}" != "true" ]]; then
    return
  fi
  rollback_published_bin_link || true
}

write_failure_state() {
  local original_exit_status=$1
  local failure_status="${ROLLBACK_BINARY_STATUS}"
  if [[ "${BIN_LINK_LOCK_STATUS}" == "release_failed" ]]; then
    failure_status="failed_with_install_lock_residual"
  fi
  if [[ -n "${STATE_FILE}" ]]; then
    node - "${STATE_FILE}" "${VERSION:-unknown}" "${REPO_ROOT}" "${PREVIOUS_BIN}" "${failure_status}" "${ROLLBACK_BINARY_STATUS}" "${BIN_LINK_LOCK_STATUS}" "${original_exit_status}" <<'NODE' || true
const fs = require("node:fs");
const [path, version, sourceRoot, rollbackBinary, status, binaryRollbackStatus, installLockStatus, originalExitStatus] = process.argv.slice(2);
fs.writeFileSync(path, `${JSON.stringify({
  schema_version: 1,
  status,
  binary_rollback_status: binaryRollbackStatus,
  install_lock_status: installLockStatus,
  original_exit_status: Number(originalExitStatus),
  version,
  source_root: sourceRoot,
  rollback_harness_binary: rollbackBinary || null,
  failed_at: new Date().toISOString(),
}, null, 2)}\n`);
NODE
    echo "failure state: ${STATE_FILE}" >&2
  fi
}

finish_install() {
  local exit_status=$?
  trap - EXIT
  rollback_binary_after_error "${exit_status}"
  if ! release_bin_link_lock && [[ "${exit_status}" -eq 0 ]]; then
    exit_status=1
  fi
  if [[ "${exit_status}" -ne 0 ]]; then
    write_failure_state "${exit_status}"
  fi
  exit "${exit_status}"
}

trap finish_install EXIT

for command_name in node npm cargo codex claude; do
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
  node scripts/check-cross-layer-consistency.mjs
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
echo "Candidate Firm binary:"
CANDIDATE_BIN="${REPO_ROOT}/target/debug/firm"
if [[ -x "${CANDIDATE_BIN}" ]]; then
  "${CANDIDATE_BIN}" --build-info
else
  echo "not built at ${CANDIDATE_BIN}; run cargo build -p firm-cli"
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
echo "Current Kimi Code Star Harness installation:"
KIMI_CODE_HOME="${KIMI_CODE_HOME:-${USER_HOME_DIR}/.kimi-code}"
KIMI_MANAGED_DIR="${KIMI_CODE_HOME}/plugins/managed/star-harness"
if [[ -f "${KIMI_MANAGED_DIR}/kimi.plugin.json" ]]; then
  KIMI_INSTALLED_VERSION="$(node -e "console.log(JSON.parse(require('fs').readFileSync(process.argv[1],'utf8')).version||'?')" "${KIMI_MANAGED_DIR}/kimi.plugin.json" 2>/dev/null || echo "?")"
  echo "  installed v${KIMI_INSTALLED_VERSION} at ${KIMI_MANAGED_DIR}"
else
  echo "  not installed (run --apply to install)"
fi

if [[ "${MODE}" == "check" ]]; then
  echo
  echo "Check-only mode; run with --apply to publish this accepted source locally."
  exit 0
fi

acquire_bin_link_lock
prepare_bin_link_publication

echo
echo "Building Harness..."
(
  cd "${REPO_ROOT}"
  cargo build -p firm-cli
)

VERSION_DIR="${INSTALL_BASE}/${VERSION}"
VERSION_BIN="${VERSION_DIR}/harness"
MARKETPLACE_SNAPSHOT="${VERSION_DIR}/marketplace"
CLAUDE_RUNNER_INSTALL="${VERSION_DIR}/apps/claude-member-runner"
DEEPSEEK_RUNNER_INSTALL="${VERSION_DIR}/apps/deepseek-member-runner"
mkdir -p "${VERSION_DIR}" "$(dirname "${BIN_LINK}")" "${STATE_BASE}/installations"
INSTALLED_AT="$(date -u '+%Y-%m-%dT%H:%M:%SZ')"
STATE_FILE="${STATE_BASE}/installations/${INSTALLED_AT//:/-}-${VERSION}.json"
APPLY_IN_PROGRESS="true"
install -m 0755 "${REPO_ROOT}/target/debug/firm" "${VERSION_BIN}"

case "${MARKETPLACE_SNAPSHOT}" in
  "${INSTALL_BASE}/"*) ;;
  *)
    echo "refusing to replace marketplace snapshot outside ${INSTALL_BASE}" >&2
    exit 1
    ;;
esac
rm -rf "${MARKETPLACE_SNAPSHOT}"
mkdir -p "${MARKETPLACE_SNAPSHOT}/.claude-plugin" "${MARKETPLACE_SNAPSHOT}/plugins"
install -m 0644 \
  "${REPO_ROOT}/.claude-plugin/marketplace.json" \
  "${MARKETPLACE_SNAPSHOT}/.claude-plugin/marketplace.json"
cp -R "${REPO_ROOT}/plugins/star-harness" "${MARKETPLACE_SNAPSHOT}/plugins/"

case "${CLAUDE_RUNNER_INSTALL}" in
  "${VERSION_DIR}/"*) ;;
  *)
    echo "refusing to replace Claude runner outside ${VERSION_DIR}" >&2
    exit 1
    ;;
esac
rm -rf "${CLAUDE_RUNNER_INSTALL}"
mkdir -p "$(dirname "${CLAUDE_RUNNER_INSTALL}")"
cp -R "${REPO_ROOT}/apps/claude-member-runner" "${CLAUDE_RUNNER_INSTALL}"
npm install \
  --prefix "${CLAUDE_RUNNER_INSTALL}" \
  --omit=dev \
  --no-audit \
  --no-fund \
  --ignore-scripts

case "${DEEPSEEK_RUNNER_INSTALL}" in
  "${VERSION_DIR}/"*) ;;
  *)
    echo "refusing to replace DeepSeek Harness runner outside ${VERSION_DIR}" >&2
    exit 1
    ;;
esac
rm -rf "${DEEPSEEK_RUNNER_INSTALL}"
mkdir -p "$(dirname "${DEEPSEEK_RUNNER_INSTALL}")"
cp -R "${REPO_ROOT}/apps/deepseek-member-runner" "${DEEPSEEK_RUNNER_INSTALL}"
npm ci \
  --prefix "${DEEPSEEK_RUNNER_INSTALL}" \
  --omit=dev \
  --no-audit \
  --no-fund \
  --ignore-scripts

publish_bin_link "${VERSION_BIN}"

echo
echo "Refreshing Codex marketplace and installing one canonical owner..."
CODEX_MARKETPLACES="$(codex plugin marketplace list)"
if grep -q '^multi-agent-harness[[:space:]]' <<<"${CODEX_MARKETPLACES}"; then
  codex plugin marketplace remove multi-agent-harness
fi
if ! codex plugin marketplace add "${MARKETPLACE_REPO}" \
  --sparse .claude-plugin \
  --sparse plugins/star-harness; then
  echo "Codex Git marketplace refresh failed; using accepted local snapshot." >&2
  codex plugin marketplace add "${MARKETPLACE_SNAPSHOT}"
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
if grep -q 'multi-agent-harness' <<<"${CLAUDE_MARKETPLACES}"; then
  claude plugin marketplace remove multi-agent-harness --scope user
fi
if ! claude plugin marketplace add "${MARKETPLACE_REPO}" \
  --scope user \
  --sparse .claude-plugin plugins/star-harness; then
  echo "Claude Git marketplace refresh failed; using accepted local snapshot." >&2
  claude plugin marketplace add "${MARKETPLACE_SNAPSHOT}" --scope user
fi
CLAUDE_PLUGINS_AFTER_MARKETPLACE="$(claude plugin list)"
if grep -q 'star-harness@multi-agent-harness' <<<"${CLAUDE_PLUGINS_AFTER_MARKETPLACE}"; then
  claude plugin update "${PLUGIN_SELECTOR}" --scope user
else
  claude plugin install "${PLUGIN_SELECTOR}" --scope user
fi

	echo
	echo "Installing Kimi Code plugin..."
	mkdir -p "${KIMI_MANAGED_DIR}/scripts" "${KIMI_MANAGED_DIR}/skills" "${KIMI_MANAGED_DIR}/commands"
	# Direct Kimi installs are upgraded in place, so explicitly remove the retired
	# Harness coordination MCP registration before publishing the CLI-only plugin.
	rm -f "${KIMI_MANAGED_DIR}/.mcp.json"
	cp "${REPO_ROOT}/plugins/star-harness/kimi.plugin.json" "${KIMI_MANAGED_DIR}/"
	cp "${REPO_ROOT}/plugins/star-harness/scripts/star-harness-hook.sh" "${KIMI_MANAGED_DIR}/scripts/"
	cp -R "${REPO_ROOT}/plugins/star-harness/skills/" "${KIMI_MANAGED_DIR}/skills/"
	cp -R "${REPO_ROOT}/plugins/star-harness/commands/" "${KIMI_MANAGED_DIR}/commands/"
	echo "  installed ${VERSION} to ${KIMI_MANAGED_DIR}"
	echo "  Run /reload in Kimi Code to activate the plugin."

node - "${STATE_FILE}" "${VERSION}" "${REPO_ROOT}" "${VERSION_BIN}" "${PREVIOUS_BIN}" "${INSTALLED_AT}" "${KIMI_MANAGED_DIR}" <<'NODE'
const fs = require("node:fs");
const [path, version, sourceRoot, binary, previousBinary, installedAt, kimiPlugin] = process.argv.slice(2);
fs.writeFileSync(path, `${JSON.stringify({
  schema_version: 1,
  version,
  source_root: sourceRoot,
  harness_binary: binary,
  rollback_harness_binary: previousBinary || null,
  codex_plugin: "star-harness@multi-agent-harness",
  claude_plugin: "star-harness@multi-agent-harness",
  kimi_plugin: kimiPlugin,
  installed_at: installedAt,
}, null, 2)}\n`);
NODE
APPLY_IN_PROGRESS="false"

echo
echo "Installed Star Harness ${VERSION}."
echo "State: ${STATE_FILE}"
echo "Start a new Codex task, a new Claude session, and run /reload in Kimi Code to activate."
