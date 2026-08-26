#!/usr/bin/env node
import assert from "node:assert/strict";
import { spawn, spawnSync } from "node:child_process";
import {
  chmodSync,
  existsSync,
  lstatSync,
  mkdirSync,
  mkdtempSync,
  readdirSync,
  readFileSync,
  readlinkSync,
  rmSync,
  symlinkSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const sourceRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const installerSource = readFileSync(
  join(sourceRoot, "scripts/manage-star-harness-install.sh"),
  "utf8",
);
const installerFsSource = readFileSync(
  join(sourceRoot, "scripts/star-harness-install-fs.rs"),
  "utf8",
);
const rustcSysroot = spawnSync("rustc", ["--print", "sysroot"], { encoding: "utf8" }).stdout.trim();
const rustcPath = join(rustcSysroot, "bin", "rustc");
assert.equal(existsSync(rustcPath), true, "installer fixture requires rustc on PATH");
const helperBuildRoot = mkdtempSync(join(tmpdir(), "star-harness-install-fs-helper-"));
const compiledInstallerFs = join(helperBuildRoot, "install-fs-helper");
const helperCompile = spawnSync(
  rustcPath,
  ["--edition=2021", join(sourceRoot, "scripts/star-harness-install-fs.rs"), "-o", compiledInstallerFs],
  { encoding: "utf8" },
);
assert.equal(helperCompile.status, 0, helperCompile.stderr);
process.on("exit", () => rmSync(helperBuildRoot, { recursive: true, force: true }));

function write(path, content, executable = false) {
  mkdirSync(dirname(path), { recursive: true });
  writeFileSync(path, content);
  if (executable) chmodSync(path, 0o755);
}

function createFixture() {
  const root = mkdtempSync(join(tmpdir(), "star-harness-install-rollback-"));
  const repo = join(root, "repo");
  const fakeBin = join(root, "fake-bin");
  const home = join(root, "home");
  const binLink = join(root, "published", "harness");
  mkdirSync(fakeBin, { recursive: true });
  mkdirSync(home, { recursive: true });

  write(join(repo, "scripts/manage-star-harness-install.sh"), installerSource, true);
  write(join(repo, "scripts/star-harness-install-fs.rs"), installerFsSource);
  for (const script of [
    "sync-star-harness-plugin-skills.mjs",
    "check-star-harness-plugin.mjs",
    "check-star-harness-hook.mjs",
    "check-cross-layer-consistency.mjs",
  ]) {
    write(join(repo, "scripts", script), "// isolated installer fixture\n");
  }
  write(
    join(repo, "plugins/star-harness/.codex-plugin/plugin.json"),
    `${JSON.stringify({ version: "fixture" })}\n`,
  );
  write(join(repo, "plugins/star-harness/kimi.plugin.json"), "{}\n");
  write(join(repo, "plugins/star-harness/scripts/star-harness-hook.sh"), "#!/bin/sh\n");
  mkdirSync(join(repo, "plugins/star-harness/skills"), { recursive: true });
  mkdirSync(join(repo, "plugins/star-harness/commands"), { recursive: true });
  write(join(repo, ".claude-plugin/marketplace.json"), "{}\n");
  mkdirSync(join(repo, "apps/claude-member-runner"), { recursive: true });
  mkdirSync(join(repo, "apps/deepseek-member-runner"), { recursive: true });
  write(
    join(repo, "target/debug/firm"),
    "#!/bin/sh\ncase \"$*\" in *--build-info*) echo '{\"git_rev\":\"fixture\"}' ;; *'member providers --json'*) echo '[]' ;; esac\n",
    true,
  );

  write(join(fakeBin, "cargo"), "#!/bin/sh\nexit 0\n", true);
  write(
    join(fakeBin, "rustc"),
    [
      "#!/bin/sh",
      "output=",
      "previous=",
      'for arg in "$@"; do',
      '  if [ "$previous" = "-o" ]; then output=$arg; fi',
      "  previous=$arg",
      "done",
      `cp "${compiledInstallerFs}" "$output" || exit $?`,
      'mv "$output" "$output.real"',
      'cat >"$output" <<\'SH\'',
      "#!/bin/sh",
      "operation=$1",
      "source_path=$2",
      "destination=$3",
      'if [ "$operation" = "hard-link-no-replace" ] && [ "$destination" = "${STAR_HARNESS_BIN_LINK}.star-harness-install.lock" ]; then',
      '  if [ -n "${FAKE_SIGNAL_AFTER_LOCK_EFFECT:-}" ] || [ "${FAKE_BLOCK_LOCK_ON_ACQUIRE:-0}" = 1 ]; then',
      '    "$0.real" "$@" || exit $?',
      '    if [ "${FAKE_BLOCK_LOCK_ON_ACQUIRE:-0}" = 1 ]; then touch "${STAR_HARNESS_BIN_LINK}.star-harness-install.lock/residual"; fi',
      '    if [ -n "${FAKE_SIGNAL_AFTER_LOCK_EFFECT:-}" ]; then kill -"${FAKE_SIGNAL_AFTER_LOCK_EFFECT}" "$PPID"; case "${FAKE_SIGNAL_AFTER_LOCK_EFFECT}" in HUP) exit 129 ;; INT) exit 130 ;; TERM) exit 143 ;; esac; fi',
      "  fi",
      "fi",
      'if [ "$operation" = "move-no-replace" ]; then',
      '  case "$destination" in',
      "    *displaced-live-entry)",
      '      if [ -n "${FAKE_FOREIGN_BEFORE_DISPLACE:-}" ]; then /bin/ln -sfn "${FAKE_FOREIGN_BEFORE_DISPLACE}" "${STAR_HARNESS_BIN_LINK}"; fi',
      '      if [ -n "${FAKE_DIRECTORY_BEFORE_DISPLACE:-}" ]; then /bin/rm -f "${STAR_HARNESS_BIN_LINK}"; /bin/mkdir "${STAR_HARNESS_BIN_LINK}"; fi',
      '      if [ "${FAKE_DELETE_BEFORE_DISPLACE:-0}" = 1 ]; then /bin/rm -f "${STAR_HARNESS_BIN_LINK}"; exit 46; fi',
      "      ;;",
      "    *rollback-live-entry)",
      '      if [ "${FAKE_UNLINK_HOLD_PATH:-}" = "${STAR_HARNESS_BIN_LINK}" ]; then touch "${FAKE_UNLINK_HOLD_READY}"; while [ ! -e "${FAKE_UNLINK_HOLD_RELEASE}" ]; do sleep 0.01; done; fi',
      '      if [ "${FAKE_UNLINK_FAIL_PATH:-}" = "${STAR_HARNESS_BIN_LINK}" ]; then exit 43; fi',
      '      if [ -n "${FAKE_FOREIGN_BEFORE_ROLLBACK:-}" ]; then /bin/ln -sfn "${FAKE_FOREIGN_BEFORE_ROLLBACK}" "${STAR_HARNESS_BIN_LINK}"; fi',
      '      if [ -n "${FAKE_DIRECTORY_BEFORE_ROLLBACK:-}" ]; then /bin/rm -f "${STAR_HARNESS_BIN_LINK}"; /bin/mkdir "${STAR_HARNESS_BIN_LINK}"; fi',
      '      if [ "${FAKE_DELETE_BEFORE_ROLLBACK:-0}" = 1 ]; then /bin/rm -f "${STAR_HARNESS_BIN_LINK}"; exit 43; fi',
      "      ;;",
      "    *lock-release-entry)",
      '      if [ -n "${FAKE_LOCK_REPLACEMENT_KIND:-}" ]; then',
      '        /bin/rm -f "${STAR_HARNESS_BIN_LINK}.star-harness-install.lock"',
      '        case "${FAKE_LOCK_REPLACEMENT_KIND}" in regular) echo foreign >"${STAR_HARNESS_BIN_LINK}.star-harness-install.lock" ;; symlink) /bin/ln -s foreign-lock "${STAR_HARNESS_BIN_LINK}.star-harness-install.lock" ;; directory) /bin/mkdir "${STAR_HARNESS_BIN_LINK}.star-harness-install.lock" ;; esac',
      "      fi",
      '      if [ "${FAKE_FAIL_LOCK_RELEASE_AFTER_EFFECT:-0}" = 1 ]; then "$0.real" "$@" || exit $?; exit 48; fi',
      '      if [ -n "${FAKE_HOLD_AFTER_PUBLIC_LOCK_MOVE_READY:-}" ]; then "$0.real" "$@" || exit $?; touch "${FAKE_HOLD_AFTER_PUBLIC_LOCK_MOVE_READY}"; while [ ! -e "${FAKE_HOLD_AFTER_PUBLIC_LOCK_MOVE_RELEASE}" ]; do sleep 0.01; done; exit 0; fi',
      "      ;;",
      "  esac",
      "fi",
      'if [ "$destination" = "${STAR_HARNESS_BIN_LINK}" ]; then',
      '  case "$source_path" in',
      "    *published-link-staged)",
      '      if [ -n "${FAKE_FOREIGN_BEFORE_PUBLISH:-}" ]; then /bin/ln -sfn "${FAKE_FOREIGN_BEFORE_PUBLISH}" "${STAR_HARNESS_BIN_LINK}"; fi',
      '      if [ -n "${FAKE_DIRECTORY_BEFORE_PUBLISH:-}" ]; then /bin/rm -f "${STAR_HARNESS_BIN_LINK}"; /bin/mkdir "${STAR_HARNESS_BIN_LINK}"; fi',
      '      if [ -n "${FAKE_DIRECTORY_SYMLINK_BEFORE_PUBLISH:-}" ]; then /bin/rm -f "${STAR_HARNESS_BIN_LINK}"; /bin/ln -s "${FAKE_DIRECTORY_SYMLINK_BEFORE_PUBLISH}" "${STAR_HARNESS_BIN_LINK}"; fi',
      '      if [ "${FAKE_HELPER_FAIL_PUBLISH:-0}" = 1 ]; then exit 45; fi',
      '      if [ "${FAKE_HELPER_FAIL_PUBLISH_AFTER_EFFECT:-0}" = 1 ]; then "$0.real" "$@" || exit $?; exit 45; fi',
      "      ;;",
      "    *previous-link-witness)",
      '      if [ "${FAKE_HELPER_FAIL_RESTORE:-0}" = 1 ]; then exit 44; fi',
      '      if [ "${FAKE_HELPER_FAIL_RESTORE_AFTER_EFFECT:-0}" = 1 ]; then "$0.real" "$@" || exit $?; exit 44; fi',
      "      ;;",
      "  esac",
      "fi",
      'exec "$0.real" "$@"',
      "SH",
      'chmod 755 "$output"',
      "",
    ].join("\n"),
    true,
  );
  write(
    join(fakeBin, "node"),
    [
      "#!/bin/sh",
      'case "$*" in',
      "  *symlinkSync*.star-harness-install.lock*)",
      '    if [ -n "${FAKE_SIGNAL_AFTER_LOCK_EFFECT:-}" ] || [ "${FAKE_BLOCK_LOCK_ON_ACQUIRE:-0}" = 1 ]; then',
      `      "${process.execPath}" "$@" || exit $?`,
      '      if [ "${FAKE_BLOCK_LOCK_ON_ACQUIRE:-0}" = 1 ]; then touch "${STAR_HARNESS_BIN_LINK}.star-harness-install.lock/residual"; fi',
      '      if [ -n "${FAKE_SIGNAL_AFTER_LOCK_EFFECT:-}" ]; then kill -"${FAKE_SIGNAL_AFTER_LOCK_EFFECT}" "$PPID"; case "${FAKE_SIGNAL_AFTER_LOCK_EFFECT}" in HUP) exit 129 ;; INT) exit 130 ;; TERM) exit 143 ;; esac; fi',
      "    fi",
      "    ;;",
      "  *renameSync*displaced-live-entry*)",
      '    if [ -n "${FAKE_FOREIGN_BEFORE_DISPLACE:-}" ]; then /bin/ln -sfn "${FAKE_FOREIGN_BEFORE_DISPLACE}" "${STAR_HARNESS_BIN_LINK}"; fi',
      '    if [ "${FAKE_DELETE_BEFORE_DISPLACE:-0}" = 1 ]; then /bin/rm -f "${STAR_HARNESS_BIN_LINK}"; exit 46; fi',
      "    ;;",
      "  *renameSync*rollback-live-entry*)",
      '    if [ "${FAKE_UNLINK_HOLD_PATH:-}" = "${STAR_HARNESS_BIN_LINK}" ]; then touch "${FAKE_UNLINK_HOLD_READY}"; while [ ! -e "${FAKE_UNLINK_HOLD_RELEASE}" ]; do sleep 0.01; done; fi',
      '    if [ "${FAKE_UNLINK_FAIL_PATH:-}" = "${STAR_HARNESS_BIN_LINK}" ]; then exit 43; fi',
      '    if [ -n "${FAKE_FOREIGN_BEFORE_ROLLBACK:-}" ]; then /bin/ln -sfn "${FAKE_FOREIGN_BEFORE_ROLLBACK}" "${STAR_HARNESS_BIN_LINK}"; fi',
      '    if [ "${FAKE_DELETE_BEFORE_ROLLBACK:-0}" = 1 ]; then /bin/rm -f "${STAR_HARNESS_BIN_LINK}"; exit 43; fi',
      "    ;;",
      "esac",
      `exec "${process.execPath}" "$@"`,
      "",
    ].join("\n"),
    true,
  );
  write(
    join(fakeBin, "mkdir"),
    [
      "#!/bin/sh",
      '/bin/mkdir "$@" || exit $?',
      "last=",
      'for arg in "$@"; do last=$arg; done',
      'case "$last" in',
      "  *.star-harness-install.lock.txn-*)",
      '    if [ -n "${FAKE_SIGNAL_AFTER_TRANSACTION_MKDIR:-}" ]; then kill -"${FAKE_SIGNAL_AFTER_TRANSACTION_MKDIR}" "$PPID"; case "${FAKE_SIGNAL_AFTER_TRANSACTION_MKDIR}" in HUP) exit 129 ;; INT) exit 130 ;; TERM) exit 143 ;; esac; fi',
      "    ;;",
      "  *.star-harness-install.lock)",
      '    if [ "${FAKE_BLOCK_LOCK_ON_ACQUIRE:-0}" = 1 ]; then touch "$last/residual"; fi',
      "    ;;",
      "esac",
      "",
    ].join("\n"),
    true,
  );
  write(
    join(fakeBin, "npm"),
    "#!/bin/sh\nif [ \"${FAKE_FAIL_BEFORE_PUBLICATION:-0}\" = 1 ]; then exit 41; fi\nexit 0\n",
    true,
  );
  write(
    join(fakeBin, "codex"),
    [
      "#!/bin/sh",
      'if [ "$1 $2" = "plugin list" ] && [ "${FAKE_CODEX_LIST_FAIL:-0}" = 1 ]; then exit 47; fi',
      'if [ "$1 $2" = "plugin add" ]; then',
      '  if [ "${FAKE_BLOCK_LOCK_RELEASE:-0}" = 1 ]; then touch "${STAR_HARNESS_BIN_LINK}.star-harness-install.lock/residual"; fi',
      '  if [ -n "${FAKE_HOLD_READY:-}" ]; then touch "${FAKE_HOLD_READY}"; while [ ! -e "${FAKE_HOLD_RELEASE}" ]; do sleep 0.01; done; fi',
      '  if [ "${FAKE_FAIL_AFTER_PUBLICATION:-0}" = 1 ]; then',
      '    if [ -n "${FAKE_FOREIGN_TARGET:-}" ]; then ln -sfn "${FAKE_FOREIGN_TARGET}" "${STAR_HARNESS_BIN_LINK}"; fi',
      '    if [ "${FAKE_RECREATE_SAME_TARGET:-0}" = 1 ]; then target=$(readlink "${STAR_HARNESS_BIN_LINK}"); unlink "${STAR_HARNESS_BIN_LINK}"; ln -s "$target" "${STAR_HARNESS_BIN_LINK}"; fi',
      "    exit 42",
      "  fi",
      "fi",
      "exit 0",
      "",
    ].join("\n"),
    true,
  );
  write(
    join(fakeBin, "claude"),
    "#!/bin/sh\nif [ \"$1 $2\" = 'plugin install' ] && [ \"${FAKE_CLAUDE_FAIL_AFTER_PUBLICATION:-0}\" = 1 ]; then exit 42; fi\nexit 0\n",
    true,
  );
  write(
    join(fakeBin, "unlink"),
    "#!/bin/sh\nif [ \"${FAKE_UNLINK_HOLD_PATH:-}\" = \"$1\" ]; then touch \"${FAKE_UNLINK_HOLD_READY}\"; while [ ! -e \"${FAKE_UNLINK_HOLD_RELEASE}\" ]; do sleep 0.01; done; fi\nif [ \"${FAKE_UNLINK_FAIL_PATH:-}\" = \"$1\" ]; then exit 43; fi\nexec /bin/rm -f -- \"$1\"\n",
    true,
  );

  return { root, repo, fakeBin, home, binLink };
}

function runApply(fixture, extraEnv = {}) {
  return spawnSync("bash", [join(fixture.repo, "scripts/manage-star-harness-install.sh"), "--apply"], {
    cwd: fixture.repo,
    encoding: "utf8",
    env: applyEnvironment(fixture, extraEnv),
  });
}

function applyEnvironment(fixture, extraEnv = {}) {
  const inheritedEnvironment = { ...process.env };
  for (const key of Object.keys(inheritedEnvironment)) {
    if (key.startsWith("FAKE_")) delete inheritedEnvironment[key];
  }
  return {
    ...inheritedEnvironment,
    ...extraEnv,
    HOME: fixture.home,
    PATH: extraEnv.PATH ?? `${fixture.fakeBin}:${process.env.PATH}`,
    STAR_HARNESS_BIN_LINK: fixture.binLink,
    STAR_HARNESS_INSTALL_ROOT: join(fixture.root, "install"),
    STAR_HARNESS_STATE_ROOT: join(fixture.root, "state"),
    KIMI_CODE_HOME: join(fixture.root, "kimi"),
  };
}

function waitForPath(path, child) {
  const sleeper = new Int32Array(new SharedArrayBuffer(4));
  const deadline = Date.now() + 15_000;
  while (!existsSync(path) && Date.now() < deadline) {
    assert.equal(child.exitCode, null, "installer exited before reaching the publication hold");
    Atomics.wait(sleeper, 0, 0, 10);
  }
  assert.equal(existsSync(path), true, `timed out waiting for ${path}`);
}

function withFixture(test) {
  const fixture = createFixture();
  try {
    test(fixture);
  } finally {
    rmSync(fixture.root, { recursive: true, force: true });
  }
}

async function withFixtureAsync(test) {
  const fixture = createFixture();
  try {
    await test(fixture);
  } finally {
    rmSync(fixture.root, { recursive: true, force: true });
  }
}

function readFailureState(fixture) {
  const stateDir = join(fixture.root, "state", "installations");
  const files = readdirSync(stateDir);
  assert.equal(files.length, 1, "failed apply must write one installation state");
  return JSON.parse(readFileSync(join(stateDir, files[0]), "utf8"));
}

function transactionDirectories(fixture) {
  return readdirSync(dirname(fixture.binLink))
    .filter((name) => name.startsWith("harness.star-harness-install.lock.txn-"))
    .map((name) => join(dirname(fixture.binLink), name))
    .filter((path) => lstatSync(path).isDirectory());
}

async function terminateAfterPublication(fixture) {
  const ready = join(fixture.root, "signal-ready");
  const release = join(fixture.root, "signal-release");
  const child = spawn(
    "bash",
    [join(fixture.repo, "scripts/manage-star-harness-install.sh"), "--apply"],
    {
      cwd: fixture.repo,
      detached: true,
      env: applyEnvironment(fixture, {
        FAKE_HOLD_READY: ready,
        FAKE_HOLD_RELEASE: release,
      }),
      stdio: "ignore",
    },
  );
  waitForPath(ready, child);
  const exit = new Promise((resolvePromise) => {
    child.once("exit", (code, signal) => resolvePromise({ code, signal }));
  });
  process.kill(-child.pid, "SIGTERM");
  return exit;
}

async function terminateAgainDuringCleanup(fixture) {
  const publicationReady = join(fixture.root, "signal-publication-ready");
  const publicationRelease = join(fixture.root, "signal-publication-release");
  const cleanupReady = join(fixture.root, "signal-cleanup-ready");
  const cleanupRelease = join(fixture.root, "signal-cleanup-release");
  const child = spawn(
    "bash",
    [join(fixture.repo, "scripts/manage-star-harness-install.sh"), "--apply"],
    {
      cwd: fixture.repo,
      detached: true,
      env: applyEnvironment(fixture, {
        FAKE_HOLD_READY: publicationReady,
        FAKE_HOLD_RELEASE: publicationRelease,
        FAKE_UNLINK_HOLD_PATH: fixture.binLink,
        FAKE_UNLINK_HOLD_READY: cleanupReady,
        FAKE_UNLINK_HOLD_RELEASE: cleanupRelease,
      }),
      stdio: "ignore",
    },
  );
  waitForPath(publicationReady, child);
  const exit = new Promise((resolvePromise) => {
    child.once("exit", (code, signal) => resolvePromise({ code, signal }));
  });
  process.kill(-child.pid, "SIGTERM");
  waitForPath(cleanupReady, child);
  process.kill(-child.pid, "SIGTERM");
  write(cleanupRelease, "continue\n");
  return exit;
}

withFixture((fixture) => {
  const original = "#!/bin/sh\necho pre-existing harness\n";
  write(fixture.binLink, original, true);
  const result = runApply(fixture);
  assert.notEqual(result.status, 0, "a pre-existing regular binary must be refused");
  assert.equal(readFileSync(fixture.binLink, "utf8"), original);
  assert.equal(lstatSync(fixture.binLink).isSymbolicLink(), false);
});

withFixture((fixture) => {
  rmSync(join(fixture.fakeBin, "claude"));
  const result = runApply(fixture, { PATH: `${fixture.fakeBin}:/bin:/usr/bin` });
  assert.equal(result.status, 1);
  assert.match(result.stderr, /missing required command: claude/);
  const state = readFailureState(fixture);
  assert.equal(state.status, "failed_before_binary_publication");
  assert.equal(state.install_lock_status, "not_acquired");
  assert.equal(state.version, "unknown");
});

withFixture((fixture) => {
  const result = runApply(fixture, { FAKE_CODEX_LIST_FAIL: "1" });
  assert.equal(result.status, 47, "apply preflight failures retain their native exit code");
  const state = readFailureState(fixture);
  assert.equal(state.status, "failed_before_binary_publication");
  assert.equal(state.install_lock_status, "not_acquired");
  assert.equal(state.original_exit_status, 47);
  assert.equal(state.version, "fixture");
});

withFixture((fixture) => {
  const lockPath = `${fixture.binLink}.star-harness-install.lock`;
  mkdirSync(dirname(lockPath), { recursive: true });
  symlinkSync(`${lockPath}.txn-stale-owner`, lockPath);
  const result = runApply(fixture);
  assert.notEqual(result.status, 0, "an unknown broken lock without owner evidence fails closed");
  assert.equal(readlinkSync(lockPath), `${lockPath}.txn-stale-owner`);
  const state = readFailureState(fixture);
  assert.equal(state.install_lock_status, "not_acquired");
});

withFixture((fixture) => {
  const lockPath = `${fixture.binLink}.star-harness-install.lock`;
  const staleTarget = `${lockPath}.txn-2147483647-deadtoken-1-1`;
  mkdirSync(dirname(lockPath), { recursive: true });
  symlinkSync(staleTarget, lockPath);
  const result = runApply(fixture);
  assert.equal(result.status, 0, "a broken lock with proof of a dead exact owner is reconciled");
  assert.equal(existsSync(lockPath), false);
  assert.equal(readlinkSync(fixture.binLink), join(fixture.root, "install", "fixture", "harness"));
});

for (const signal of ["HUP", "INT", "TERM"]) {
  withFixture((fixture) => {
    const expectedStatus = { HUP: 129, INT: 130, TERM: 143 }[signal];
    const result = runApply(fixture, { FAKE_SIGNAL_AFTER_LOCK_EFFECT: signal });
    assert.equal(result.status, expectedStatus, `${signal} after lock mkdir retains its signal exit code`);
    assert.equal(existsSync(`${fixture.binLink}.star-harness-install.lock`), false);
    const state = readFailureState(fixture);
    assert.equal(state.status, "failed_before_binary_publication");
    assert.equal(state.install_lock_status, "released");
    assert.equal(state.original_exit_status, expectedStatus);
    assert.equal(state.final_exit_status, expectedStatus);
  });
}

withFixture((fixture) => {
  const result = runApply(fixture, { FAKE_SIGNAL_AFTER_TRANSACTION_MKDIR: "TERM" });
  assert.equal(result.status, 143);
  assert.equal(existsSync(`${fixture.binLink}.star-harness-install.lock`), false);
  const transactionDirs = readdirSync(dirname(fixture.binLink)).filter((name) =>
    name.startsWith("harness.star-harness-install.lock.txn-"),
  );
  assert.deepEqual(transactionDirs, []);
  const state = readFailureState(fixture);
  assert.equal(state.install_lock_status, "not_acquired");
  assert.equal(state.original_exit_status, 143);
});

withFixture((fixture) => {
  const original = "owned by user\n";
  write(fixture.binLink, original);
  const result = runApply(fixture, { FAKE_BLOCK_LOCK_ON_ACQUIRE: "1" });
  assert.equal(result.status, 1);
  assert.equal(readFileSync(fixture.binLink, "utf8"), original);
  const state = readFailureState(fixture);
  assert.equal(state.status, "failed_with_install_lock_residual");
  assert.equal(state.binary_rollback_status, "failed_before_binary_publication");
  assert.equal(state.install_lock_status, "release_failed");
  assert.equal(state.original_exit_status, 1);
  assert.equal(state.final_exit_status, 1);
});

withFixture((fixture) => {
  mkdirSync(fixture.binLink, { recursive: true });
  write(join(fixture.binLink, "sentinel"), "preserve\n");
  const result = runApply(fixture);
  assert.notEqual(result.status, 0, "a pre-existing directory must be refused");
  assert.equal(readFileSync(join(fixture.binLink, "sentinel"), "utf8"), "preserve\n");
});

withFixture((fixture) => {
  const foreignTarget = join(fixture.root, "foreign-harness");
  write(foreignTarget, "#!/bin/sh\nexit 0\n", true);
  mkdirSync(dirname(fixture.binLink), { recursive: true });
  symlinkSync(foreignTarget, fixture.binLink);
  const result = runApply(fixture, { FAKE_FAIL_BEFORE_PUBLICATION: "1" });
  assert.notEqual(result.status, 0, "the injected pre-publication failure must fail apply");
  assert.equal(readlinkSync(fixture.binLink), foreignTarget);
  assert.equal(readFailureState(fixture).status, "failed_before_binary_publication");
});

withFixture((fixture) => {
  const brokenTarget = join(fixture.root, "missing-harness");
  mkdirSync(dirname(fixture.binLink), { recursive: true });
  symlinkSync(brokenTarget, fixture.binLink);
  const previousIdentity = lstatSync(fixture.binLink);
  const result = runApply(fixture, { FAKE_FAIL_AFTER_PUBLICATION: "1" });
  assert.notEqual(result.status, 0);
  assert.equal(readlinkSync(fixture.binLink), brokenTarget);
  const restoredIdentity = lstatSync(fixture.binLink);
  assert.equal(restoredIdentity.dev, previousIdentity.dev);
  assert.equal(restoredIdentity.ino, previousIdentity.ino);
});

withFixture((fixture) => {
  const result = runApply(fixture, { FAKE_FAIL_AFTER_PUBLICATION: "1" });
  assert.notEqual(result.status, 0, "the injected post-publication failure must fail apply");
  assert.equal(existsSync(fixture.binLink), false, "rollback removes only the link it created");
  assert.equal(readFailureState(fixture).status, "failed_and_created_binary_link_removed");
});

withFixture((fixture) => {
  const result = runApply(fixture, { FAKE_HELPER_FAIL_PUBLISH: "1" });
  assert.equal(result.status, 45);
  assert.equal(existsSync(fixture.binLink), false);
  const state = readFailureState(fixture);
  assert.equal(state.status, "failed_before_binary_publication");
  assert.equal(state.binary_rollback_status, "failed_before_binary_publication");
  assert.equal(state.install_lock_status, "released");
  assert.equal(existsSync(`${fixture.binLink}.star-harness-install.lock`), false);
});

withFixture((fixture) => {
  const result = runApply(fixture, { FAKE_HELPER_FAIL_PUBLISH_AFTER_EFFECT: "1" });
  assert.equal(result.status, 45, "publication helper failure must remain the primary exit code");
  assert.equal(existsSync(fixture.binLink), false, "an effected publication is reconciled and rolled back");
  const state = readFailureState(fixture);
  assert.equal(state.status, "failed_and_created_binary_link_removed");
  assert.equal(state.original_exit_status, 45);
  assert.equal(state.final_exit_status, 45);
  assert.equal(state.install_lock_status, "released");
});

withFixture((fixture) => {
  const result = runApply(fixture, { FAKE_DIRECTORY_BEFORE_PUBLISH: "1" });
  assert.notEqual(result.status, 0);
  assert.equal(lstatSync(fixture.binLink).isDirectory(), true);
  assert.deepEqual(readdirSync(fixture.binLink), [], "exact no-replace cannot create a link inside a foreign directory");
  const state = readFailureState(fixture);
  assert.equal(state.status, "failed_before_binary_publication_path_changed");
  assert.equal(state.install_lock_status, "released");
});

withFixture((fixture) => {
  const foreignDirectory = join(fixture.root, "foreign-directory");
  mkdirSync(foreignDirectory);
  const result = runApply(fixture, { FAKE_DIRECTORY_SYMLINK_BEFORE_PUBLISH: foreignDirectory });
  assert.notEqual(result.status, 0);
  assert.equal(readlinkSync(fixture.binLink), foreignDirectory);
  assert.deepEqual(readdirSync(foreignDirectory), [], "exact no-replace cannot follow a foreign directory symlink");
  const state = readFailureState(fixture);
  assert.equal(state.status, "failed_before_binary_publication_path_changed");
});

withFixture((fixture) => {
  const foreignTarget = join(fixture.root, "publication-race-owner");
  write(foreignTarget, "#!/bin/sh\nexit 0\n", true);
  const result = runApply(fixture, { FAKE_FOREIGN_BEFORE_PUBLISH: foreignTarget });
  assert.notEqual(result.status, 0);
  assert.equal(readlinkSync(fixture.binLink), foreignTarget, "publication never overwrites a racing owner");
  const state = readFailureState(fixture);
  assert.equal(state.status, "failed_before_binary_publication_path_changed");
  assert.equal(state.install_lock_status, "released");
});

withFixture((fixture) => {
  const previousTarget = join(fixture.root, "previous-harness");
  const foreignTarget = join(fixture.root, "publication-race-owner");
  write(previousTarget, "#!/bin/sh\nexit 0\n", true);
  write(foreignTarget, "#!/bin/sh\nexit 0\n", true);
  mkdirSync(dirname(fixture.binLink), { recursive: true });
  symlinkSync(previousTarget, fixture.binLink);
  const result = runApply(fixture, { FAKE_FOREIGN_BEFORE_PUBLISH: foreignTarget });
  assert.notEqual(result.status, 0);
  assert.equal(readlinkSync(fixture.binLink), foreignTarget);
  const state = readFailureState(fixture);
  assert.equal(state.status, "failed_before_binary_publication_path_changed");
  assert.equal(state.install_lock_status, "rollback_residual_preserved");
  const lockDir = `${fixture.binLink}.star-harness-install.lock`;
  assert.equal(existsSync(join(lockDir, "previous-link-witness")), true);
  assert.equal(existsSync(join(lockDir, "displaced-live-entry")), true);
});

withFixture((fixture) => {
  const previousTarget = join(fixture.root, "previous-harness");
  write(previousTarget, "#!/bin/sh\nexit 0\n", true);
  mkdirSync(dirname(fixture.binLink), { recursive: true });
  symlinkSync(previousTarget, fixture.binLink);
  const previousIdentity = lstatSync(fixture.binLink);
  const result = runApply(fixture, { FAKE_FAIL_AFTER_PUBLICATION: "1" });
  assert.notEqual(result.status, 0, "the injected post-publication failure must fail apply");
  assert.equal(readlinkSync(fixture.binLink), previousTarget);
  const restoredIdentity = lstatSync(fixture.binLink);
  assert.equal(restoredIdentity.dev, previousIdentity.dev);
  assert.equal(restoredIdentity.ino, previousIdentity.ino, "rollback restores the exact previous link object");
  assert.equal(readFailureState(fixture).status, "failed_and_previous_binary_restored");
});

withFixture((fixture) => {
  const foreignTarget = join(fixture.root, "foreign-harness");
  write(foreignTarget, "#!/bin/sh\nexit 0\n", true);
  const result = runApply(fixture, {
    FAKE_FAIL_AFTER_PUBLICATION: "1",
    FAKE_FOREIGN_TARGET: foreignTarget,
  });
  assert.notEqual(result.status, 0, "the injected post-publication failure must fail apply");
  assert.equal(
    readlinkSync(fixture.binLink),
    foreignTarget,
    "rollback must preserve a link changed by another owner",
  );
  assert.equal(
    readFailureState(fixture).status,
    "failed_after_binary_publication_link_changed",
  );
});

withFixture((fixture) => {
  const previousTarget = join(fixture.root, "previous-harness");
  const foreignTarget = join(fixture.root, "displacement-race-owner");
  write(previousTarget, "#!/bin/sh\nexit 0\n", true);
  write(foreignTarget, "#!/bin/sh\nexit 0\n", true);
  mkdirSync(dirname(fixture.binLink), { recursive: true });
  symlinkSync(previousTarget, fixture.binLink);
  const result = runApply(fixture, { FAKE_FOREIGN_BEFORE_DISPLACE: foreignTarget });
  assert.notEqual(result.status, 0);
  assert.equal(readlinkSync(fixture.binLink), foreignTarget, "a displaced foreign entry is restored without overwrite");
  const state = readFailureState(fixture);
  assert.equal(state.status, "failed_before_binary_publication_path_changed");
  assert.equal(state.install_lock_status, "released");
});

withFixture((fixture) => {
  const previousTarget = join(fixture.root, "previous-harness");
  write(previousTarget, "#!/bin/sh\nexit 0\n", true);
  mkdirSync(dirname(fixture.binLink), { recursive: true });
  symlinkSync(previousTarget, fixture.binLink);
  const result = runApply(fixture, { FAKE_DIRECTORY_BEFORE_DISPLACE: "1" });
  assert.notEqual(result.status, 0);
  assert.equal(lstatSync(fixture.binLink).isDirectory(), true);
  assert.deepEqual(readdirSync(fixture.binLink), [], "a quarantined foreign directory is atomically restored");
  const state = readFailureState(fixture);
  assert.equal(state.status, "failed_before_binary_publication_path_changed");
  assert.equal(state.install_lock_status, "released");
});

withFixture((fixture) => {
  const previousTarget = join(fixture.root, "previous-harness");
  write(previousTarget, "#!/bin/sh\nexit 0\n", true);
  mkdirSync(dirname(fixture.binLink), { recursive: true });
  symlinkSync(previousTarget, fixture.binLink);
  const previousIdentity = lstatSync(fixture.binLink);
  const result = runApply(fixture, { FAKE_DELETE_BEFORE_DISPLACE: "1" });
  assert.equal(result.status, 46);
  assert.equal(readlinkSync(fixture.binLink), previousTarget);
  const restoredIdentity = lstatSync(fixture.binLink);
  assert.equal(restoredIdentity.dev, previousIdentity.dev);
  assert.equal(restoredIdentity.ino, previousIdentity.ino);
  const state = readFailureState(fixture);
  assert.equal(state.status, "failed_before_binary_publication");
  assert.equal(state.install_lock_status, "released");
});

withFixture((fixture) => {
  const foreignTarget = join(fixture.root, "rollback-race-owner");
  write(foreignTarget, "#!/bin/sh\nexit 0\n", true);
  const result = runApply(fixture, {
    FAKE_CLAUDE_FAIL_AFTER_PUBLICATION: "1",
    FAKE_FOREIGN_BEFORE_ROLLBACK: foreignTarget,
  });
  assert.equal(result.status, 42, "rollback reconciliation preserves the primary failure");
  assert.equal(readlinkSync(fixture.binLink), foreignTarget, "rollback never deletes a racing owner");
  const state = readFailureState(fixture);
  assert.equal(state.status, "failed_after_binary_publication_link_changed");
  assert.equal(state.install_lock_status, "released");
});

withFixture((fixture) => {
  const result = runApply(fixture, {
    FAKE_CLAUDE_FAIL_AFTER_PUBLICATION: "1",
    FAKE_DIRECTORY_BEFORE_ROLLBACK: "1",
  });
  assert.equal(result.status, 42);
  assert.equal(lstatSync(fixture.binLink).isDirectory(), true);
  assert.deepEqual(readdirSync(fixture.binLink), [], "rollback restores a racing foreign directory");
  const state = readFailureState(fixture);
  assert.equal(state.status, "failed_after_binary_publication_link_changed");
  assert.equal(state.install_lock_status, "released");
});

withFixture((fixture) => {
  const previousTarget = join(fixture.root, "previous-harness");
  write(previousTarget, "#!/bin/sh\nexit 0\n", true);
  mkdirSync(dirname(fixture.binLink), { recursive: true });
  symlinkSync(previousTarget, fixture.binLink);
  const previousIdentity = lstatSync(fixture.binLink);
  const result = runApply(fixture, {
    FAKE_CLAUDE_FAIL_AFTER_PUBLICATION: "1",
    FAKE_DELETE_BEFORE_ROLLBACK: "1",
  });
  assert.equal(result.status, 42);
  assert.equal(readlinkSync(fixture.binLink), previousTarget);
  const restoredIdentity = lstatSync(fixture.binLink);
  assert.equal(restoredIdentity.dev, previousIdentity.dev);
  assert.equal(restoredIdentity.ino, previousIdentity.ino);
  const state = readFailureState(fixture);
  assert.equal(state.status, "failed_and_previous_binary_restored");
  assert.equal(state.install_lock_status, "released");
});

withFixture((fixture) => {
  const result = runApply(fixture, {
    FAKE_FAIL_AFTER_PUBLICATION: "1",
    FAKE_RECREATE_SAME_TARGET: "1",
  });
  assert.notEqual(result.status, 0, "the injected post-publication failure must fail apply");
  assert.equal(
    readlinkSync(fixture.binLink),
    join(fixture.root, "install", "fixture", "harness"),
    "rollback must preserve a same-target link recreated by another owner",
  );
  assert.equal(
    readFailureState(fixture).status,
    "failed_after_binary_publication_link_changed",
  );
});

withFixture((fixture) => {
  const result = runApply(fixture, {
    FAKE_CLAUDE_FAIL_AFTER_PUBLICATION: "1",
    FAKE_UNLINK_FAIL_PATH: fixture.binLink,
  });
  assert.equal(result.status, 42, "rollback failure must preserve the original installer exit code");
  assert.equal(existsSync(fixture.binLink), true, "failed rollback must retain its residual link");
  assert.equal(
    readFailureState(fixture).status,
    "failed_after_binary_publication_remove_failed",
  );
  assert.equal(
    readFailureState(fixture).install_lock_status,
    "rollback_residual_preserved",
  );
  const lockDir = `${fixture.binLink}.star-harness-install.lock`;
  assert.equal(existsSync(lockDir), true);
  assert.equal(existsSync(join(lockDir, "published-link-witness")), true);
});

withFixture((fixture) => {
  const previousTarget = join(fixture.root, "previous-harness");
  write(previousTarget, "#!/bin/sh\nexit 0\n", true);
  mkdirSync(dirname(fixture.binLink), { recursive: true });
  symlinkSync(previousTarget, fixture.binLink);
  const previousIdentity = lstatSync(fixture.binLink);
  const result = runApply(fixture, {
    FAKE_CLAUDE_FAIL_AFTER_PUBLICATION: "1",
    FAKE_HELPER_FAIL_RESTORE_AFTER_EFFECT: "1",
  });
  assert.equal(result.status, 42);
  assert.equal(readlinkSync(fixture.binLink), previousTarget);
  const restoredIdentity = lstatSync(fixture.binLink);
  assert.equal(restoredIdentity.dev, previousIdentity.dev);
  assert.equal(restoredIdentity.ino, previousIdentity.ino);
  const state = readFailureState(fixture);
  assert.equal(state.status, "failed_and_previous_binary_restored");
  assert.equal(state.install_lock_status, "released");
  assert.equal(existsSync(`${fixture.binLink}.star-harness-install.lock`), false);
});

withFixture((fixture) => {
  const previousTarget = join(fixture.root, "previous-harness");
  write(previousTarget, "#!/bin/sh\nexit 0\n", true);
  mkdirSync(dirname(fixture.binLink), { recursive: true });
  symlinkSync(previousTarget, fixture.binLink);
  const result = runApply(fixture, {
    FAKE_CLAUDE_FAIL_AFTER_PUBLICATION: "1",
    FAKE_HELPER_FAIL_RESTORE: "1",
  });
  assert.equal(result.status, 42);
  assert.equal(existsSync(fixture.binLink), false);
  const state = readFailureState(fixture);
  assert.equal(state.status, "failed_after_binary_publication_restore_failed");
  assert.equal(state.install_lock_status, "rollback_residual_preserved");
  const lockDir = `${fixture.binLink}.star-harness-install.lock`;
  assert.equal(existsSync(join(lockDir, "previous-link-witness")), true);
  assert.equal(existsSync(join(lockDir, "published-link-witness")), true);
  assert.equal(existsSync(join(lockDir, "rollback-live-entry")), true);
});

await withFixtureAsync(async (fixture) => {
  const result = await terminateAfterPublication(fixture);
  assert.equal(result.code, 143);
  assert.equal(result.signal, null);
  assert.equal(existsSync(fixture.binLink), false);
  const state = readFailureState(fixture);
  assert.equal(state.status, "failed_and_created_binary_link_removed");
  assert.equal(state.original_exit_status, 143);
  assert.equal(state.final_exit_status, 143);
  assert.equal(existsSync(`${fixture.binLink}.star-harness-install.lock`), false);
});

await withFixtureAsync(async (fixture) => {
  const result = await terminateAgainDuringCleanup(fixture);
  assert.equal(result.code, 143);
  assert.equal(result.signal, null);
  assert.equal(existsSync(fixture.binLink), false);
  const state = readFailureState(fixture);
  assert.equal(state.status, "failed_and_created_binary_link_removed");
  assert.equal(state.original_exit_status, 143);
  assert.equal(state.final_exit_status, 143);
  assert.equal(existsSync(`${fixture.binLink}.star-harness-install.lock`), false);
});

await withFixtureAsync(async (fixture) => {
  const previousTarget = join(fixture.root, "previous-harness");
  write(previousTarget, "#!/bin/sh\nexit 0\n", true);
  mkdirSync(dirname(fixture.binLink), { recursive: true });
  symlinkSync(previousTarget, fixture.binLink);
  const previousIdentity = lstatSync(fixture.binLink);
  const result = await terminateAfterPublication(fixture);
  assert.equal(result.code, 143);
  assert.equal(readlinkSync(fixture.binLink), previousTarget);
  const restoredIdentity = lstatSync(fixture.binLink);
  assert.equal(restoredIdentity.dev, previousIdentity.dev);
  assert.equal(restoredIdentity.ino, previousIdentity.ino);
  assert.equal(readFailureState(fixture).status, "failed_and_previous_binary_restored");
});

withFixture((fixture) => {
  const result = runApply(fixture, { FAKE_BLOCK_LOCK_RELEASE: "1" });
  assert.equal(result.status, 1, "lock release failure must turn an otherwise successful apply into failure");
  const state = readFailureState(fixture);
  assert.equal(state.status, "failed_with_install_lock_residual");
  assert.equal(state.install_lock_status, "release_failed");
  assert.equal(state.original_exit_status, 0);
  assert.equal(state.final_exit_status, 1);
  assert.equal(state.binary_rollback_status, "not_attempted_install_completed");
  assert.equal(readlinkSync(fixture.binLink), join(fixture.root, "install", "fixture", "harness"));
  assert.equal(existsSync(`${fixture.binLink}.star-harness-install.lock`), false);
  const transactions = transactionDirectories(fixture);
  assert.equal(transactions.length, 1, "private cleanup evidence remains without blocking the public lock");
  assert.equal(existsSync(join(transactions[0], "residual")), true);
});

withFixture((fixture) => {
  const result = runApply(fixture, { FAKE_FAIL_LOCK_RELEASE_AFTER_EFFECT: "1" });
  assert.equal(result.status, 1, "an effect-after-error lock release cannot report success");
  const state = readFailureState(fixture);
  assert.equal(state.status, "failed_install_lock_release");
  assert.equal(state.install_lock_status, "release_failed_no_residual");
  assert.equal(state.original_exit_status, 0);
  assert.equal(state.final_exit_status, 1);
  assert.equal(existsSync(`${fixture.binLink}.star-harness-install.lock`), false);
  assert.deepEqual(transactionDirectories(fixture), []);
});

for (const replacementKind of ["regular", "symlink", "directory"]) {
  withFixture((fixture) => {
    const lockPath = `${fixture.binLink}.star-harness-install.lock`;
    const result = runApply(fixture, { FAKE_LOCK_REPLACEMENT_KIND: replacementKind });
    assert.equal(result.status, 1, `a foreign ${replacementKind} lock replacement fails closed`);
    if (replacementKind === "regular") {
      assert.equal(readFileSync(lockPath, "utf8"), "foreign\n");
    } else if (replacementKind === "symlink") {
      assert.equal(readlinkSync(lockPath), "foreign-lock");
    } else {
      assert.equal(lstatSync(lockPath).isDirectory(), true);
      assert.deepEqual(readdirSync(lockPath), []);
    }
    const state = readFailureState(fixture);
    assert.equal(state.install_lock_status, "release_failed");
    assert.equal(state.original_exit_status, 0);
    assert.equal(state.final_exit_status, 1);
    assert.equal(transactionDirectories(fixture).length, 1);
  });
}

await withFixtureAsync(async (fixture) => {
  const ready = join(fixture.root, "installer-holds-public-lock");
  const release = join(fixture.root, "release-orphan-provider");
  const installer = spawn(
    "bash",
    [join(fixture.repo, "scripts/manage-star-harness-install.sh"), "--apply"],
    {
      cwd: fixture.repo,
      env: applyEnvironment(fixture, {
        FAKE_HOLD_READY: ready,
        FAKE_HOLD_RELEASE: release,
      }),
      stdio: "ignore",
    },
  );
  waitForPath(ready, installer);
  const exit = new Promise((resolvePromise) => installer.once("exit", resolvePromise));
  installer.kill("SIGKILL");
  await exit;
  write(release, "continue\n");
  const lockPath = `${fixture.binLink}.star-harness-install.lock`;
  assert.equal(existsSync(lockPath), true);
  assert.equal(existsSync(readlinkSync(lockPath)), true, "SIGKILL leaves the old private transaction");
  const retry = runApply(fixture);
  assert.equal(retry.status, 0, "a dead exact lock owner is safely reconciled on retry");
  assert.equal(existsSync(lockPath), false);
  assert.equal(transactionDirectories(fixture).length, 1, "only the dead owner's private evidence remains");
});

await withFixtureAsync(async (fixture) => {
  const ready = join(fixture.root, "public-lock-moved");
  const release = join(fixture.root, "release-orphan-helper");
  const installer = spawn(
    "bash",
    [join(fixture.repo, "scripts/manage-star-harness-install.sh"), "--apply"],
    {
      cwd: fixture.repo,
      env: applyEnvironment(fixture, {
        FAKE_HOLD_AFTER_PUBLIC_LOCK_MOVE_READY: ready,
        FAKE_HOLD_AFTER_PUBLIC_LOCK_MOVE_RELEASE: release,
      }),
      stdio: "ignore",
    },
  );
  waitForPath(ready, installer);
  const exit = new Promise((resolvePromise) => installer.once("exit", resolvePromise));
  installer.kill("SIGKILL");
  await exit;
  write(release, "continue\n");
  assert.equal(
    existsSync(`${fixture.binLink}.star-harness-install.lock`),
    false,
    "death after the atomic public release cannot leave a stale public lock",
  );
  const retry = runApply(fixture);
  assert.equal(retry.status, 0, "a later installer is not blocked by private crash residue");
});

withFixture((fixture) => {
  const installer = join(fixture.repo, "scripts/manage-star-harness-install.sh");
  write(installer, `${readFileSync(installer, "utf8")}\nfalse\n`, true);
  const result = runApply(fixture);
  assert.equal(result.status, 1, "post-completion display failure remains a process failure");
  const stateDir = join(fixture.root, "state", "installations");
  const states = readdirSync(stateDir);
  assert.equal(states.length, 1);
  const state = JSON.parse(readFileSync(join(stateDir, states[0]), "utf8"));
  assert.equal(state.version, "fixture");
  assert.equal("status" in state, false, "completed installation state must not be rewritten as failure");
  assert.equal(readlinkSync(fixture.binLink), join(fixture.root, "install", "fixture", "harness"));
  assert.equal(existsSync(`${fixture.binLink}.star-harness-install.lock`), false);
});

withFixture((fixture) => {
  const result = runApply(fixture, {
    FAKE_BLOCK_LOCK_RELEASE: "1",
    FAKE_CLAUDE_FAIL_AFTER_PUBLICATION: "1",
  });
  assert.equal(result.status, 42, "cleanup failures cannot replace the primary exit code");
  const state = readFailureState(fixture);
  assert.equal(state.status, "failed_with_install_lock_residual");
  assert.equal(state.install_lock_status, "release_failed");
  assert.equal(state.original_exit_status, 42);
  assert.equal(state.final_exit_status, 42);
  assert.equal(state.binary_rollback_status, "failed_and_created_binary_link_removed");
  assert.equal(existsSync(fixture.binLink), false);
  assert.equal(existsSync(`${fixture.binLink}.star-harness-install.lock`), false);
  assert.equal(transactionDirectories(fixture).length, 1);
});

{
  const fixture = createFixture();
  const ready = join(fixture.root, "first-installer-published");
  const release = join(fixture.root, "release-first-installer");
  const first = spawn(
    "bash",
    [join(fixture.repo, "scripts/manage-star-harness-install.sh"), "--apply"],
    {
      cwd: fixture.repo,
      env: applyEnvironment(fixture, {
        FAKE_FAIL_AFTER_PUBLICATION: "1",
        FAKE_HOLD_READY: ready,
        FAKE_HOLD_RELEASE: release,
      }),
      stdio: "ignore",
    },
  );
  try {
    waitForPath(ready, first);
    const publishedTarget = readlinkSync(fixture.binLink);
    const second = runApply(fixture);
    assert.notEqual(second.status, 0, "a concurrent installer must fail closed");
    assert.match(second.stderr, /publication is already owned by another installer/);
    const rejectedState = readFailureState(fixture);
    assert.equal(rejectedState.status, "failed_before_binary_publication");
    assert.equal(rejectedState.install_lock_status, "not_acquired");
    assert.equal(
      readlinkSync(fixture.binLink),
      publishedTarget,
      "the rejected installer cannot enter the publication critical section",
    );
    const exit = new Promise((resolvePromise) => {
      first.once("exit", (...args) => resolvePromise(args));
    });
    write(release, "continue\n");
    const [code] = await exit;
    assert.notEqual(code, 0, "the held installer must take its injected failure");
    assert.equal(existsSync(fixture.binLink), false, "the owning installer rolls back its link");
    assert.equal(
      existsSync(`${fixture.binLink}.star-harness-install.lock`),
      false,
      "the owning installer releases the publication lock on exit",
    );
    assert.equal(
      readdirSync(join(fixture.root, "state", "installations")).length,
      2,
      "concurrent installers retain distinct failure states",
    );
  } finally {
    if (first.exitCode === null) {
      write(release, "continue\n");
      first.kill("SIGKILL");
    }
    rmSync(fixture.root, { recursive: true, force: true });
  }
}

console.log("star-harness installer rollback boundary: PASS");
