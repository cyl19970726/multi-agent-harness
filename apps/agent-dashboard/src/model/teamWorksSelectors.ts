import type { DashboardSnapshot, Work } from "../types";
import { organizationMembersById } from "./orgSelectors";

/**
 * Company-wide Work is a read projection over native Team Work. Delegation is
 * explicit provenance from WorkDelegation; no child-Team or naming inference.
 */

export type TeamWorkDemandClass = "unassigned" | "delegated" | "follow-up" | "owned";

export interface TeamWorkRow {
  work: Work;
  demandClass: TeamWorkDemandClass;
  /** Owning Team label via run.agent_team_id → teams; honest fallbacks only. */
  teamLabel: string;
  /** Proven root-to-team label path; never inferred from run or member names. */
  teamPath: string;
  teamId?: string;
  missionId?: string;
  nodeId?: string;
  runId: string;
  runStatus?: string;
  hostId?: string;
  hostLabel?: string;
  /** Responsible member label from `owner_member_id` → members; id fallback. */
  ownerLabel?: string;
  /** Source observation from the canonical creator actor. */
  sourceLabel: string;
  parentWorkId?: string | null;
  /** Durable team scope from Work.team_id, falling back to run.agent_team_id. */
  durableTeamId?: string;
  delegationId?: string;
}

export interface TeamWorksFacets {
  teams: Array<{ id: string; label: string }>;
  missions: Array<{ id: string; label: string }>;
  nodes: Array<{ id: string; label: string }>;
  hosts: Array<{ id: string; label: string }>;
  members: Array<{ id: string; label: string }>;
  statuses: string[];
  priorities: string[];
  sources: Array<{ id: string; label: string }>;
}

export interface TeamWorksModel {
  rows: TeamWorkRow[];
  counts: { unassigned: number; delegated: number; followUp: number; owned: number; total: number };
  facets: TeamWorksFacets;
  /**
   * True when every visible row belongs to one TeamRun — happens when the app
   * fetched the run-scoped snapshot (a team is selected). The view must label
   * this instead of presenting a partial list as the global aggregate.
   */
  scopedToSingleRun: boolean;
  singleRunId?: string;
}

export interface TeamWorksFilters {
  teamId?: string;
  missionId?: string;
  nodeId?: string;
  hostId?: string;
  memberId?: string;
  status?: string;
  priority?: string;
  source?: string;
  demand?: TeamWorkDemandClass;
}

const STATUS_ORDER = ["open", "active", "blocked", "on_hold", "review", "accepted", "failed", "cancelled"];
const PRIORITY_ORDER = ["urgent", "high", "normal", "low"];

function workLifecycleLabel(work: Work): string {
  if (work.condition !== "normal") return work.condition;
  if (work.phase === "closed") return work.resolution ?? "closed";
  return work.phase;
}

export function buildTeamWorksModel(snapshot: DashboardSnapshot): TeamWorksModel {
  const works = snapshot.works ?? [];
  const runsById = new Map((snapshot.team_runs ?? []).map((run) => [run.id, run]));
  const teamsById = new Map((snapshot.teams ?? []).map((team) => [team.id, team]));
  const membersById = organizationMembersById(snapshot);
  const missionsById = new Map((snapshot.missions ?? []).map((mission) => [mission.id, mission]));
  const nodesById = new Map((snapshot.execution_nodes ?? []).map((node) => [node.id, node]));
  const delegationByTarget = new Map(
    (snapshot.work_delegations ?? []).map((delegation) => [
      `${delegation.target_work_ref.team_run_id}:${delegation.target_work_ref.work_id}`,
      delegation,
    ]),
  );

  const rows: TeamWorkRow[] = works.map((work) => {
    const run = runsById.get(work.team_run_id);
    const team = run?.agent_team_id ? teamsById.get(run.agent_team_id) : undefined;
    const delegation = delegationByTarget.get(`${work.team_run_id}:${work.id}`);
    const demandClass: TeamWorkDemandClass =
      delegation
        ? "delegated"
        : work.phase === "open" && work.condition === "normal" && !work.owner_member_id
        ? "unassigned"
        : work.parent_work_id
          ? "follow-up"
          : "owned";
    const ownerLabel = work.owner_member_id
      ? (membersById.get(work.owner_member_id)?.name ?? work.owner_member_id)
      : undefined;
    return {
      work,
      demandClass,
      teamLabel: team?.name ?? team?.id ?? `TeamRun ${work.team_run_id}`,
      teamPath: team?.name ?? team?.id ?? "Team unavailable",
      teamId: team?.id,
      missionId: team?.mission_id,
      nodeId: team?.node_id,
      runId: work.team_run_id,
      runStatus: run?.status,
      hostId: team?.host_agent_id,
      hostLabel: team?.host_agent_id
        ? (membersById.get(team.host_agent_id)?.name ?? team.host_agent_id)
        : undefined,
      ownerLabel,
      sourceLabel: delegation ? `delegated from ${delegation.source_work_ref.work_id}` : `${work.created_by_actor?.kind ?? "unknown"} intake`,
      parentWorkId: work.parent_work_id ?? null,
      durableTeamId: work.team_id ?? team?.id,
      delegationId: delegation?.id,
    };
  });

  rows.sort((a, b) => (b.work.updated_at ?? "").localeCompare(a.work.updated_at ?? ""));

  const runIds = new Set(works.map((work) => work.team_run_id));
  const scopedToSingleRun = runIds.size === 1 && works.length > 0;

  const uniq = <T extends { id: string; label: string }>(items: T[]): T[] => {
    const seen = new Map<string, T>();
    for (const item of items) if (!seen.has(item.id)) seen.set(item.id, item);
    return [...seen.values()].sort((a, b) => a.label.localeCompare(b.label));
  };

  const facets: TeamWorksFacets = {
    teams: uniq(
      rows
        .filter((row) => row.teamId)
        .map((row) => ({ id: row.teamId as string, label: row.teamLabel })),
    ),
    missions: uniq(
      rows.filter((row) => row.missionId).map((row) => ({
        id: row.missionId as string,
        label: missionsById.get(row.missionId as string)?.title ?? (row.missionId as string),
      })),
    ),
    nodes: uniq(
      rows.filter((row) => row.nodeId).map((row) => ({
        id: row.nodeId as string,
        label: nodesById.get(row.nodeId as string)?.display_name ?? (row.nodeId as string),
      })),
    ),
    hosts: uniq(
      rows
        .map((row) => row.hostId ? { id: row.hostId, label: row.hostLabel ?? row.hostId } : undefined)
        .filter((item): item is { id: string; label: string } => Boolean(item)),
    ),
    members: uniq(
      rows
        .filter((row) => row.work.owner_member_id)
        .map((row) => ({
          id: row.work.owner_member_id as string,
          label: row.ownerLabel ?? (row.work.owner_member_id as string),
        })),
    ),
    statuses: STATUS_ORDER.filter((status) => rows.some((row) => workLifecycleLabel(row.work) === status)),
    priorities: PRIORITY_ORDER.filter((p) => rows.some((row) => row.work.priority === p)),
    sources: uniq(
      rows.map((row) => ({
        id: row.work.created_by_actor?.kind ?? "unknown",
        label: row.sourceLabel,
      })),
    ),
  };

  return {
    rows,
    counts: {
      unassigned: rows.filter((row) => row.demandClass === "unassigned").length,
      delegated: rows.filter((row) => row.demandClass === "delegated").length,
      followUp: rows.filter((row) => row.demandClass === "follow-up").length,
      owned: rows.filter((row) => row.demandClass === "owned").length,
      total: rows.length,
    },
    facets,
    scopedToSingleRun,
    singleRunId: scopedToSingleRun ? [...runIds][0] : undefined,
  };
}

/** AND-composed filtering over the aggregate; an empty filter set returns all rows. */
export function filterTeamWorks(rows: TeamWorkRow[], filters: TeamWorksFilters): TeamWorkRow[] {
  return rows.filter((row) => {
    if (filters.teamId && row.teamId !== filters.teamId) return false;
    if (filters.missionId && row.missionId !== filters.missionId) return false;
    if (filters.nodeId && row.nodeId !== filters.nodeId) return false;
    if (filters.hostId && row.hostId !== filters.hostId) return false;
    if (filters.memberId && row.work.owner_member_id !== filters.memberId) return false;
    if (filters.status && workLifecycleLabel(row.work) !== filters.status) return false;
    if (filters.priority && row.work.priority !== filters.priority) return false;
    if (filters.source) {
      const sourceId = row.work.created_by_actor?.kind ?? "unknown";
      if (sourceId !== filters.source) return false;
    }
    if (filters.demand && row.demandClass !== filters.demand) return false;
    return true;
  });
}
