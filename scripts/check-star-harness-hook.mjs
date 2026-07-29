#!/usr/bin/env node

import {
  chmodSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
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
const hookLog = join(temp, "hook-record.log");

writeFileSync(
  fakeHarness,
  `#!/usr/bin/env bash
set -euo pipefail
if [[ "\${1:-} \${2:-}" == "team-run host-inbox" ]]; then
  printf '%s\\n' '[{"team_run_id":"run-1","team_run_status":"running","mission_id":"mission-1","messages":[{"id":"msg-1","from_member_id":"member-1","kind":"handoff","correlation_id":"corr-1","body":"RESULT: done\\nChecks passed"}]}]'
elif [[ "\${1:-} \${2:-}" == "hook record" ]]; then
  printf '%s\\n' "$*" >> "\${HOOK_LOG:?}"
  exit 0
else
  printf 'unexpected fake harness args: %s\\n' "$*" >&2
  exit 1
fi
`,
);
chmodSync(fakeHarness, 0o755);

function runRaw(event, extraEnv = {}, extraPayload = {}) {
  return spawnSync("bash", [hook], {
    input: JSON.stringify({
      hook_event_name: event,
      session_id: "codex-session-1",
      ...extraPayload,
    }),
    encoding: "utf8",
    env: {
      ...process.env,
      HARNESS_BIN: fakeHarness,
      HARNESS_HOST_SURFACE: "",
      CLAUDE_PLUGIN_ROOT: "",
      KIMI_PLUGIN_ROOT: "",
      HOOK_LOG: hookLog,
      ...extraEnv,
    },
  });
}

function run(event, extraEnv = {}, extraPayload = {}) {
  const result = runRaw(event, extraEnv, extraPayload);
  if (result.status !== 0) {
    throw new Error(
      `${event} hook failed (${result.status}): ${result.stderr || result.stdout}`,
    );
  }
  return result.stdout;
}

try {
  const started = run("SessionStart", { HARNESS_HOST_SURFACE: "codex-app" });
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

  const prompt = run("UserPromptSubmit", {
    HARNESS_HOST_SURFACE: "codex-app",
  });
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
  run("SessionStart", {
    HARNESS_AGENT_MEMBER_ID: "member-claude",
    CLAUDE_PLUGIN_ROOT: temp,
  });
  run("SessionStart", {
    HARNESS_AGENT_MEMBER_ID: "member-kimi",
    KIMI_PLUGIN_ROOT: temp,
  });
  const recordedHooks = readFileSync(hookLog, "utf8");
  for (const expected of [
    "hook record --provider codex --agent member-1",
    "hook record --provider claude --agent member-claude",
    "hook record --provider kimi --agent member-kimi",
  ]) {
    if (!recordedHooks.includes(expected)) {
      throw new Error(`Member hook did not preserve provider identity: ${expected}`);
    }
  }

  const stopped = JSON.parse(
    run(
      "Stop",
      { HARNESS_HOST_SURFACE: "codex-app" },
      { turn_id: "turn-1", stop_hook_active: false },
    ),
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
    run(
      "Stop",
      { HARNESS_HOST_SURFACE: "codex-app" },
      { turn_id: "turn-2", stop_hook_active: true },
    ),
  );
  if (Object.keys(continued).length !== 0) {
    throw new Error("A continued Stop hook must not create a continuation loop");
  }

  const claudeStarted = run("SessionStart", {
    CLAUDE_PLUGIN_ROOT: temp,
  });
  if (
    !claudeStarted.includes(
      "Host native binding: surface=claude-code thread=codex-session-1",
    )
  ) {
    throw new Error("Claude hooks must use the claude-code Host surface");
  }
  const claudeStopped = JSON.parse(
    run(
      "Stop",
      { CLAUDE_PLUGIN_ROOT: temp },
      { stop_hook_active: false },
    ),
  );
  if (
    claudeStopped.decision !== "block" ||
    !claudeStopped.reason.includes("TeamRun=run-1")
  ) {
    throw new Error("Claude Stop must continue the exact native Host task");
  }

  const kimiPrompt = run("UserPromptSubmit", {
    KIMI_PLUGIN_ROOT: temp,
  });
  if (!kimiPrompt.includes("Needs you: TeamRun=run-1")) {
    throw new Error("Kimi UserPromptSubmit must surface Host mail");
  }
  const kimiStopped = runRaw(
    "Stop",
    { KIMI_PLUGIN_ROOT: temp },
    { stop_hook_active: false },
  );
  if (
    kimiStopped.status !== 2 ||
    !kimiStopped.stderr.includes("TeamRun=run-1")
  ) {
    throw new Error(
      `Kimi Stop must block through exit 2 with a reason; status=${kimiStopped.status} stderr=${kimiStopped.stderr}`,
    );
  }
  const kimiContinued = runRaw(
    "Stop",
    { KIMI_PLUGIN_ROOT: temp },
    { stop_hook_active: true },
  );
  if (kimiContinued.status !== 0 || kimiContinued.stdout.trim()) {
    throw new Error("A continued Kimi Stop must allow exit without output");
  }

  console.log("Star Harness Codex, Claude, and Kimi Host Inbox hooks are valid");
} finally {
  rmSync(temp, { recursive: true, force: true });
}
