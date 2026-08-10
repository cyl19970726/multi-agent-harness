import type {
  AgentTeam,
  DashboardSnapshot,
  DurableAgentMember,
  MemberRun,
  TeamRun,
  Work,
} from "../types";

/** Flat Organization projection: Company owns peer AgentTeams; each Team is
 * one Mission on one Node. Cross-Team structure is WorkDelegation, never a
 * parent/child Team edge. */
export interface OrgWorkCounts {
  assigned: number;
  unassigned: number;
  inProgress: number;
  blocked: number;
  review: number;
}

export interface OrgRuntimeSummary {
  running: number;
  total: number;
}

export interface OrgMemberIdentity {
  id: string;
  name?: string;
  description?: string;
  role?: string;
  status?: string;
  identitySource: "durable" | "compatibility";
}

export interface OrgTeamNode {
  team: AgentTeam;
  depth: 0;
  parentId: null;
  childTeamIds: [];
  members: OrgMemberIdentity[];
  host?: OrgMemberIdentity;
  compatLeadLabel?: string;
  workCounts: OrgWorkCounts;
  runtime: OrgRuntimeSummary;
  latestRunId?: string;
  findings: string[];
}

export interface AgentTeamOrgModel {
  roots: OrgTeamNode[];
  nodesById: Map<string, OrgTeamNode>;
  findings: string[];
  hasTopologyEdges: false;
}

export function orgTeamPath(model: AgentTeamOrgModel, teamId: string): OrgTeamNode[] {
  const node = model.nodesById.get(teamId);
  return node ? [node] : [];
}

const ACTIVE_RUN_STATUSES = new Set(["planning", "running", "waiting", "reviewing"]);

function isRecord(value: unknown): value is Record<string, unknown> {
  return Boolean(value) && typeof value === "object" && !Array.isArray(value);
}

export function durableAgentMembers(snapshot: DashboardSnapshot): DurableAgentMember[] {
  if (snapshot.durable_agent_members) return snapshot.durable_agent_members;
  if (!isRecord(snapshot.company_os)) return [];
  const rows = snapshot.company_os.durable_agent_members;
  return Array.isArray(rows) ? rows as DurableAgentMember[] : [];
}

export function organizationMembersById(snapshot: DashboardSnapshot): Map<string, OrgMemberIdentity> {
  const merged = new Map<string, OrgMemberIdentity>();
  for (const member of snapshot.members ?? []) {
    merged.set(member.id, {
      id: member.id,
      name: member.name,
      description: member.description,
      role: member.role,
      status: member.status,
      identitySource: "compatibility",
    });
  }
  for (const member of durableAgentMembers(snapshot)) {
    merged.set(member.id, {
      id: member.id,
      name: member.name,
      description: member.description,
      role: member.role,
      status: member.status,
      identitySource: "durable",
    });
  }
  return merged;
}

function latestRunForTeam(runs: TeamRun[], teamId: string): TeamRun | undefined {
  const candidates = runs
    .filter((run) => run.agent_team_id === teamId)
    .sort((a, b) => (b.created_at ?? "").localeCompare(a.created_at ?? ""));
  return candidates.find((run) => run.status && ACTIVE_RUN_STATUSES.has(run.status)) ?? candidates[0];
}

function workCountsForTeam(works: Work[], runIds: Set<string>): OrgWorkCounts {
  const counts: OrgWorkCounts = { assigned: 0, unassigned: 0, inProgress: 0, blocked: 0, review: 0 };
  for (const work of works) {
    if (!runIds.has(work.team_run_id)) continue;
    if (work.phase === "open") {
      if (work.owner_member_id) counts.assigned += 1;
      else counts.unassigned += 1;
    } else if (work.phase === "active") counts.inProgress += 1;
    if (work.condition === "blocked") counts.blocked += 1;
    if (work.phase === "review") counts.review += 1;
  }
  return counts;
}

function runtimeForTeam(memberRuns: MemberRun[], runIds: Set<string>): OrgRuntimeSummary {
  const scoped = memberRuns.filter((member) => Boolean(member.team_run_id && runIds.has(member.team_run_id)));
  return {
    running: scoped.filter((member) => member.status === "running").length,
    total: scoped.length,
  };
}

export function buildAgentTeamOrgModel(snapshot: DashboardSnapshot): AgentTeamOrgModel {
  const teams = snapshot.teams ?? [];
  const runs = snapshot.team_runs ?? [];
  const membersById = organizationMembersById(snapshot);
  const nodesById = new Map<string, OrgTeamNode>();

  for (const team of teams) {
    const findings: string[] = [];
    const members = (team.member_ids ?? []).flatMap((memberId) => {
      const member = membersById.get(memberId);
      if (!member) {
        findings.push(`member ${memberId} is not present in the snapshot`);
        return [];
      }
      return [member];
    }).sort((a, b) => (a.name ?? a.id).localeCompare(b.name ?? b.id));
    const host = membersById.get(team.host_agent_id);
    if (!host) findings.push(`Host Agent ${team.host_agent_id} is not present in the snapshot`);
    const runIds = new Set(runs.filter((run) => run.agent_team_id === team.id).map((run) => run.id));
    nodesById.set(team.id, {
      team,
      depth: 0,
      parentId: null,
      childTeamIds: [],
      members,
      host,
      workCounts: workCountsForTeam(snapshot.works ?? [], runIds),
      runtime: runtimeForTeam(snapshot.member_runs ?? [], runIds),
      latestRunId: latestRunForTeam(runs, team.id)?.id,
      findings,
    });
  }

  const roots = [...nodesById.values()].sort((a, b) =>
    (a.team.name ?? a.team.id).localeCompare(b.team.name ?? b.team.id),
  );
  return { roots, nodesById, findings: [], hasTopologyEdges: false };
}
