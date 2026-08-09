import type { DashboardSnapshot, Work } from "../types";
import { organizationMembersById } from "./orgSelectors";

/**
 * Global "Team Works" aggregate: one cross-TeamRun projection over native
 * Team Work rows, derived only from real snapshot fields (`works`,
 * `team_runs`, `teams`, `members`). It never mixes a second Company task ledger with
 * Team Work and never fabricates an owner, source, or milestone.
 *
 * Demand classes follow the recursive-Work discovery contract
 * (docs/company-os/nested-agent-team-organization.md): discovered-unassigned,
 * self-owned (owned here, since the global viewer is the operator), and
 * follow-up. The delegated class is intentionally omitted: `WorkDelegation`
 * (harness-core) is not yet projected into the dashboard snapshot, so no
 * honest row-level derivation exists — tracked as a follow-up Work.
 *
 * Contract: docs/design/company-os-v6/recursive-org-docs-works-v1.
 */

export type TeamWorkDemandClass = "unassigned" | "follow-up" | "owned";

export interface TeamWorkRow {
  work: Work;
  demandClass: TeamWorkDemandClass;
  /** Owning Team label via run.agent_team_id → teams; honest fallbacks only. */
  teamLabel: string;
  /** Proven root-to-team label path; never inferred from run or member names. */
  teamPath: string;
  teamId?: string;
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
}

export interface TeamWorksFacets {
  teams: Array<{ id: string; label: string }>;
  hosts: Array<{ id: string; label: string }>;
  members: Array<{ id: string; label: string }>;
  statuses: string[];
  priorities: string[];
  sources: Array<{ id: string; label: string }>;
}

export interface TeamWorksModel {
  rows: TeamWorkRow[];
  counts: { unassigned: number; followUp: number; owned: number; total: number };
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
  const teamPath = (teamId?: string): string => {
    if (!teamId) return "Team unavailable";
    const labels: string[] = [];
    const seen = new Set<string>();
    let cursor = teamsById.get(teamId);
    while (cursor && !seen.has(cursor.id)) {
      seen.add(cursor.id);
      labels.unshift(cursor.name ?? cursor.id);
      cursor = cursor.parent_team_id ? teamsById.get(cursor.parent_team_id) : undefined;
    }
    return labels.join(" / ") || teamId;
  };

  const rows: TeamWorkRow[] = works.map((work) => {
    const run = runsById.get(work.team_run_id);
    const team = run?.agent_team_id ? teamsById.get(run.agent_team_id) : undefined;
    const demandClass: TeamWorkDemandClass =
      work.phase === "open" && work.condition === "normal" && !work.owner_member_id
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
      teamPath: teamPath(team?.id),
      teamId: team?.id,
      runId: work.team_run_id,
      runStatus: run?.status,
      // Durable Team authority wins. TeamRun.host_actor is execution-attempt
      // metadata and remains only as a compatibility fallback for old Teams.
      hostId: team?.host_member_id ?? run?.host_actor?.id ?? undefined,
      hostLabel: team?.host_member_id
        ? (membersById.get(team.host_member_id)?.name ?? team.host_member_id)
        : (run?.host_actor?.display_name ?? run?.host_actor?.id ?? undefined),
      ownerLabel,
      sourceLabel: `${work.created_by_actor?.kind ?? "unknown"} intake`,
      parentWorkId: work.parent_work_id ?? null,
      durableTeamId: work.team_id ?? team?.id,
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
