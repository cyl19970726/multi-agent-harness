#!/usr/bin/env node
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
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
    "#!/bin/sh\nif [ \"$1 $2\" = 'plugin add' ] && [ \"${FAKE_FAIL_AFTER_PUBLICATION:-0}\" = 1 ]; then\n  if [ -n \"${FAKE_FOREIGN_TARGET:-}\" ]; then ln -sfn \"${FAKE_FOREIGN_TARGET}\" \"${STAR_HARNESS_BIN_LINK}\"; fi\n  exit 42\nfi\nexit 0\n",
    true,
  );
  write(join(fakeBin, "claude"), "#!/bin/sh\nexit 0\n", true);

  return { root, repo, fakeBin, home, binLink };
}

function runApply(fixture, extraEnv = {}) {
  return spawnSync("bash", [join(fixture.repo, "scripts/manage-star-harness-install.sh"), "--apply"], {
    cwd: fixture.repo,
    encoding: "utf8",
    env: {
      ...process.env,
      ...extraEnv,
      HOME: fixture.home,
      PATH: `${fixture.fakeBin}:${process.env.PATH}`,
      STAR_HARNESS_BIN_LINK: fixture.binLink,
      STAR_HARNESS_INSTALL_ROOT: join(fixture.root, "install"),
      STAR_HARNESS_STATE_ROOT: join(fixture.root, "state"),
      KIMI_CODE_HOME: join(fixture.root, "kimi"),
    },
  });
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

console.log("star-harness installer rollback boundary: PASS");
