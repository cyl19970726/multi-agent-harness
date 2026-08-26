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
    join(fakeBin, "npm"),
    "#!/bin/sh\nif [ \"${FAKE_FAIL_BEFORE_PUBLICATION:-0}\" = 1 ]; then exit 41; fi\nexit 0\n",
    true,
  );
  write(
    join(fakeBin, "codex"),
    "#!/bin/sh\nif [ \"$1 $2\" = 'plugin add' ]; then\n  if [ \"${FAKE_BLOCK_LOCK_RELEASE:-0}\" = 1 ]; then touch \"${STAR_HARNESS_BIN_LINK}.star-harness-install.lock/residual\"; fi\n  if [ \"${FAKE_FAIL_AFTER_PUBLICATION:-0}\" = 1 ]; then\n    if [ -n \"${FAKE_FOREIGN_TARGET:-}\" ]; then ln -sfn \"${FAKE_FOREIGN_TARGET}\" \"${STAR_HARNESS_BIN_LINK}\"; fi\n    if [ \"${FAKE_RECREATE_SAME_TARGET:-0}\" = 1 ]; then target=$(readlink \"${STAR_HARNESS_BIN_LINK}\"); unlink \"${STAR_HARNESS_BIN_LINK}\"; ln -s \"$target\" \"${STAR_HARNESS_BIN_LINK}\"; fi\n    if [ -n \"${FAKE_HOLD_READY:-}\" ]; then touch \"${FAKE_HOLD_READY}\"; while [ ! -e \"${FAKE_HOLD_RELEASE}\" ]; do sleep 0.01; done; fi\n    exit 42\n  fi\nfi\nexit 0\n",
    true,
  );
  write(
    join(fakeBin, "claude"),
    "#!/bin/sh\nif [ \"$1 $2\" = 'plugin install' ] && [ \"${FAKE_CLAUDE_FAIL_AFTER_PUBLICATION:-0}\" = 1 ]; then exit 42; fi\nexit 0\n",
    true,
  );
  write(
    join(fakeBin, "unlink"),
    "#!/bin/sh\nif [ \"${FAKE_UNLINK_FAIL_PATH:-}\" = \"$1\" ]; then exit 43; fi\nexec /bin/rm -f -- \"$1\"\n",
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
  return {
    ...process.env,
    ...extraEnv,
    HOME: fixture.home,
    PATH: `${fixture.fakeBin}:${process.env.PATH}`,
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

function readFailureState(fixture) {
  const stateDir = join(fixture.root, "state", "installations");
  const files = readdirSync(stateDir);
  assert.equal(files.length, 1, "failed apply must write one installation state");
  return JSON.parse(readFileSync(join(stateDir, files[0]), "utf8"));
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
  const result = runApply(fixture, { FAKE_FAIL_AFTER_PUBLICATION: "1" });
  assert.notEqual(result.status, 0, "the injected post-publication failure must fail apply");
  assert.equal(existsSync(fixture.binLink), false, "rollback removes only the link it created");
  assert.equal(readFailureState(fixture).status, "failed_and_created_binary_link_removed");
});

withFixture((fixture) => {
  const previousTarget = join(fixture.root, "previous-harness");
  write(previousTarget, "#!/bin/sh\nexit 0\n", true);
  mkdirSync(dirname(fixture.binLink), { recursive: true });
  symlinkSync(previousTarget, fixture.binLink);
  const result = runApply(fixture, { FAKE_FAIL_AFTER_PUBLICATION: "1" });
  assert.notEqual(result.status, 0, "the injected post-publication failure must fail apply");
  assert.equal(readlinkSync(fixture.binLink), previousTarget);
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
  assert.equal(existsSync(`${fixture.binLink}.star-harness-install.lock`), true);
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
  assert.equal(existsSync(`${fixture.binLink}.star-harness-install.lock`), true);
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
    assert.equal(
      readlinkSync(fixture.binLink),
      publishedTarget,
      "the rejected installer cannot enter the publication critical section",
    );
    write(release, "continue\n");
    const [code] = await new Promise((resolvePromise) => {
      first.once("exit", (...args) => resolvePromise(args));
    });
    assert.notEqual(code, 0, "the held installer must take its injected failure");
    assert.equal(existsSync(fixture.binLink), false, "the owning installer rolls back its link");
    assert.equal(
      existsSync(`${fixture.binLink}.star-harness-install.lock`),
      false,
      "the owning installer releases the publication lock on exit",
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
