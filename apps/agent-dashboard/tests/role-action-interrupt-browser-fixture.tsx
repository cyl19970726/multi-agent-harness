import React from "react";
import { createRoot } from "react-dom/client";

import { RoleActionPanel } from "../src/surfaces/RoleActionPanel";
import type { RoleActionExecutionResult } from "../src/model/roleViews";

declare global {
  interface Window {
    __interruptActionCalls: Array<{
      path: string;
      body: unknown;
      headers: Readonly<Record<string, string>>;
    }>;
    __interruptActionResult: RoleActionExecutionResult;
    __interruptCompleted: number;
  }
}

window.__interruptActionCalls = [];
window.__interruptActionResult = { ok: true };
window.__interruptCompleted = 0;

createRoot(document.getElementById("root")!).render(
  <RoleActionPanel
    actions={[
      {
        kind: "interrupt_member_run",
        target_ref: { kind: "member_run", id: "member/run one" },
        required_version: 7,
        disabled_reason: null,
      },
    ]}
    context={{ teamRunId: "team-run-fixture" }}
    onAction={async (path, body, options) => {
      window.__interruptActionCalls.push({
        path,
        body,
        headers: options?.headers ?? {},
      });
      return window.__interruptActionResult;
    }}
    onCompleted={() => {
      window.__interruptCompleted += 1;
    }}
  />,
);
