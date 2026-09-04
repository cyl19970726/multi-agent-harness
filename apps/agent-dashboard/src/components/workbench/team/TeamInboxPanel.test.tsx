import { renderToStaticMarkup } from "react-dom/server";
import { act, create, type ReactTestRenderer } from "react-test-renderer";
import { afterEach, describe, expect, it, vi } from "vitest";

import { AgentFirmApiError } from "@/api";

const fetchRoleViewMock = vi.hoisted(() => vi.fn());

vi.mock("../../../model/roleViews", async (importOriginal) => ({
  ...await importOriginal<typeof import("../../../model/roleViews")>(),
  fetchRoleView: fetchRoleViewMock,
}));

import { TeamInboxLoadState, TeamInboxPanel } from "./TeamInboxPanel";

const flushPromises = async () => {
  await Promise.resolve();
  await Promise.resolve();
  await Promise.resolve();
};

afterEach(() => {
  fetchRoleViewMock.mockReset();
});

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

describe("TeamInboxPanel revalidation", () => {
  const props = (refreshKey: string, viewerIdentity = "operator\u0000local") => ({
    apiUrl: "http://example.test", space: "space-1", project: "project-1",
    teamId: "team-1", viewerIdentity, refreshKey,
  });

  it("does not re-request after identity-required until viewer identity changes", async () => {
    fetchRoleViewMock.mockRejectedValue(new AgentFirmApiError(
      403,
      "NOT_AUTHORIZED",
      "TeamInbox requires a Team-scoped AgentMember identity",
    ));
    let renderer: ReactTestRenderer;
    await act(async () => {
      renderer = create(<TeamInboxPanel {...props("revision-1")} />);
      await flushPromises();
    });
    expect(fetchRoleViewMock).toHaveBeenCalledTimes(1);
    expect(JSON.stringify(renderer!.toJSON())).toContain("Sign in as the Team Host");

    await act(async () => {
      renderer!.update(<TeamInboxPanel {...props("revision-2")} />);
      await flushPromises();
    });
    expect(fetchRoleViewMock).toHaveBeenCalledTimes(1);

    await act(async () => {
      renderer!.update(<TeamInboxPanel {...props("revision-3", "agent_member\u0000host-1")} />);
      await flushPromises();
    });
    expect(fetchRoleViewMock).toHaveBeenCalledTimes(2);
    renderer!.unmount();
  });

  it("keeps a committed inbox visible during background revalidation", async () => {
    fetchRoleViewMock
      .mockResolvedValueOnce({
        schema_version: "agentfirm.role_views.v1", view_kind: "team_inbox",
        source_execution_space_id: "space-1", source_store_identity: "store-1",
        as_of_event_sequence: 1, freshness: "current", allowed_actions: [], attention: [],
        data: { items: [] },
      })
      .mockImplementation(() => new Promise(() => undefined));
    let renderer: ReactTestRenderer;
    await act(async () => {
      renderer = create(<TeamInboxPanel {...props("revision-1", "agent_member\u0000host-1")} />);
      await flushPromises();
    });
    expect(JSON.stringify(renderer!.toJSON())).toContain("No Team-addressed deliveries");

    await act(async () => {
      renderer!.update(<TeamInboxPanel {...props("revision-2", "agent_member\u0000host-1")} />);
      await flushPromises();
    });
    const markup = JSON.stringify(renderer!.toJSON());
    expect(markup).toContain("No Team-addressed deliveries");
    expect(markup).not.toContain("Loading Team Inbox");
    renderer!.unmount();
  });
});
