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
INSTALL_BASE="${STAR_HARNESS_INSTALL_ROOT:-${USER_HOME_DIR}/.local/lib/star-harness}"
BIN_LINK="${STAR_HARNESS_BIN_LINK:-${USER_HOME_DIR}/.local/bin/harness}"
FIRM_LINK="${STAR_HARNESS_FIRM_LINK:-${USER_HOME_DIR}/.local/bin/firm}"
STATE_BASE="${STAR_HARNESS_STATE_ROOT:-${USER_HOME_DIR}/.local/state/star-harness}"
APPLY_IN_PROGRESS="false"
INSTALL_COMPLETED="false"
PREVIOUS_BIN=""
PREVIOUS_BIN_PRESENT="false"
PREVIOUS_BIN_IDENTITY=""
PREVIOUS_BIN_OBJECT_IDENTITY=""
PREVIOUS_FIRM=""
PREVIOUS_FIRM_PRESENT="false"
PREVIOUS_FIRM_IDENTITY=""
PREVIOUS_FIRM_OBJECT_IDENTITY=""
BIN_LINK_PUBLICATION_ARMED="false"
FIRM_LINK_PUBLICATION_ARMED="false"
PUBLICATION_OBSERVED_LIVE="false"
PUBLISHED_BIN_TARGET=""
PUBLISHED_BIN_IDENTITY=""
PUBLISHED_FIRM_TARGET=""
PUBLISHED_FIRM_IDENTITY=""
ROLLBACK_BINARY_STATUS="failed_before_binary_publication"
ROLLBACK_FIRM_STATUS="failed_before_binary_publication"
STATE_FILE=""
BIN_LINK_LOCK_DIR="${BIN_LINK}.star-harness-install.lock"
BIN_LINK_TRANSACTION_DIR=""
PREVIOUS_BIN_WITNESS=""
PUBLISHED_BIN_STAGED=""
PUBLISHED_BIN_WITNESS=""
DISPLACED_BIN_ENTRY=""
ROLLBACK_BIN_ENTRY=""
PREVIOUS_FIRM_WITNESS=""
PUBLISHED_FIRM_STAGED=""
PUBLISHED_FIRM_WITNESS=""
DISPLACED_FIRM_ENTRY=""
ROLLBACK_FIRM_ENTRY=""
INSTALL_FS_HELPER=""
INSTALL_FS_HELPER_REAL=""
INSTALL_FS_HELPER_SOURCE="${REPO_ROOT}/scripts/star-harness-install-fs.rs"
BIN_LINK_LOCK_STAGED=""
BIN_LINK_LOCK_RELEASE_ENTRY=""
BIN_LINK_STALE_LOCK_WITNESS=""
BIN_LINK_STALE_LOCK_ENTRY=""
BIN_LINK_LOCK_OWNED="false"
BIN_LINK_LOCK_STATUS="not_acquired"
PRESERVE_BIN_LINK_TRANSACTION="false"
LOCK_ACQUIRE_CRITICAL="false"
PENDING_INSTALL_SIGNAL=0

acquire_bin_link_lock() {
  local transaction_status=0
  local lock_status=0
  local owner_start_token
  owner_start_token="$(process_start_token "$$")"
  if [[ -z "${owner_start_token}" ]]; then
    echo "failed to resolve the Harness installer lock owner identity" >&2
    return 1
  fi
  initialize_bin_link_transaction_paths "$$" "${owner_start_token}"
  mkdir -p "$(dirname "${BIN_LINK}")"
  LOCK_ACQUIRE_CRITICAL="true"
  mkdir "${BIN_LINK_TRANSACTION_DIR}" 2>/dev/null || transaction_status=$?
  LOCK_ACQUIRE_CRITICAL="false"
  if [[ "${PENDING_INSTALL_SIGNAL}" -ne 0 ]]; then
    local pending_signal="${PENDING_INSTALL_SIGNAL}"
    PENDING_INSTALL_SIGNAL=0
    rmdir "${BIN_LINK_TRANSACTION_DIR}" 2>/dev/null || true
    exit "${pending_signal}"
  fi
  if [[ "${transaction_status}" -ne 0 || ! -d "${BIN_LINK_TRANSACTION_DIR}" ]]; then
    echo "failed to create Harness binary publication transaction directory: ${BIN_LINK_TRANSACTION_DIR}" >&2
    return 1
  fi

  if ! rustc --edition=2021 "${INSTALL_FS_HELPER_SOURCE}" -o "${INSTALL_FS_HELPER}"; then
    echo "failed to build the Harness installer filesystem helper" >&2
    return 1
  fi
  ln -s "${BIN_LINK_TRANSACTION_DIR}" "${BIN_LINK_LOCK_STAGED}"

  if [[ -e "${BIN_LINK_LOCK_DIR}" || -L "${BIN_LINK_LOCK_DIR}" ]]; then
    if ! reconcile_stale_bin_link_lock; then
      echo "Harness binary publication is already owned by another installer: ${BIN_LINK_LOCK_DIR}" >&2
      return 1
    fi
  fi

  LOCK_ACQUIRE_CRITICAL="true"
  link_without_replace "${BIN_LINK_LOCK_STAGED}" "${BIN_LINK_LOCK_DIR}" 2>/dev/null || lock_status=$?
  if path_matches_object \
    "${BIN_LINK_LOCK_DIR}" \
    "$(bin_link_object_identity "${BIN_LINK_LOCK_STAGED}")" \
    "${BIN_LINK_TRANSACTION_DIR}"; then
    BIN_LINK_LOCK_OWNED="true"
    BIN_LINK_LOCK_STATUS="held"
  fi
  LOCK_ACQUIRE_CRITICAL="false"
  if [[ "${PENDING_INSTALL_SIGNAL}" -ne 0 ]]; then
    local pending_signal="${PENDING_INSTALL_SIGNAL}"
    PENDING_INSTALL_SIGNAL=0
    exit "${pending_signal}"
  fi
  if [[ "${lock_status}" -ne 0 || "${BIN_LINK_LOCK_OWNED}" != "true" ]]; then
    echo "Harness binary publication is already owned by another installer: ${BIN_LINK_LOCK_DIR}" >&2
    return 1
  fi
}

release_bin_link_lock() {
  if [[ "${BIN_LINK_LOCK_OWNED}" == "true" ]]; then
    if [[ "${PRESERVE_BIN_LINK_TRANSACTION}" == "true" ]]; then
      BIN_LINK_LOCK_STATUS="rollback_residual_preserved"
      echo "preserved Harness binary publication lock and ownership witnesses after rollback failure: ${BIN_LINK_LOCK_DIR}" >&2
      return 1
    fi
    local release_status=0
    local lock_identity
    lock_identity="$(bin_link_object_identity "${BIN_LINK_LOCK_STAGED}" 2>/dev/null || true)"
    if [[ -z "${lock_identity}" ]] || ! path_matches_object \
      "${BIN_LINK_LOCK_DIR}" "${lock_identity}" "${BIN_LINK_TRANSACTION_DIR}"; then
      BIN_LINK_LOCK_STATUS="release_failed"
      PRESERVE_BIN_LINK_TRANSACTION="true"
      echo "refusing to release a changed Harness binary publication lock: ${BIN_LINK_LOCK_DIR}" >&2
      return 1
    fi
    move_without_replace "${BIN_LINK_LOCK_DIR}" "${BIN_LINK_LOCK_RELEASE_ENTRY}" || release_status=$?
    if path_matches_object \
      "${BIN_LINK_LOCK_RELEASE_ENTRY}" "${lock_identity}" "${BIN_LINK_TRANSACTION_DIR}"; then
      BIN_LINK_LOCK_OWNED="false"
    elif [[ -e "${BIN_LINK_LOCK_RELEASE_ENTRY}" || -L "${BIN_LINK_LOCK_RELEASE_ENTRY}" ]]; then
      if ! restore_quarantined_entry_to_path "${BIN_LINK_LOCK_RELEASE_ENTRY}" "${BIN_LINK_LOCK_DIR}"; then
        echo "failed to restore a concurrently changed Harness publication lock; evidence is preserved in ${BIN_LINK_TRANSACTION_DIR}" >&2
      fi
      BIN_LINK_LOCK_STATUS="release_failed"
      PRESERVE_BIN_LINK_TRANSACTION="true"
      return 1
    else
      BIN_LINK_LOCK_STATUS="release_failed"
      PRESERVE_BIN_LINK_TRANSACTION="true"
      echo "failed to atomically release Harness binary publication lock: ${BIN_LINK_LOCK_DIR}" >&2
      return 1
    fi
    local artifact
    for artifact in \
      "${PREVIOUS_BIN_WITNESS}" \
      "${PUBLISHED_BIN_STAGED}" \
      "${PUBLISHED_BIN_WITNESS}" \
      "${DISPLACED_BIN_ENTRY}" \
      "${ROLLBACK_BIN_ENTRY}" \
      "${PREVIOUS_FIRM_WITNESS}" \
      "${PUBLISHED_FIRM_STAGED}" \
      "${PUBLISHED_FIRM_WITNESS}" \
      "${DISPLACED_FIRM_ENTRY}" \
      "${ROLLBACK_FIRM_ENTRY}" \
      "${BIN_LINK_LOCK_STAGED}" \
      "${BIN_LINK_LOCK_RELEASE_ENTRY}" \
      "${BIN_LINK_STALE_LOCK_WITNESS}" \
      "${BIN_LINK_STALE_LOCK_ENTRY}" \
      "${INSTALL_FS_HELPER}" \
      "${INSTALL_FS_HELPER_REAL}"; do
      if [[ -e "${artifact}" || -L "${artifact}" ]]; then
        if ! unlink "${artifact}"; then
          BIN_LINK_LOCK_STATUS="release_failed"
          echo "failed to remove Harness binary publication artifact: ${artifact}" >&2
          return 1
        fi
      fi
    done
    if ! rmdir "${BIN_LINK_TRANSACTION_DIR}"; then
      BIN_LINK_LOCK_STATUS="release_failed"
      echo "failed to remove Harness binary publication transaction directory: ${BIN_LINK_TRANSACTION_DIR}" >&2
      return 1
    fi
    if [[ "${release_status}" -ne 0 ]]; then
      BIN_LINK_LOCK_STATUS="release_failed_no_residual"
      echo "Harness publication lock moved but the filesystem helper reported failure" >&2
      return "${release_status}"
    fi
    BIN_LINK_LOCK_STATUS="released"
  elif [[ "${PRESERVE_BIN_LINK_TRANSACTION}" != "true" && -d "${BIN_LINK_TRANSACTION_DIR}" ]]; then
    local artifact
    for artifact in \
      "${BIN_LINK_LOCK_STAGED}" \
      "${BIN_LINK_STALE_LOCK_WITNESS}" \
      "${BIN_LINK_STALE_LOCK_ENTRY}" \
      "${INSTALL_FS_HELPER}" \
      "${INSTALL_FS_HELPER_REAL}"; do
      if [[ -e "${artifact}" || -L "${artifact}" ]]; then
        unlink "${artifact}" 2>/dev/null || true
      fi
    done
    rmdir "${BIN_LINK_TRANSACTION_DIR}" 2>/dev/null || true
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

process_start_identity() {
  LC_ALL=C TZ=UTC0 ps -o lstart= -p "$1" 2>/dev/null
}

process_start_token() {
  local identity
  identity="$(process_start_identity "$1")"
  [[ -n "${identity}" ]] || return 1
  identity="${identity//[^[:alnum:]]/_}"
  printf '%s' "${identity}"
}

process_is_proven_absent() {
  node -e '
    try {
      process.kill(Number(process.argv[1]), 0);
      process.exit(1);
    } catch (error) {
      process.exit(error && error.code === "ESRCH" ? 0 : 1);
    }
  ' "$1"
}

initialize_bin_link_transaction_paths() {
  local owner_pid=$1
  local owner_start_token=$2
  BIN_LINK_TRANSACTION_DIR="${BIN_LINK_LOCK_DIR}.txn-${owner_pid}-${owner_start_token}-${RANDOM}-${RANDOM}"
  PREVIOUS_BIN_WITNESS="${BIN_LINK_TRANSACTION_DIR}/previous-link-witness"
  PUBLISHED_BIN_STAGED="${BIN_LINK_TRANSACTION_DIR}/published-link-staged"
  PUBLISHED_BIN_WITNESS="${BIN_LINK_TRANSACTION_DIR}/published-link-witness"
  DISPLACED_BIN_ENTRY="${BIN_LINK_TRANSACTION_DIR}/displaced-live-entry"
  ROLLBACK_BIN_ENTRY="${BIN_LINK_TRANSACTION_DIR}/rollback-live-entry"
  PREVIOUS_FIRM_WITNESS="${BIN_LINK_TRANSACTION_DIR}/previous-firm-link-witness"
  PUBLISHED_FIRM_STAGED="${BIN_LINK_TRANSACTION_DIR}/published-firm-link-staged"
  PUBLISHED_FIRM_WITNESS="${BIN_LINK_TRANSACTION_DIR}/published-firm-link-witness"
  DISPLACED_FIRM_ENTRY="${BIN_LINK_TRANSACTION_DIR}/displaced-firm-live-entry"
  ROLLBACK_FIRM_ENTRY="${BIN_LINK_TRANSACTION_DIR}/rollback-firm-live-entry"
  INSTALL_FS_HELPER="${BIN_LINK_TRANSACTION_DIR}/install-fs-helper"
  INSTALL_FS_HELPER_REAL="${INSTALL_FS_HELPER}.real"
  BIN_LINK_LOCK_STAGED="${BIN_LINK_TRANSACTION_DIR}/lock-staged"
  BIN_LINK_LOCK_RELEASE_ENTRY="${BIN_LINK_TRANSACTION_DIR}/lock-release-entry"
  BIN_LINK_STALE_LOCK_WITNESS="${BIN_LINK_TRANSACTION_DIR}/stale-lock-witness"
  BIN_LINK_STALE_LOCK_ENTRY="${BIN_LINK_TRANSACTION_DIR}/stale-lock-entry"
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
  "${INSTALL_FS_HELPER}" hard-link-no-replace "$1" "$2"
}

move_without_replace() {
  "${INSTALL_FS_HELPER}" move-no-replace "$1" "$2"
}

move_to_transaction_entry() {
  move_without_replace "$1" "$2"
}

restore_quarantined_entry_to_path() {
  local entry=$1
  local destination=$2
  local entry_identity=""
  local restore_status=0
  if [[ ! -e "${entry}" && ! -L "${entry}" ]]; then
    return 1
  fi
  entry_identity="$(bin_link_object_identity "${entry}" 2>/dev/null || true)"
  move_without_replace "${entry}" "${destination}" || restore_status=$?
  if [[ -n "${entry_identity}" && "$(bin_link_object_identity "${destination}" 2>/dev/null || true)" == "${entry_identity}" ]]; then
    return 0
  fi
  if [[ "${restore_status}" -ne 0 ]]; then
    return "${restore_status}"
  fi
  return 1
}

restore_quarantined_entry() {
  restore_quarantined_entry_to_path "$1" "${BIN_LINK}"
}

reconcile_stale_bin_link_lock() {
  local stale_target=""
  local stale_identity=""
  local move_status=0
  local owner_pid=""
  local owner_start_token=""
  local current_start_token=""
  local owner_metadata=""
  if ! link_without_replace "${BIN_LINK_LOCK_DIR}" "${BIN_LINK_STALE_LOCK_WITNESS}" 2>/dev/null; then
    return 1
  fi
  if [[ ! -L "${BIN_LINK_STALE_LOCK_WITNESS}" ]]; then
    unlink "${BIN_LINK_STALE_LOCK_WITNESS}" 2>/dev/null || true
    return 1
  fi
  stale_target="$(readlink "${BIN_LINK_STALE_LOCK_WITNESS}" 2>/dev/null || true)"
  case "${stale_target}" in
    "${BIN_LINK_LOCK_DIR}.txn-"*) ;;
    *)
      unlink "${BIN_LINK_STALE_LOCK_WITNESS}" 2>/dev/null || true
      return 1
      ;;
  esac
  owner_metadata="${stale_target#"${BIN_LINK_LOCK_DIR}.txn-"}"
  if [[ "${owner_metadata}" =~ ^([0-9]+)-([[:alnum:]_]+)-([0-9]+)-([0-9]+)$ ]]; then
    owner_pid="${BASH_REMATCH[1]}"
    owner_start_token="${BASH_REMATCH[2]}"
  else
    unlink "${BIN_LINK_STALE_LOCK_WITNESS}" 2>/dev/null || true
    return 1
  fi
  current_start_token="$(process_start_token "${owner_pid}" 2>/dev/null || true)"
  if [[ "${current_start_token}" == "${owner_start_token}" ]]; then
    unlink "${BIN_LINK_STALE_LOCK_WITNESS}" 2>/dev/null || true
    return 1
  fi
  if [[ -z "${current_start_token}" ]] && ! process_is_proven_absent "${owner_pid}"; then
    unlink "${BIN_LINK_STALE_LOCK_WITNESS}" 2>/dev/null || true
    return 1
  fi
  stale_identity="$(bin_link_object_identity "${BIN_LINK_STALE_LOCK_WITNESS}" 2>/dev/null || true)"
  move_without_replace "${BIN_LINK_LOCK_DIR}" "${BIN_LINK_STALE_LOCK_ENTRY}" || move_status=$?
  if [[ -n "${stale_identity}" \
    && "$(bin_link_object_identity "${BIN_LINK_STALE_LOCK_ENTRY}" 2>/dev/null || true)" == "${stale_identity}" \
    && "$(readlink "${BIN_LINK_STALE_LOCK_ENTRY}" 2>/dev/null || true)" == "${stale_target}" ]]; then
    if [[ "${move_status}" -ne 0 ]]; then
      PRESERVE_BIN_LINK_TRANSACTION="true"
      echo "stale Harness lock moved but the filesystem helper reported failure; evidence is preserved" >&2
      return "${move_status}"
    fi
    unlink "${BIN_LINK_STALE_LOCK_ENTRY}"
    unlink "${BIN_LINK_STALE_LOCK_WITNESS}"
    return 0
  fi
  if [[ -e "${BIN_LINK_STALE_LOCK_ENTRY}" || -L "${BIN_LINK_STALE_LOCK_ENTRY}" ]]; then
    if ! restore_quarantined_entry_to_path "${BIN_LINK_STALE_LOCK_ENTRY}" "${BIN_LINK_LOCK_DIR}"; then
      PRESERVE_BIN_LINK_TRANSACTION="true"
      echo "failed to restore a concurrently changed publication lock; evidence is preserved" >&2
    fi
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
    elif [[ ! -e "${BIN_LINK}" && ! -L "${BIN_LINK}" ]]; then
      if ! restore_previous_without_overwrite; then
        PRESERVE_BIN_LINK_TRANSACTION="true"
        echo "failed to restore the previous Harness link after its uncertain displacement; ownership evidence is preserved in ${BIN_LINK_LOCK_DIR}" >&2
      fi
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
    PUBLICATION_OBSERVED_LIVE="true"
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
    if [[ "${PUBLICATION_OBSERVED_LIVE}" == "true" ]]; then
      ROLLBACK_BINARY_STATUS="failed_after_binary_publication_link_changed"
    else
      ROLLBACK_BINARY_STATUS="failed_before_binary_publication_path_changed"
    fi
    echo "preserved changed Harness path ${BIN_LINK}; it is no longer owned by this install" >&2
    return
  fi

  PUBLICATION_OBSERVED_LIVE="true"
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
  elif [[ "${PREVIOUS_BIN_PRESENT}" == "true" && ! -e "${BIN_LINK}" && ! -L "${BIN_LINK}" ]]; then
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
    ROLLBACK_BINARY_STATUS="failed_and_created_binary_link_removed"
    echo "removed incomplete Harness link ${BIN_LINK}" >&2
    return
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
  rollback_published_firm_link || true
  rollback_published_bin_link || true
}

# The firm alias follows the exact same publication discipline as the primary
# harness link: same lock, same transaction, witness + staged links,
# refuse-on-change, and exact previous-object restore on rollback.

path_matches_previous_firm_snapshot() {
  local path=$1
  [[ "${PREVIOUS_FIRM_PRESENT}" == "true" && -L "${path}" ]] || return 1
  [[ "$(bin_link_identity "${path}" 2>/dev/null || true)" == "${PREVIOUS_FIRM_IDENTITY}" ]] || return 1
  [[ "$(readlink "${path}" 2>/dev/null || true)" == "${PREVIOUS_FIRM}" ]]
}

restore_previous_firm_without_overwrite() {
  local restore_status=0
  link_without_replace "${PREVIOUS_FIRM_WITNESS}" "${FIRM_LINK}" || restore_status=$?
  if path_matches_object "${FIRM_LINK}" "${PREVIOUS_FIRM_OBJECT_IDENTITY}" "${PREVIOUS_FIRM}"; then
    return 0
  fi
  if [[ "${restore_status}" -ne 0 ]]; then
    return "${restore_status}"
  fi
  return 1
}

restore_quarantined_firm_entry() {
  restore_quarantined_entry_to_path "$1" "${FIRM_LINK}"
}

prepare_firm_link_publication() {
  if [[ -e "${FIRM_LINK}" || -L "${FIRM_LINK}" ]]; then
    link_without_replace "${FIRM_LINK}" "${PREVIOUS_FIRM_WITNESS}"
    if [[ ! -L "${PREVIOUS_FIRM_WITNESS}" ]]; then
      echo "refusing to replace non-symlink ${FIRM_LINK}" >&2
      return 1
    fi
    PREVIOUS_FIRM="$(readlink "${PREVIOUS_FIRM_WITNESS}")"
    PREVIOUS_FIRM_IDENTITY="$(bin_link_identity "${PREVIOUS_FIRM_WITNESS}")"
    PREVIOUS_FIRM_OBJECT_IDENTITY="$(bin_link_object_identity "${PREVIOUS_FIRM_WITNESS}")"
    PREVIOUS_FIRM_PRESENT="true"
  else
    PREVIOUS_FIRM=""
    PREVIOUS_FIRM_IDENTITY=""
    PREVIOUS_FIRM_OBJECT_IDENTITY=""
    PREVIOUS_FIRM_PRESENT="false"
  fi
}

publish_firm_link() {
  local target=$1
  local displace_status=0
  local publish_status=0
  ln -s "${target}" "${PUBLISHED_FIRM_STAGED}"
  link_without_replace "${PUBLISHED_FIRM_STAGED}" "${PUBLISHED_FIRM_WITNESS}"
  PUBLISHED_FIRM_TARGET="${target}"
  PUBLISHED_FIRM_IDENTITY="$(bin_link_object_identity "${PUBLISHED_FIRM_WITNESS}")"
  FIRM_LINK_PUBLICATION_ARMED="true"

  if [[ "${PREVIOUS_FIRM_PRESENT}" == "true" ]]; then
    if ! path_matches_previous_firm_snapshot "${FIRM_LINK}"; then
      echo "refusing to publish over changed Firm alias link ${FIRM_LINK}" >&2
      return 1
    fi
    move_to_transaction_entry "${FIRM_LINK}" "${DISPLACED_FIRM_ENTRY}" || displace_status=$?
    if path_matches_object "${DISPLACED_FIRM_ENTRY}" "${PREVIOUS_FIRM_OBJECT_IDENTITY}" "${PREVIOUS_FIRM}"; then
      if [[ "${displace_status}" -ne 0 ]]; then
        if ! restore_previous_firm_without_overwrite; then
          PRESERVE_BIN_LINK_TRANSACTION="true"
          echo "failed to reconcile uncertain Firm alias link displacement; ownership evidence is preserved in ${BIN_LINK_LOCK_DIR}" >&2
        fi
        return "${displace_status}"
      fi
    elif [[ -e "${DISPLACED_FIRM_ENTRY}" || -L "${DISPLACED_FIRM_ENTRY}" ]]; then
      if ! restore_quarantined_firm_entry "${DISPLACED_FIRM_ENTRY}"; then
        PRESERVE_BIN_LINK_TRANSACTION="true"
        echo "failed to restore concurrently changed Firm alias path; ownership evidence is preserved in ${BIN_LINK_LOCK_DIR}" >&2
      fi
      echo "refusing to publish over concurrently changed Firm alias link ${FIRM_LINK}" >&2
      if [[ "${displace_status}" -ne 0 ]]; then
        return "${displace_status}"
      fi
      return 1
    elif path_matches_previous_firm_snapshot "${FIRM_LINK}"; then
      echo "failed to displace the previous Firm alias link" >&2
      if [[ "${displace_status}" -ne 0 ]]; then
        return "${displace_status}"
      fi
      return 1
    elif [[ ! -e "${FIRM_LINK}" && ! -L "${FIRM_LINK}" ]]; then
      if ! restore_previous_firm_without_overwrite; then
        PRESERVE_BIN_LINK_TRANSACTION="true"
        echo "failed to restore the previous Firm alias link after its uncertain displacement; ownership evidence is preserved in ${BIN_LINK_LOCK_DIR}" >&2
      fi
      if [[ "${displace_status}" -ne 0 ]]; then
        return "${displace_status}"
      fi
      return 1
    else
      echo "refusing to publish after the Firm alias path changed concurrently" >&2
      if [[ "${displace_status}" -ne 0 ]]; then
        return "${displace_status}"
      fi
      return 1
    fi
  fi

  link_without_replace "${PUBLISHED_FIRM_STAGED}" "${FIRM_LINK}" || publish_status=$?
  if path_matches_object "${FIRM_LINK}" "${PUBLISHED_FIRM_IDENTITY}" "${PUBLISHED_FIRM_TARGET}"; then
    if [[ "${publish_status}" -ne 0 ]]; then
      return "${publish_status}"
    fi
    return 0
  fi

  if [[ "${PREVIOUS_FIRM_PRESENT}" == "true" ]] && ! restore_previous_firm_without_overwrite; then
    PRESERVE_BIN_LINK_TRANSACTION="true"
    echo "failed to restore the previous Firm alias link after publication refusal; ownership evidence is preserved in ${BIN_LINK_LOCK_DIR}" >&2
  fi
  echo "refusing to publish over occupied Firm alias path ${FIRM_LINK}" >&2
  if [[ "${publish_status}" -ne 0 ]]; then
    return "${publish_status}"
  fi
  return 1
}

rollback_published_firm_link() {
  if [[ "${FIRM_LINK_PUBLICATION_ARMED}" != "true" ]]; then
    ROLLBACK_FIRM_STATUS="failed_before_binary_publication"
    return
  fi
  if ! path_matches_object "${FIRM_LINK}" "${PUBLISHED_FIRM_IDENTITY}" "${PUBLISHED_FIRM_TARGET}"; then
    if [[ "${PREVIOUS_FIRM_PRESENT}" == "true" ]] && path_matches_object "${FIRM_LINK}" "${PREVIOUS_FIRM_OBJECT_IDENTITY}" "${PREVIOUS_FIRM}"; then
      ROLLBACK_FIRM_STATUS="failed_before_binary_publication"
      return
    elif [[ "${PREVIOUS_FIRM_PRESENT}" == "true" ]] \
      && path_matches_object "${DISPLACED_FIRM_ENTRY}" "${PREVIOUS_FIRM_OBJECT_IDENTITY}" "${PREVIOUS_FIRM}" \
      && [[ ! -e "${FIRM_LINK}" && ! -L "${FIRM_LINK}" ]]; then
      if restore_previous_firm_without_overwrite; then
        ROLLBACK_FIRM_STATUS="failed_and_previous_binary_restored"
        echo "restored Firm alias link to ${PREVIOUS_FIRM}" >&2
        return
      fi
      ROLLBACK_FIRM_STATUS="failed_after_binary_publication_restore_failed"
      PRESERVE_BIN_LINK_TRANSACTION="true"
      echo "failed to restore Firm alias link ${FIRM_LINK}; ownership evidence remains residual" >&2
      return 1
    elif [[ "${PREVIOUS_FIRM_PRESENT}" != "true" && ! -e "${FIRM_LINK}" && ! -L "${FIRM_LINK}" ]]; then
      ROLLBACK_FIRM_STATUS="failed_before_binary_publication"
      return
    fi
    ROLLBACK_FIRM_STATUS="failed_after_binary_publication_link_changed"
    echo "preserved changed Firm alias path ${FIRM_LINK}; it is no longer owned by this install" >&2
    return
  fi

  move_to_transaction_entry "${FIRM_LINK}" "${ROLLBACK_FIRM_ENTRY}" || true
  if path_matches_object "${ROLLBACK_FIRM_ENTRY}" "${PUBLISHED_FIRM_IDENTITY}" "${PUBLISHED_FIRM_TARGET}"; then
    :
  elif [[ -e "${ROLLBACK_FIRM_ENTRY}" || -L "${ROLLBACK_FIRM_ENTRY}" ]]; then
    if ! restore_quarantined_firm_entry "${ROLLBACK_FIRM_ENTRY}"; then
      PRESERVE_BIN_LINK_TRANSACTION="true"
      echo "failed to restore concurrently changed Firm alias path; ownership evidence is preserved in ${BIN_LINK_LOCK_DIR}" >&2
      ROLLBACK_FIRM_STATUS="failed_after_binary_publication_remove_failed"
      return 1
    fi
    ROLLBACK_FIRM_STATUS="failed_after_binary_publication_link_changed"
    echo "restored concurrently changed Firm alias path ${FIRM_LINK}" >&2
    return
  elif path_matches_object "${FIRM_LINK}" "${PUBLISHED_FIRM_IDENTITY}" "${PUBLISHED_FIRM_TARGET}"; then
    ROLLBACK_FIRM_STATUS="failed_after_binary_publication_remove_failed"
    PRESERVE_BIN_LINK_TRANSACTION="true"
    echo "failed to quarantine published Firm alias link ${FIRM_LINK}; ownership evidence remains residual" >&2
    return 1
  elif [[ "${PREVIOUS_FIRM_PRESENT}" == "true" && ! -e "${FIRM_LINK}" && ! -L "${FIRM_LINK}" ]]; then
    if restore_previous_firm_without_overwrite; then
      ROLLBACK_FIRM_STATUS="failed_and_previous_binary_restored"
      echo "restored Firm alias link to ${PREVIOUS_FIRM}" >&2
      return
    fi
    ROLLBACK_FIRM_STATUS="failed_after_binary_publication_restore_failed"
    PRESERVE_BIN_LINK_TRANSACTION="true"
    echo "failed to restore Firm alias link ${FIRM_LINK}; ownership evidence remains residual" >&2
    return 1
  elif [[ "${PREVIOUS_FIRM_PRESENT}" != "true" && ! -e "${FIRM_LINK}" && ! -L "${FIRM_LINK}" ]]; then
    ROLLBACK_FIRM_STATUS="failed_and_created_binary_link_removed"
    echo "removed incomplete Firm alias link ${FIRM_LINK}" >&2
    return
  else
    ROLLBACK_FIRM_STATUS="failed_after_binary_publication_link_changed"
    echo "preserved concurrently changed Firm alias path ${FIRM_LINK}" >&2
    return
  fi

  if [[ "${PREVIOUS_FIRM_PRESENT}" == "true" ]]; then
    if ! restore_previous_firm_without_overwrite; then
      ROLLBACK_FIRM_STATUS="failed_after_binary_publication_restore_failed"
      PRESERVE_BIN_LINK_TRANSACTION="true"
      echo "failed to restore Firm alias link ${FIRM_LINK}; published link remains residual" >&2
      return 1
    fi
    ROLLBACK_FIRM_STATUS="failed_and_previous_binary_restored"
    echo "restored Firm alias link to ${PREVIOUS_FIRM}" >&2
  else
    if [[ -e "${FIRM_LINK}" || -L "${FIRM_LINK}" ]]; then
      ROLLBACK_FIRM_STATUS="failed_after_binary_publication_link_changed"
      echo "preserved concurrently changed Firm alias path ${FIRM_LINK}" >&2
    else
      ROLLBACK_FIRM_STATUS="failed_and_created_binary_link_removed"
      echo "removed incomplete Firm alias link ${FIRM_LINK}" >&2
    fi
  fi
}

write_failure_state() {
  local original_exit_status=$1
  local final_exit_status=$2
  local failure_status="${ROLLBACK_BINARY_STATUS}"
  if [[ "${BIN_LINK_LOCK_STATUS}" == "release_failed" ]]; then
    failure_status="failed_with_install_lock_residual"
  elif [[ "${BIN_LINK_LOCK_STATUS}" == "release_failed_no_residual" ]]; then
    failure_status="failed_install_lock_release"
  fi
  if [[ -n "${STATE_FILE}" ]]; then
    node - "${STATE_FILE}" "${VERSION:-unknown}" "${REPO_ROOT}" "${PREVIOUS_BIN}" "${failure_status}" "${ROLLBACK_BINARY_STATUS}" "${ROLLBACK_FIRM_STATUS}" "${BIN_LINK_LOCK_STATUS}" "${original_exit_status}" "${final_exit_status}" <<'NODE' || true
const fs = require("node:fs");
const [path, version, sourceRoot, rollbackBinary, status, binaryRollbackStatus, firmRollbackStatus, installLockStatus, originalExitStatus, finalExitStatus] = process.argv.slice(2);
fs.writeFileSync(path, `${JSON.stringify({
  schema_version: 1,
  status,
  binary_rollback_status: binaryRollbackStatus,
  firm_rollback_status: firmRollbackStatus,
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
  if [[ "${final_exit_status}" -ne 0 \
    && ( "${INSTALL_COMPLETED}" != "true" || "${BIN_LINK_LOCK_STATUS}" == release_failed* ) ]]; then
    write_failure_state "${original_exit_status}" "${final_exit_status}"
  fi
  exit "${final_exit_status}"
}

handle_install_signal() {
  if [[ "${LOCK_ACQUIRE_CRITICAL}" == "true" ]]; then
    if [[ "${PENDING_INSTALL_SIGNAL}" -eq 0 ]]; then
      PENDING_INSTALL_SIGNAL=$1
    fi
    return 0
  fi
  trap '' HUP INT TERM
  exit "$1"
}

trap finish_install EXIT
trap 'handle_install_signal 129' HUP
trap 'handle_install_signal 130' INT
trap 'handle_install_signal 143' TERM

if [[ "${MODE}" == "apply" ]]; then
  mkdir -p "${STATE_BASE}/installations"
  INSTALLED_AT="$(date -u '+%Y-%m-%dT%H:%M:%SZ')"
  STATE_FILE="${STATE_BASE}/installations/${INSTALLED_AT//:/-}-unknown-$$.json"
fi

for command_name in node npm cargo rustc git; do
  if ! command -v "${command_name}" >/dev/null 2>&1; then
    echo "missing required command: ${command_name}" >&2
    exit 1
  fi
done

# The installation version is the firm-cli crate version plus the exact source
# revision (ADR 0063 retired the plugin manifest that used to carry it), so a
# new revision publishes into its own ${INSTALL_BASE}/<version> directory
# instead of overwriting the binary an active MemberRun loaded. Re-applying the
# same revision (or a dirty tree, suffixed .dirty) republishes that directory;
# outside a git checkout only the crate version is available.
CRATE_VERSION="$(awk -F'"' '/^version = "/ { print $2; exit }' "${REPO_ROOT}/crates/firm-cli/Cargo.toml")"
if [[ -z "${CRATE_VERSION}" ]]; then
  echo "could not read the firm-cli crate version from ${REPO_ROOT}/crates/firm-cli/Cargo.toml" >&2
  exit 1
fi
SOURCE_REVISION="$(git -C "${REPO_ROOT}" rev-parse --short=12 HEAD 2>/dev/null || true)"
if [[ -n "${SOURCE_REVISION}" ]]; then
  VERSION="${CRATE_VERSION}+g${SOURCE_REVISION}"
  if ! git -C "${REPO_ROOT}" diff --quiet HEAD -- 2>/dev/null; then
    VERSION="${VERSION}.dirty"
  fi
else
  VERSION="${CRATE_VERSION}"
fi

echo "Star Harness source: ${VERSION} (${REPO_ROOT})"
(
  cd "${REPO_ROOT}"
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
echo "Firm alias link (same versioned binary as harness):"
HARNESS_LINK_TARGET=""
if [[ -L "${BIN_LINK}" ]]; then
  HARNESS_LINK_TARGET="$(readlink "${BIN_LINK}")"
fi
if [[ -L "${FIRM_LINK}" ]]; then
  ls -l "${FIRM_LINK}"
  FIRM_LINK_TARGET="$(readlink "${FIRM_LINK}")"
  if [[ -n "${HARNESS_LINK_TARGET}" && "${FIRM_LINK_TARGET}" == "${HARNESS_LINK_TARGET}" ]]; then
    echo "ok: harness and firm links both resolve to ${FIRM_LINK_TARGET}"
  else
    echo "warning: firm resolves to ${FIRM_LINK_TARGET} but harness resolves to ${HARNESS_LINK_TARGET:-not installed}; run --apply to converge both links"
  fi
elif [[ -e "${FIRM_LINK}" ]]; then
  echo "warning: ${FIRM_LINK} exists and is not a symlink; leaving it untouched"
else
  echo "not installed at ${FIRM_LINK} (run --apply to publish both links)"
fi

echo
echo "Candidate Firm binary:"
CANDIDATE_BIN="${REPO_ROOT}/target/debug/firm"
if [[ -x "${CANDIDATE_BIN}" ]]; then
  "${CANDIDATE_BIN}" --build-info
else
  echo "not built at ${CANDIDATE_BIN}; run cargo build -p firm-cli"
fi

if [[ "${MODE}" == "check" ]]; then
  echo
  echo "Check-only mode; run with --apply to publish this accepted source locally."
  exit 0
fi

acquire_bin_link_lock

echo
echo "Building Harness..."
(
  cd "${REPO_ROOT}"
  cargo build -p firm-cli
)
prepare_bin_link_publication
prepare_firm_link_publication

VERSION_DIR="${INSTALL_BASE}/${VERSION}"
VERSION_BIN="${VERSION_DIR}/harness"
CLAUDE_RUNNER_INSTALL="${VERSION_DIR}/apps/claude-member-runner"
DEEPSEEK_RUNNER_INSTALL="${VERSION_DIR}/apps/deepseek-member-runner"
mkdir -p "${VERSION_DIR}" "$(dirname "${BIN_LINK}")" "$(dirname "${FIRM_LINK}")"
APPLY_IN_PROGRESS="true"
install -m 0755 "${REPO_ROOT}/target/debug/firm" "${VERSION_BIN}"

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
publish_firm_link "${VERSION_BIN}"

node - "${STATE_FILE}" "${VERSION}" "${REPO_ROOT}" "${VERSION_BIN}" "${PREVIOUS_BIN}" "${INSTALLED_AT}" "${FIRM_LINK}" <<'NODE'
const fs = require("node:fs");
const [path, version, sourceRoot, binary, previousBinary, installedAt, firmLink] = process.argv.slice(2);
fs.writeFileSync(path, `${JSON.stringify({
  schema_version: 2,
  version,
  source_root: sourceRoot,
  harness_binary: binary,
  firm_binary_link: firmLink,
  rollback_harness_binary: previousBinary || null,
  installed_at: installedAt,
}, null, 2)}\n`);
NODE
INSTALL_COMPLETED="true"
APPLY_IN_PROGRESS="false"
ROLLBACK_BINARY_STATUS="not_attempted_install_completed"

echo
echo "Installed Star Harness ${VERSION}."
echo "Binary links: ${BIN_LINK} (primary) and ${FIRM_LINK} (alias) -> ${VERSION_BIN}"
echo "State: ${STATE_FILE}"
echo "Existing member sessions keep the binary they loaded; start new sessions to use this one."
