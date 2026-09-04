import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";

import { AgentFirmApiError } from "@/api";
import { TeamInboxLoadState } from "./TeamInboxPanel";

describe("TeamInboxLoadState", () => {
  it("renders the identity-required state as a quiet read-only affordance", () => {
    const markup = renderToStaticMarkup(
      <TeamInboxLoadState
        loading={false}
        error={new AgentFirmApiError(
          403,
          "NOT_AUTHORIZED",
          "TeamInbox requires a Team-scoped AgentMember identity",
        )}
      >
        <p>Inbox contents</p>
      </TeamInboxLoadState>,
    );

    expect(markup).toContain("Sign in as the Team Host to read the Team Inbox");
    expect(markup).toContain("text-muted-foreground");
    expect(markup).not.toContain('role="alert"');
    expect(markup).not.toContain("destructive");
  });

  it("keeps genuine request failures in the error callout", () => {
    const markup = renderToStaticMarkup(
      <TeamInboxLoadState loading={false} error={new Error("network unavailable")}>
        <p>Inbox contents</p>
      </TeamInboxLoadState>,
    );

    expect(markup).toContain('role="alert"');
    expect(markup).toContain("Team Inbox is unavailable");
    expect(markup).toContain("Error: network unavailable");
    expect(markup).toContain("destructive");
    expect(markup).not.toContain("Sign in as the Team Host");
  });
});
