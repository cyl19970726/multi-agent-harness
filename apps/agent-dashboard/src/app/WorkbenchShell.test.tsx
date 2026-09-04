import { describe, expect, it } from "vitest";

import type { DashboardSnapshot } from "../types";
import { snapshotContentRevision } from "./WorkbenchShell";

describe("snapshotContentRevision", () => {
  it("does not advance for lease heartbeat-only snapshot changes", () => {
    const snapshot = {
      generated_at: "unix-ms:1",
      node_daemon_leases: [{ renewed_unix_ms: 1, expires_unix_ms: 2 }],
      team_supervisor_leases: [{ renewed_unix_ms: 1, expires_unix_ms: 2 }],
      live_member_activity: { member: { updated_at: "unix-ms:1" } },
      works: [{ id: "work-1", version: 3 }],
    } as unknown as DashboardSnapshot;
    const heartbeat = {
      ...snapshot,
      generated_at: "unix-ms:10",
      node_daemon_leases: [{ renewed_unix_ms: 10, expires_unix_ms: 11 }],
      team_supervisor_leases: [{ renewed_unix_ms: 10, expires_unix_ms: 11 }],
      live_member_activity: { member: { updated_at: "unix-ms:10" } },
    } as unknown as DashboardSnapshot;

    expect(snapshotContentRevision(heartbeat)).toBe(snapshotContentRevision(snapshot));
    expect(snapshotContentRevision({ ...snapshot, works: [{ id: "work-1", version: 4 }] } as unknown as DashboardSnapshot))
      .not.toBe(snapshotContentRevision(snapshot));
  });
});
