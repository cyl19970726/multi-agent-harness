import { fetchSnapshot, fetchTeamRunSnapshot } from "../api";
import type { SelectionState } from "./selection";

/** ID prefixes are only a fast path for historical links, never Team identity. */
export async function readSelectionSnapshot(
  selection: Pick<SelectionState, "surface" | "teamId">,
  baseUrl: string, project: string, company: string, space: string, signal: AbortSignal,
) {
  const full = () => fetchSnapshot(baseUrl, project, company, space, signal);
  const bounded = (id: string) => fetchTeamRunSnapshot(baseUrl, id, project, company, space, signal);
  if (selection.surface !== "team" || !selection.teamId) return full();
  const id = selection.teamId;
  if (id.startsWith("team-run-")) {
    try {
      const snapshot = await bounded(id);
      if (snapshot.team_runs?.some(run => run.id === id)) return snapshot;
      throw new Error("TeamRun snapshot did not contain the requested run");
    } catch (error) {
      // A durable Team may itself use this prefix. Only not-found permits
      // relation-based resolution; transport and authorization failures surface.
      if (!(error instanceof Error) || error.message !== "HTTP 404") throw error;
    }
  }
  const snapshot = await full();
  if (snapshot.teams?.some(team => team.id === id)) {
    const latest = (snapshot.team_runs ?? []).filter(run => run.agent_team_id === id)
      .sort((a, b) => timestamp(b.created_at) - timestamp(a.created_at))[0];
    return latest ? bounded(latest.id) : snapshot;
  }
  // Nonstandard historical Run IDs also resolve from authoritative relations.
  return snapshot.team_runs?.some(run => run.id === id) ? bounded(id) : snapshot;
}

function timestamp(value: string | undefined) {
  const parsed = value?.startsWith("unix-ms:") ? Number(value.slice(8)) : Date.parse(value ?? "");
  return Number.isFinite(parsed) ? parsed : 0;
}
