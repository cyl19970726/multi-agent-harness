#!/usr/bin/env node

import { chmodSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const hook = join(
  repoRoot,
  "plugins",
  "star-harness",
  "scripts",
  "star-harness-hook.sh",
);
const temp = mkdtempSync(join(tmpdir(), "star-harness-hook-"));
const fakeHarness = join(temp, "harness");

writeFileSync(
  fakeHarness,
  `#!/usr/bin/env bash
set -euo pipefail
if [[ "\${1:-} \${2:-} \${3:-}" == "team-run list --json" ]]; then
  printf '%s\\n' '{"runs":[{"id":"run-1","status":"running","member_run_ids":["member-1"],"project_id":"demo"}]}'
elif [[ "\${1:-} \${2:-}" == "team-run inbox" ]]; then
  printf '%s\\n' '[{"id":"msg-1","from_member_id":"member-1","kind":"handoff","correlation_id":"corr-1","body":"RESULT: done\\nChecks passed"}]'
elif [[ "\${1:-} \${2:-}" == "hook record" ]]; then
  exit 0
else
  printf 'unexpected fake harness args: %s\\n' "$*" >&2
  exit 1
fi
`,
);
chmodSync(fakeHarness, 0o755);

function run(event, extraEnv = {}) {
  const result = spawnSync("bash", [hook], {
    input: JSON.stringify({ hook_event_name: event }),
    encoding: "utf8",
    env: {
      ...process.env,
      HARNESS_BIN: fakeHarness,
      ...extraEnv,
    },
  });
  if (result.status !== 0) {
    throw new Error(
      `${event} hook failed (${result.status}): ${result.stderr || result.stdout}`,
    );
  }
  return result.stdout;
}

try {
  const started = run("SessionStart");
  if (!started.includes("active TeamRun=run-1")) {
    throw new Error("SessionStart must orient the Host to the active TeamRun");
  }
  if (
    !started.includes("Needs you: TeamRun=run-1 pending_host_messages=1") ||
    !started.includes("from=member-1 kind=handoff message=msg-1")
  ) {
    throw new Error("SessionStart must include a bounded Host Inbox summary");
  }

  const prompt = run("UserPromptSubmit");
  if (prompt.includes("active TeamRun=")) {
    throw new Error("UserPromptSubmit should not repeat active-run orientation");
  }
  if (!prompt.includes("Needs you: TeamRun=run-1 pending_host_messages=1")) {
    throw new Error("UserPromptSubmit must surface actionable Host mail");
  }

  const member = run("SessionStart", {
    HARNESS_AGENT_MEMBER_ID: "member-1",
  });
  if (member.trim()) {
    throw new Error("A Member hook must not receive the Lead Inbox");
  }

  const stopped = run("Stop");
  if (stopped.trim()) {
    throw new Error("Stop must not inject Host Inbox orientation");
  }

  console.log("Star Harness Host Inbox hook contract is valid");
} finally {
  rmSync(temp, { recursive: true, force: true });
}
