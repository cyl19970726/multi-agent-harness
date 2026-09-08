import { beforeEach, describe, expect, it, vi } from "vitest";
import { fetchSnapshot, fetchTeamRunSnapshot } from "../api";
import type { DashboardSnapshot } from "../types";
import { readSelectionSnapshot } from "./readSnapshot";
vi.mock("../api", () => ({fetchSnapshot: vi.fn(), fetchTeamRunSnapshot: vi.fn()}));
const signal = new AbortController().signal;
const read = (teamId?:string, surface:"team"|"agents"="team") => readSelectionSnapshot({surface,teamId}, "url", "project", "company", "space", signal);
const snapshot = (teams:unknown[]=[], runs:unknown[]=[]) => ({teams,team_runs:runs}) as DashboardSnapshot;
beforeEach(() => vi.resetAllMocks());
describe("selection snapshot scope", () => {
  it("resolves an arbitrary durable Team ID to its latest related run", async () => {
    vi.mocked(fetchSnapshot).mockResolvedValue(snapshot([{id:"g900-managed"}], [{id:"new",agent_team_id:"g900-managed",created_at:"unix-ms:1000"},{id:"old",agent_team_id:"g900-managed",created_at:"unix-ms:999"},{id:"other",agent_team_id:"other",created_at:"unix-ms:2000"}]));
    const scoped = snapshot([], [{id:"new"}]);
    vi.mocked(fetchTeamRunSnapshot).mockResolvedValue(scoped);
    expect(await read("g900-managed")).toBe(scoped);
    expect(fetchTeamRunSnapshot).toHaveBeenCalledTimes(1);
    expect(fetchTeamRunSnapshot).toHaveBeenCalledWith("url","new","project","company","space",signal);
  });
  it("keeps historical run deep links bounded with zero global reads", async () => {
    const scoped = snapshot([], [{id:"team-run-old"}]);
    vi.mocked(fetchTeamRunSnapshot).mockResolvedValue(scoped);
    expect(await read("team-run-old")).toBe(scoped);
    expect(fetchSnapshot).not.toHaveBeenCalled();
  });
  it("returns a no-run Team snapshot once without inventing a run", async () => {
    const full = snapshot([{id:"g900-empty"}]);
    vi.mocked(fetchSnapshot).mockResolvedValue(full);
    expect(await read("g900-empty")).toBe(full);
    expect(fetchSnapshot).toHaveBeenCalledTimes(1);
    expect(fetchTeamRunSnapshot).not.toHaveBeenCalled();
  });
  it("handles durable IDs with the historical prefix using recorded relations after 404", async () => {
    vi.mocked(fetchTeamRunSnapshot).mockRejectedValueOnce(new Error("HTTP 404"));
    const full = snapshot([{id:"team-run-durable"}]);
    vi.mocked(fetchSnapshot).mockResolvedValue(full);
    expect(await read("team-run-durable")).toBe(full);
  });
  it("resolves nonstandard historical run IDs", async () => {
    vi.mocked(fetchSnapshot).mockResolvedValue(snapshot([], [{id:"old-custom-run"}]));
    await read("old-custom-run");
    expect(fetchTeamRunSnapshot).toHaveBeenCalledWith("url","old-custom-run","project","company","space",signal);
  });
  it("preserves ordinary full reads outside Team pages", async () => {
    await read("team-run-old", "agents");
    expect(fetchSnapshot).toHaveBeenCalledTimes(1);
    expect(fetchTeamRunSnapshot).not.toHaveBeenCalled();
  });
  it("does not disguise a failed bounded read as a full snapshot", async () => {
    vi.mocked(fetchTeamRunSnapshot).mockRejectedValue(new Error("HTTP 403"));
    await expect(read("team-run-old")).rejects.toThrow("HTTP 403");
    expect(fetchSnapshot).not.toHaveBeenCalled();
  });
});
