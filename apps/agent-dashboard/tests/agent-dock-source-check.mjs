#!/usr/bin/env node
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "../src/components/workbench/agent/dock");
const read = (file) => readFileSync(join(root, file), "utf8");
const shell = read("DockShell.tsx"), work = read("WorkDock.tsx"), messages = read("MessagesDock.tsx"), controller = read("useAgentDockController.ts");
const workspace=readFileSync(join(root,"../../../../surfaces/AgentConversationWorkspace.tsx"),"utf8");
assert.match(shell, /role="separator"/);
assert.match(shell, /aria-valuenow/);
assert.match(shell, /displayMode === "overlay"/);
assert.match(shell, /event\.key === "Escape"/);
assert.match(controller, /localStorage\.setItem/);
assert.match(work, /Outcome|Current outcome/);
for (const heading of ["Objective", "Acceptance", "Result", "Evidence", "Review", "History"]) assert.ok(work.includes(`title=\"${heading}\"`), `Work detail omits ${heading}`);
for (const lens of ["priority", "inbox", "outbox", "all"]) assert.ok(messages.includes(`\"${lens}\"`), `Messages omits ${lens}`);
assert.match(messages, /incoming\(message\) && message\.deliveries/);
assert.match(messages, /does not mutate Work, prove a Result, or grant acceptance/);
assert.match(messages, /correlation:/);
assert.match(work, /most recently closed Work/);
assert.doesNotMatch(shell + work + messages, /SessionTimeline|ProviderEvent|native_event/);
assert.match(workspace,/agent-workspace-sessionbar/);
assert.match(workspace,/<DockShell/);
assert.match(workspace,/onOpenMessage/);
assert.doesNotMatch(workspace,/<Tabs\.Root|WorkspaceTab value="messages"|WorkspaceTab value="work"/);
console.log("agent dock source check passed");
