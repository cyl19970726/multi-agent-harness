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
INSTALL_COMPLETED="false"
PREVIOUS_BIN=""
PREVIOUS_BIN_PRESENT="false"
PREVIOUS_BIN_IDENTITY=""
PREVIOUS_BIN_OBJECT_IDENTITY=""
BIN_LINK_PUBLICATION_ARMED="false"
PUBLISHED_BIN_TARGET=""
PUBLISHED_BIN_IDENTITY=""
ROLLBACK_BINARY_STATUS="failed_before_binary_publication"
STATE_FILE=""
BIN_LINK_LOCK_DIR="${BIN_LINK}.star-harness-install.lock"
PREVIOUS_BIN_WITNESS="${BIN_LINK_LOCK_DIR}/previous-link-witness"
PUBLISHED_BIN_STAGED="${BIN_LINK_LOCK_DIR}/published-link-staged"
PUBLISHED_BIN_WITNESS="${BIN_LINK_LOCK_DIR}/published-link-witness"
DISPLACED_BIN_ENTRY="${BIN_LINK_LOCK_DIR}/displaced-live-entry"
ROLLBACK_BIN_ENTRY="${BIN_LINK_LOCK_DIR}/rollback-live-entry"
BIN_LINK_LOCK_OWNED="false"
BIN_LINK_LOCK_STATUS="not_acquired"
PRESERVE_BIN_LINK_TRANSACTION="false"

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
    if [[ "${PRESERVE_BIN_LINK_TRANSACTION}" == "true" ]]; then
      BIN_LINK_LOCK_STATUS="rollback_residual_preserved"
      echo "preserved Harness binary publication lock and ownership witnesses after rollback failure: ${BIN_LINK_LOCK_DIR}" >&2
      return 1
    fi
    local artifact
    for artifact in \
      "${PREVIOUS_BIN_WITNESS}" \
      "${PUBLISHED_BIN_STAGED}" \
      "${PUBLISHED_BIN_WITNESS}" \
      "${DISPLACED_BIN_ENTRY}" \
      "${ROLLBACK_BIN_ENTRY}"; do
      if [[ -e "${artifact}" || -L "${artifact}" ]]; then
        if ! unlink "${artifact}"; then
          BIN_LINK_LOCK_STATUS="release_failed"
          echo "failed to remove Harness binary publication artifact: ${artifact}" >&2
          return 1
        fi
      fi
    done
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
    process.stdout.write(`${stat.dev}:${stat.ino}:${stat.ctimeNs}:${stat.birthtimeNs}`);
  ' "$1"
}

bin_link_object_identity() {
  # A publication witness keeps this symlink inode alive, so dev:ino cannot be
  # reused until the transaction releases the witness.
  # shellcheck disable=SC2016
  node -e '
    const fs = require("node:fs");
    const stat = fs.lstatSync(process.argv[1], { bigint: true });
    process.stdout.write(`${stat.dev}:${stat.ino}`);
  ' "$1"
}

path_matches_object() {
  local path=$1
  local expected_identity=$2
  local expected_target=$3
  [[ -L "${path}" ]] || return 1
  [[ "$(bin_link_object_identity "${path}" 2>/dev/null || true)" == "${expected_identity}" ]] || return 1
  [[ "$(readlink "${path}" 2>/dev/null || true)" == "${expected_target}" ]]
}

path_matches_previous_snapshot() {
  local path=$1
  [[ "${PREVIOUS_BIN_PRESENT}" == "true" && -L "${path}" ]] || return 1
  [[ "$(bin_link_identity "${path}" 2>/dev/null || true)" == "${PREVIOUS_BIN_IDENTITY}" ]] || return 1
  [[ "$(readlink "${path}" 2>/dev/null || true)" == "${PREVIOUS_BIN}" ]]
}

link_without_replace() {
  # BSD and GNU ln both use -P to hard-link the symlink object itself. Without
  # -f this is an atomic no-replace operation at the destination path.
  ln -P "$1" "$2"
}

move_to_transaction_entry() {
  node -e 'require("node:fs").renameSync(process.argv[1], process.argv[2])' "$1" "$2"
}

restore_quarantined_entry() {
  local entry=$1
  local entry_identity=""
  local restore_status=0
  if [[ ! -e "${entry}" && ! -L "${entry}" ]]; then
    return 1
  fi
  entry_identity="$(bin_link_object_identity "${entry}" 2>/dev/null || true)"
  link_without_replace "${entry}" "${BIN_LINK}" || restore_status=$?
  if [[ -n "${entry_identity}" && "$(bin_link_object_identity "${BIN_LINK}" 2>/dev/null || true)" == "${entry_identity}" ]]; then
    return 0
  fi
  if [[ "${restore_status}" -ne 0 ]]; then
    return "${restore_status}"
  fi
  return 1
}

restore_previous_without_overwrite() {
  local restore_status=0
  link_without_replace "${PREVIOUS_BIN_WITNESS}" "${BIN_LINK}" || restore_status=$?
  if path_matches_object "${BIN_LINK}" "${PREVIOUS_BIN_OBJECT_IDENTITY}" "${PREVIOUS_BIN}"; then
    return 0
  fi
  if [[ "${restore_status}" -ne 0 ]]; then
    return "${restore_status}"
  fi
  return 1
}

prepare_bin_link_publication() {
  if [[ -e "${BIN_LINK}" || -L "${BIN_LINK}" ]]; then
    link_without_replace "${BIN_LINK}" "${PREVIOUS_BIN_WITNESS}"
    if [[ ! -L "${PREVIOUS_BIN_WITNESS}" ]]; then
      echo "refusing to replace non-symlink ${BIN_LINK}" >&2
      return 1
    fi
    PREVIOUS_BIN="$(readlink "${PREVIOUS_BIN_WITNESS}")"
    PREVIOUS_BIN_IDENTITY="$(bin_link_identity "${PREVIOUS_BIN_WITNESS}")"
    PREVIOUS_BIN_OBJECT_IDENTITY="$(bin_link_object_identity "${PREVIOUS_BIN_WITNESS}")"
    PREVIOUS_BIN_PRESENT="true"
  else
    PREVIOUS_BIN=""
    PREVIOUS_BIN_IDENTITY=""
    PREVIOUS_BIN_OBJECT_IDENTITY=""
    PREVIOUS_BIN_PRESENT="false"
  fi
}

publish_bin_link() {
  local target=$1
  local displace_status=0
  local publish_status=0
  ln -s "${target}" "${PUBLISHED_BIN_STAGED}"
  link_without_replace "${PUBLISHED_BIN_STAGED}" "${PUBLISHED_BIN_WITNESS}"
  PUBLISHED_BIN_TARGET="${target}"
  PUBLISHED_BIN_IDENTITY="$(bin_link_object_identity "${PUBLISHED_BIN_WITNESS}")"
  BIN_LINK_PUBLICATION_ARMED="true"

  if [[ "${PREVIOUS_BIN_PRESENT}" == "true" ]]; then
    if ! path_matches_previous_snapshot "${BIN_LINK}"; then
      echo "refusing to publish over changed Harness link ${BIN_LINK}" >&2
      return 1
    fi
    move_to_transaction_entry "${BIN_LINK}" "${DISPLACED_BIN_ENTRY}" || displace_status=$?
    if path_matches_object "${DISPLACED_BIN_ENTRY}" "${PREVIOUS_BIN_OBJECT_IDENTITY}" "${PREVIOUS_BIN}"; then
      if [[ "${displace_status}" -ne 0 ]]; then
        if ! restore_previous_without_overwrite; then
          PRESERVE_BIN_LINK_TRANSACTION="true"
          echo "failed to reconcile uncertain Harness link displacement; ownership evidence is preserved in ${BIN_LINK_LOCK_DIR}" >&2
        fi
        return "${displace_status}"
      fi
    elif [[ -e "${DISPLACED_BIN_ENTRY}" || -L "${DISPLACED_BIN_ENTRY}" ]]; then
      if ! restore_quarantined_entry "${DISPLACED_BIN_ENTRY}"; then
        PRESERVE_BIN_LINK_TRANSACTION="true"
        echo "failed to restore concurrently changed Harness path; ownership evidence is preserved in ${BIN_LINK_LOCK_DIR}" >&2
      fi
      echo "refusing to publish over concurrently changed Harness link ${BIN_LINK}" >&2
      if [[ "${displace_status}" -ne 0 ]]; then
        return "${displace_status}"
      fi
      return 1
    elif path_matches_previous_snapshot "${BIN_LINK}"; then
      echo "failed to displace the previous Harness link" >&2
      if [[ "${displace_status}" -ne 0 ]]; then
        return "${displace_status}"
      fi
      return 1
    else
      echo "refusing to publish after the Harness path changed concurrently" >&2
      if [[ "${displace_status}" -ne 0 ]]; then
        return "${displace_status}"
      fi
      return 1
    fi
  fi

  link_without_replace "${PUBLISHED_BIN_STAGED}" "${BIN_LINK}" || publish_status=$?
  if path_matches_object "${BIN_LINK}" "${PUBLISHED_BIN_IDENTITY}" "${PUBLISHED_BIN_TARGET}"; then
    if [[ "${publish_status}" -ne 0 ]]; then
      return "${publish_status}"
    fi
    return 0
  fi

  if [[ "${PREVIOUS_BIN_PRESENT}" == "true" ]] && ! restore_previous_without_overwrite; then
    PRESERVE_BIN_LINK_TRANSACTION="true"
    echo "failed to restore the previous Harness link after publication refusal; ownership evidence is preserved in ${BIN_LINK_LOCK_DIR}" >&2
  fi
  echo "refusing to publish over occupied Harness path ${BIN_LINK}" >&2
  if [[ "${publish_status}" -ne 0 ]]; then
    return "${publish_status}"
  fi
  return 1
}

rollback_published_bin_link() {
  if [[ "${BIN_LINK_PUBLICATION_ARMED}" != "true" ]]; then
    ROLLBACK_BINARY_STATUS="failed_before_binary_publication"
    return
  fi
  if ! path_matches_object "${BIN_LINK}" "${PUBLISHED_BIN_IDENTITY}" "${PUBLISHED_BIN_TARGET}"; then
    if [[ "${PREVIOUS_BIN_PRESENT}" == "true" ]] && path_matches_object "${BIN_LINK}" "${PREVIOUS_BIN_OBJECT_IDENTITY}" "${PREVIOUS_BIN}"; then
      ROLLBACK_BINARY_STATUS="failed_before_binary_publication"
      return
    elif [[ "${PREVIOUS_BIN_PRESENT}" == "true" ]] \
      && path_matches_object "${DISPLACED_BIN_ENTRY}" "${PREVIOUS_BIN_OBJECT_IDENTITY}" "${PREVIOUS_BIN}" \
      && [[ ! -e "${BIN_LINK}" && ! -L "${BIN_LINK}" ]]; then
      if restore_previous_without_overwrite; then
        ROLLBACK_BINARY_STATUS="failed_and_previous_binary_restored"
        echo "restored Harness link to ${PREVIOUS_BIN}" >&2
        return
      fi
      ROLLBACK_BINARY_STATUS="failed_after_binary_publication_restore_failed"
      PRESERVE_BIN_LINK_TRANSACTION="true"
      echo "failed to restore Harness link ${BIN_LINK}; ownership evidence remains residual" >&2
      return 1
    elif [[ "${PREVIOUS_BIN_PRESENT}" != "true" && ! -e "${BIN_LINK}" && ! -L "${BIN_LINK}" ]]; then
      ROLLBACK_BINARY_STATUS="failed_before_binary_publication"
      return
    fi
    ROLLBACK_BINARY_STATUS="failed_after_binary_publication_link_changed"
    echo "preserved changed Harness path ${BIN_LINK}; it is no longer owned by this install" >&2
    return
  fi

  move_to_transaction_entry "${BIN_LINK}" "${ROLLBACK_BIN_ENTRY}" || true
  if path_matches_object "${ROLLBACK_BIN_ENTRY}" "${PUBLISHED_BIN_IDENTITY}" "${PUBLISHED_BIN_TARGET}"; then
    :
  elif [[ -e "${ROLLBACK_BIN_ENTRY}" || -L "${ROLLBACK_BIN_ENTRY}" ]]; then
    if ! restore_quarantined_entry "${ROLLBACK_BIN_ENTRY}"; then
      PRESERVE_BIN_LINK_TRANSACTION="true"
      echo "failed to restore concurrently changed Harness path; ownership evidence is preserved in ${BIN_LINK_LOCK_DIR}" >&2
      ROLLBACK_BINARY_STATUS="failed_after_binary_publication_remove_failed"
      return 1
    fi
    ROLLBACK_BINARY_STATUS="failed_after_binary_publication_link_changed"
    echo "restored concurrently changed Harness path ${BIN_LINK}" >&2
    return
  elif path_matches_object "${BIN_LINK}" "${PUBLISHED_BIN_IDENTITY}" "${PUBLISHED_BIN_TARGET}"; then
    ROLLBACK_BINARY_STATUS="failed_after_binary_publication_remove_failed"
    PRESERVE_BIN_LINK_TRANSACTION="true"
    echo "failed to quarantine published Harness link ${BIN_LINK}; ownership evidence remains residual" >&2
    return 1
  else
    ROLLBACK_BINARY_STATUS="failed_after_binary_publication_link_changed"
    echo "preserved concurrently changed Harness path ${BIN_LINK}" >&2
    return
  fi

  if [[ "${PREVIOUS_BIN_PRESENT}" == "true" ]]; then
    if ! restore_previous_without_overwrite; then
      ROLLBACK_BINARY_STATUS="failed_after_binary_publication_restore_failed"
      PRESERVE_BIN_LINK_TRANSACTION="true"
      echo "failed to restore Harness link ${BIN_LINK}; published link remains residual" >&2
      return 1
    fi
    ROLLBACK_BINARY_STATUS="failed_and_previous_binary_restored"
    echo "restored Harness link to ${PREVIOUS_BIN}" >&2
  else
    if [[ -e "${BIN_LINK}" || -L "${BIN_LINK}" ]]; then
      ROLLBACK_BINARY_STATUS="failed_after_binary_publication_link_changed"
      echo "preserved concurrently changed Harness path ${BIN_LINK}" >&2
    else
      ROLLBACK_BINARY_STATUS="failed_and_created_binary_link_removed"
      echo "removed incomplete Harness link ${BIN_LINK}" >&2
    fi
  fi
}

rollback_binary_after_error() {
  local exit_status=$1
  if [[ "${exit_status}" -eq 0 || "${APPLY_IN_PROGRESS}" != "true" ]]; then
    return 0
  fi
  rollback_published_bin_link || true
}

write_failure_state() {
  local original_exit_status=$1
  local final_exit_status=$2
  local failure_status="${ROLLBACK_BINARY_STATUS}"
  if [[ "${BIN_LINK_LOCK_STATUS}" == "release_failed" ]]; then
    failure_status="failed_with_install_lock_residual"
  fi
  if [[ -n "${STATE_FILE}" ]]; then
    node - "${STATE_FILE}" "${VERSION:-unknown}" "${REPO_ROOT}" "${PREVIOUS_BIN}" "${failure_status}" "${ROLLBACK_BINARY_STATUS}" "${BIN_LINK_LOCK_STATUS}" "${original_exit_status}" "${final_exit_status}" <<'NODE' || true
const fs = require("node:fs");
const [path, version, sourceRoot, rollbackBinary, status, binaryRollbackStatus, installLockStatus, originalExitStatus, finalExitStatus] = process.argv.slice(2);
fs.writeFileSync(path, `${JSON.stringify({
  schema_version: 1,
  status,
  binary_rollback_status: binaryRollbackStatus,
  install_lock_status: installLockStatus,
  original_exit_status: Number(originalExitStatus),
  final_exit_status: Number(finalExitStatus),
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
  local original_exit_status=$?
  local final_exit_status="${original_exit_status}"
  trap - EXIT
  trap '' HUP INT TERM
  if [[ "${INSTALL_COMPLETED}" == "true" || "${original_exit_status}" -eq 0 ]]; then
    ROLLBACK_BINARY_STATUS="not_attempted_install_completed"
  else
    rollback_binary_after_error "${original_exit_status}" || true
  fi
  if ! release_bin_link_lock && [[ "${final_exit_status}" -eq 0 ]]; then
    final_exit_status=1
  fi
  if [[ "${final_exit_status}" -ne 0 && ( "${INSTALL_COMPLETED}" != "true" || "${BIN_LINK_LOCK_STATUS}" == "release_failed" ) ]]; then
    write_failure_state "${original_exit_status}" "${final_exit_status}"
  fi
  exit "${final_exit_status}"
}

handle_install_signal() {
  trap '' HUP INT TERM
  exit "$1"
}

trap finish_install EXIT
trap 'handle_install_signal 129' HUP
trap 'handle_install_signal 130' INT
trap 'handle_install_signal 143' TERM

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

mkdir -p "${STATE_BASE}/installations"
INSTALLED_AT="$(date -u '+%Y-%m-%dT%H:%M:%SZ')"
STATE_FILE="${STATE_BASE}/installations/${INSTALLED_AT//:/-}-${VERSION}-$$.json"
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
mkdir -p "${VERSION_DIR}" "$(dirname "${BIN_LINK}")"
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
INSTALL_COMPLETED="true"
APPLY_IN_PROGRESS="false"
ROLLBACK_BINARY_STATUS="not_attempted_install_completed"

echo
echo "Installed Star Harness ${VERSION}."
echo "State: ${STATE_FILE}"
echo "Start a new Codex task, a new Claude session, and run /reload in Kimi Code to activate."
