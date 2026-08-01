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
elif [[ "\${1:-} \${2:-}" == "member-run show" ]]; then
  member_id=""
  while [[ \$# -gt 0 ]]; do
    if [[ "\$1" == "--id" ]]; then
      member_id="\${2:-}"
      break
    fi
    shift
  done
  if [[ "\$member_id" == "member-run-driven" ]]; then
    printf '%s\\n' '{"member_run":{"id":"member-run-driven","team_run_id":"run-1","status":"running","provider_profile":{"execution_mode":"codex_app_server","execution_driver":"host_driven"}}}'
  elif [[ "\$member_id" == "member-run-stopped" ]]; then
    printf '%s\\n' '{"member_run":{"id":"member-run-stopped","team_run_id":"run-1","coordination_status":"closed","status":"idle","provider_profile":{"execution_mode":"external_interactive","execution_driver":"user_driven"}}}'
  else
    printf '{"member_run":{"id":"%s","team_run_id":"run-1","status":"idle","provider_profile":{"execution_mode":"external_interactive","execution_driver":"user_driven"}}}\\n' "\$member_id"
  fi
elif [[ "\${1:-} \${2:-}" == "team-run inbox" ]]; then
  printf '%s\\n' '[{"id":"mmsg-1","from_member_id":"member-run-greeter","kind":"message","correlation_id":"corr-9","body":"hello external member","deliveries":[{"member_id":"member-run-ext","status":"queued"}]}]'
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

  const memberEnv = {
    HARNESS_HOST_SURFACE: "codex-app",
    HARNESS_TEAM_RUN_ID: "run-1",
    HARNESS_MEMBER_RUN_ID: "member-run-ext",
  };

  const extStarted = run("SessionStart", memberEnv);
  if (
    !extStarted.includes(
      "External member binding: team_run=run-1 member_run=member-run-ext",
    ) ||
    !extStarted.includes("Member mail: TeamRun=run-1 pending_member_messages=1") ||
    !extStarted.includes("from=member-run-greeter kind=message message=mmsg-1")
  ) {
    throw new Error(
      `External member SessionStart must inject bound member mail: ${extStarted}`,
    );
  }

  const extPrompt = run("UserPromptSubmit", memberEnv);
  if (extPrompt.includes("External member binding:")) {
    throw new Error("UserPromptSubmit should not repeat member binding orientation");
  }
  if (!extPrompt.includes("Member mail: TeamRun=run-1")) {
    throw new Error("UserPromptSubmit must surface actionable member mail");
  }

  const extStopped = JSON.parse(
    run("Stop", memberEnv, { turn_id: "turn-9", stop_hook_active: false }),
  );
  if (Object.keys(extStopped).length !== 0) {
    throw new Error(
      "External Codex Stop must remain user-driven by default",
    );
  }

  const extOptedIn = JSON.parse(
    run(
      "Stop",
      { ...memberEnv, HARNESS_EXTERNAL_AUTO_CONTINUE: "1" },
      { turn_id: "turn-10", stop_hook_active: false },
    ),
  );
  if (
    extOptedIn.decision !== "block" ||
    !extOptedIn.reason.includes("Member Inbox") ||
    !extOptedIn.reason.includes("mmsg-1") ||
    !extOptedIn.reason.includes("--member-id member-run-ext")
  ) {
    throw new Error("Opted-in external Codex Stop must continue with bounded mail");
  }

  const extContinued = JSON.parse(
    run(
      "Stop",
      { ...memberEnv, HARNESS_EXTERNAL_AUTO_CONTINUE: "true" },
      { turn_id: "turn-11", stop_hook_active: true },
    ),
  );
  if (Object.keys(extContinued).length !== 0) {
    throw new Error("A continued member Stop hook must not create a loop");
  }

  const extKimiStopped = runRaw(
    "Stop",
    {
      KIMI_PLUGIN_ROOT: temp,
      HARNESS_TEAM_RUN_ID: "run-1",
      HARNESS_MEMBER_RUN_ID: "member-run-ext",
    },
    { stop_hook_active: false },
  );
  if (extKimiStopped.status !== 0 || extKimiStopped.stderr.trim()) {
    throw new Error(
      `External Kimi Stop must remain user-driven by default; status=${extKimiStopped.status} stderr=${extKimiStopped.stderr}`,
    );
  }

  const extKimiOptedIn = runRaw(
    "Stop",
    {
      KIMI_PLUGIN_ROOT: temp,
      HARNESS_TEAM_RUN_ID: "run-1",
      HARNESS_MEMBER_RUN_ID: "member-run-ext",
      HARNESS_EXTERNAL_AUTO_CONTINUE: "yes",
    },
    { stop_hook_active: false },
  );
  if (
    extKimiOptedIn.status !== 2 ||
    !extKimiOptedIn.stderr.includes("mmsg-1")
  ) {
    throw new Error(
      `Opted-in external Kimi Stop must block through exit 2; status=${extKimiOptedIn.status} stderr=${extKimiOptedIn.stderr}`,
    );
  }

  const drivenBinding = run("SessionStart", {
    HARNESS_TEAM_RUN_ID: "run-1",
    HARNESS_MEMBER_RUN_ID: "member-run-driven",
  });
  if (drivenBinding.trim()) {
    throw new Error("An unverified driven MemberRun binding must not intake its Inbox");
  }

  const stoppedBinding = run("SessionStart", {
    HARNESS_TEAM_RUN_ID: "run-1",
    HARNESS_MEMBER_RUN_ID: "member-run-stopped",
  });
  if (stoppedBinding.trim()) {
    throw new Error("A closed external MemberRun binding must not intake mail");
  }

  const drivenPrecedence = run("SessionStart", {
    HARNESS_AGENT_MEMBER_ID: "member-driven",
    HARNESS_TEAM_RUN_ID: "run-1",
    HARNESS_MEMBER_RUN_ID: "member-run-ext",
  });
  if (drivenPrecedence.trim()) {
    throw new Error(
      "A driven Member (HARNESS_AGENT_MEMBER_ID) must stay telemetry-only even when member inbox env leaks in",
    );
  }

  console.log("Star Harness Codex, Claude, and Kimi Host Inbox hooks are valid");
} finally {
  rmSync(temp, { recursive: true, force: true });
}
