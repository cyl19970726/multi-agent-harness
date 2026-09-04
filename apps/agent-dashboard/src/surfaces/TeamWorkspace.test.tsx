import { act, create, type ReactTestRenderer } from "react-test-renderer";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { RoleView, TeamWorkspaceData, ViewerContextData } from "../model/roleViews";

const fetchRoleViewMock = vi.hoisted(() => vi.fn());

vi.mock("../model/roleViews", async (importOriginal) => ({
  ...await importOriginal<typeof import("../model/roleViews")>(),
  fetchRoleView: fetchRoleViewMock,
}));
vi.mock("@/components/workbench/team/TeamCapacityStrip", () => ({ TeamCapacityStrip: () => null }));
vi.mock("@/components/workbench/team/TeamConversation", () => ({ TeamConversationStream: () => null }));
vi.mock("@/components/workbench/team/TeamMembersCapacity", () => ({ TeamMembersCapacity: () => null }));
vi.mock("@/components/workbench/team/TeamWorksBoard", () => ({ TeamWorksBoard: () => <p>Committed Work view</p> }));
vi.mock("@/components/workbench/team/AgentTeamVisualPrimitives", () => ({
  AgentTeamTabs: ({ children }: { children: React.ReactNode }) => <div>{children}</div>,
  AgentTeamTab: ({ children }: { children: React.ReactNode }) => <button>{children}</button>,
}));

import { TeamWorkspace } from "./TeamWorkspace";

const envelope = {
  schema_version: "agentfirm.role_views.v1",
  source_execution_space_id: "space-1",
  source_store_identity: "store-1",
  as_of_event_sequence: 1,
  freshness: "current",
  allowed_actions: [],
  attention: [],
} as const;

const viewerContext = {
  ...envelope,
  view_kind: "viewer_context",
  data: {
    viewer_actor_ref: { kind: "agent_member", id: "host-1" },
    teams: [{
      team_id: "team-1",
      display_name: "Team One",
      viewer_role: "host",
      viewer_agent_member_id: "host-1",
      default_conversation: "host",
      latest_run_id: "run-1",
      team_run_ids: ["run-1"],
      current_member_run_id: "member-run-1",
    }],
  },
} as unknown as RoleView<ViewerContextData>;

const workspace = {
  ...envelope,
  view_kind: "team_workspace",
  data: {
    team: {
      team_id: "team-1", display_name: "Team One", team_revision: 1,
      mission_id: "", host_agent_id: "host-1", viewer_role: "host",
      node_id: "node-1", placement_generation: 1, status: "active", latest_run: null,
    },
    works: [], work_graph: { nodes: [], edges: [], ready_work_ids: [], attention_work_ids: [] },
    members: [], messages: [], activity: [], activity_truncated: false,
    pressure_summary: { active_turns: 0, ready_members: 0, total_members: 0, ready_work: 0, review_work: 0, blocked_work: 0 },
    reports: [], findings: [], failures: [], gate_requirements: [], gate_evaluations: [],
    gate_waivers: [], workspace_attention: [], delegation_provenance: [], collaboration: {},
    page: { as_of_event_sequence: 1, item_count: 0, next_cursor: null }, runtime_fabric: {},
  },
} as unknown as RoleView<TeamWorkspaceData>;

const flushPromises = async () => {
  await Promise.resolve();
  await Promise.resolve();
  await Promise.resolve();
};

afterEach(() => {
  fetchRoleViewMock.mockReset();
});

describe("TeamWorkspace revalidation", () => {
  it("keeps a committed view and hides Refreshing status when refreshKey changes", async () => {
    let workspaceRequests = 0;
    fetchRoleViewMock.mockImplementation((_apiUrl: string, path: string) => {
      if (path.includes("viewer-context")) return Promise.resolve(viewerContext);
      workspaceRequests += 1;
      return workspaceRequests === 1 ? Promise.resolve(workspace) : new Promise(() => undefined);
    });
    vi.stubGlobal("window", {
      setTimeout: globalThis.setTimeout.bind(globalThis),
      clearTimeout: globalThis.clearTimeout.bind(globalThis),
      requestAnimationFrame: (callback: FrameRequestCallback) => { callback(0); return 1; },
    });
    const props = (refreshKey: string) => ({
      apiUrl: "http://example.test", space: "space-1", project: "project-1", teamId: "team-1",
      refreshKey, selection: { surface: "team" as const, teamId: "team-1", teamTab: "works" as const },
      onAction: vi.fn(), onSelectionChange: vi.fn(), onSelectionReplace: vi.fn(),
    });
    let renderer: ReactTestRenderer;
    await act(async () => {
      renderer = create(<TeamWorkspace {...props("revision-1")} />);
      await flushPromises();
    });
    expect(JSON.stringify(renderer!.toJSON())).toContain("Committed Work view");

    await act(async () => {
      renderer!.update(<TeamWorkspace {...props("revision-2")} />);
      await new Promise((resolve) => globalThis.setTimeout(resolve, 550));
      await flushPromises();
    });

    const markup = JSON.stringify(renderer!.toJSON());
    expect(workspaceRequests).toBe(2);
    expect(markup).toContain("Committed Work view");
    expect(markup).not.toContain("Refreshing authenticated TeamWorkspace");
    await act(async () => {
      renderer!.unmount();
    });
    vi.unstubAllGlobals();
  });

  it("restores loading behavior when the viewer identity changes", async () => {
    const hostContext = {
      ...viewerContext,
      data: {
        ...viewerContext.data,
        viewer_actor_ref: { kind: "agent_member", id: "host-2" },
        teams: [{ ...viewerContext.data.teams[0], viewer_agent_member_id: "host-2" }],
      },
    } as RoleView<ViewerContextData>;
    let viewerRequests = 0;
    let workspaceRequests = 0;
    fetchRoleViewMock.mockImplementation((_apiUrl: string, path: string) => {
      if (path.includes("viewer-context")) {
        viewerRequests += 1;
        return Promise.resolve(viewerRequests === 1 ? viewerContext : hostContext);
      }
      workspaceRequests += 1;
      return workspaceRequests === 1 ? Promise.resolve(workspace) : new Promise(() => undefined);
    });
    vi.stubGlobal("window", {
      setTimeout: globalThis.setTimeout.bind(globalThis),
      clearTimeout: globalThis.clearTimeout.bind(globalThis),
      requestAnimationFrame: (callback: FrameRequestCallback) => { callback(0); return 1; },
    });
    const props = (refreshKey: string) => ({
      apiUrl: "http://example.test", space: "space-1", project: "project-1", teamId: "team-1",
      refreshKey, selection: { surface: "team" as const, teamId: "team-1", teamTab: "works" as const },
      onAction: vi.fn(), onSelectionChange: vi.fn(), onSelectionReplace: vi.fn(),
    });
    let renderer: ReactTestRenderer;
    await act(async () => {
      renderer = create(<TeamWorkspace {...props("revision-1")} />);
      await flushPromises();
    });
    await act(async () => {
      renderer!.update(<TeamWorkspace {...props("revision-2")} />);
      await new Promise((resolve) => globalThis.setTimeout(resolve, 550));
      await flushPromises();
    });

    const markup = JSON.stringify(renderer!.toJSON());
    expect(markup).toContain("Loading Agent Team · team-1");
    expect(markup).not.toContain("Committed Work view");
    await act(async () => {
      renderer!.unmount();
    });
    vi.unstubAllGlobals();
  });
});
