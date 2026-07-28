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
if [[ "\${1:-} \${2:-}" == "team-run host-inbox" ]]; then
  printf '%s\\n' '[{"team_run_id":"run-1","team_run_status":"running","mission_id":"mission-1","messages":[{"id":"msg-1","from_member_id":"member-1","kind":"handoff","correlation_id":"corr-1","body":"RESULT: done\\nChecks passed"}]}]'
elif [[ "\${1:-} \${2:-}" == "hook record" ]]; then
  exit 0
else
  printf 'unexpected fake harness args: %s\\n' "$*" >&2
  exit 1
fi
`,
);
chmodSync(fakeHarness, 0o755);

function run(event, extraEnv = {}, extraPayload = {}) {
  const result = spawnSync("bash", [hook], {
    input: JSON.stringify({
      hook_event_name: event,
      session_id: "codex-session-1",
      ...extraPayload,
    }),
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
  if (
    !started.includes(
      "Host native binding: surface=codex-app thread=codex-session-1",
    )
  ) {
    throw new Error("SessionStart must expose the exact native Host binding");
  }
  if (
    !started.includes("Needs you: TeamRun=run-1 pending_host_messages=1") ||
    !started.includes("from=member-1 kind=handoff message=msg-1")
  ) {
    throw new Error("SessionStart must include a bounded Host Inbox summary");
  }

  const prompt = run("UserPromptSubmit");
  if (prompt.includes("Host native binding:")) {
    throw new Error("UserPromptSubmit should not repeat Host binding orientation");
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

  const stopped = JSON.parse(
    run("Stop", {}, { turn_id: "turn-1", stop_hook_active: false }),
  );
  if (
    stopped.decision !== "block" ||
    !stopped.reason.includes("received new coordination mail") ||
    !stopped.reason.includes("TeamRun=run-1")
  ) {
    throw new Error(
      "Codex Stop must continue the same native task with bounded Host mail",
    );
  }

  const continued = JSON.parse(
    run("Stop", {}, { turn_id: "turn-2", stop_hook_active: true }),
  );
  if (Object.keys(continued).length !== 0) {
    throw new Error("A continued Stop hook must not create a continuation loop");
  }

  console.log("Star Harness Host Inbox hook contract is valid");
} finally {
  rmSync(temp, { recursive: true, force: true });
}
