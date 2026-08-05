import type {
  AgentTeam,
  DashboardSnapshot,
  DurableAgentMember,
  MemberRun,
  TeamRun,
  Work,
} from "../types";

/**
 * Recursive Organization model (ADR 0052) derived only from real snapshot
 * fields. Topology edges come from `AgentTeam.parent_team_id` /
 * `host_member_id` (wire names frozen by the core topology slice). Rows
 * pre-dating that slice read as hostless roots; ancestry is never inferred
 * from names, sessions, or first-row fallback.
 *
 * Contract: docs/design/company-os-v6/recursive-org-docs-works-v1.
 */

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

/** Identity-only view used by Organization. Durable rows override matching
 * compatibility AgentMember rows without absorbing runtime/session fields. */
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
  depth: number;
  parentId: string | null;
  childTeamIds: string[];
  /** Direct members resolved from `member_ids`; unresolved ids are findings. */
  members: OrgMemberIdentity[];
  /** Durable Host from `host_member_id` when the store records one. */
  host?: OrgMemberIdentity;
  /**
   * Compatibility lead label from `owner_agent_id` when no `host_member_id`
   * is recorded. Labelled as compatibility, never presented as the ADR 0052
   * Host relation.
   */
  compatLeadLabel?: string;
  /** Current Work counts across this Team's runs (TeamRun-scoped Works). */
  workCounts: OrgWorkCounts;
  /** Runtime state of MemberRuns across this Team's runs — never durable status. */
  runtime: OrgRuntimeSummary;
  /** Newest active TeamRun for this Team (navigation target), if any. */
  latestRunId?: string;
  findings: string[];
}

export interface AgentTeamOrgModel {
  /** Root nodes in deterministic order; children hang off `childTeamIds`. */
  roots: OrgTeamNode[];
  nodesById: Map<string, OrgTeamNode>;
  /** Store-level integrity findings (missing parent, cycle, dangling member). */
  findings: string[];
  /** True when at least one `parent_team_id` edge exists in the store. */
  hasTopologyEdges: boolean;
}

/** Proven root-to-node path. Missing parents/cycles were already detached. */
export function orgTeamPath(model: AgentTeamOrgModel, teamId: string): OrgTeamNode[] {
  const path: OrgTeamNode[] = [];
  const seen = new Set<string>();
  let cursor = model.nodesById.get(teamId);
  while (cursor && !seen.has(cursor.team.id)) {
    seen.add(cursor.team.id);
    path.unshift(cursor);
    cursor = cursor.parentId ? model.nodesById.get(cursor.parentId) : undefined;
  }
  return path;
}

const ACTIVE_RUN_STATUSES = new Set([
  "planning",
  "running",
  "waiting",
  "reviewing",
]);

function isRecord(value: unknown): value is Record<string, unknown> {
  return Boolean(value) && typeof value === "object" && !Array.isArray(value);
}

/** Read the additive ADR 0052 projection while accepting a future top-level
 * lift. An explicitly present top-level field wins, including an empty list. */
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
  return (
    candidates.find((run) => run.status && ACTIVE_RUN_STATUSES.has(run.status)) ??
    candidates[0]
  );
}

function workCountsForTeam(
  works: Work[],
  runIds: Set<string>,
): OrgWorkCounts {
  const counts: OrgWorkCounts = {
    assigned: 0,
    unassigned: 0,
    inProgress: 0,
    blocked: 0,
    review: 0,
  };
  for (const work of works) {
    if (!runIds.has(work.team_run_id)) continue;
    if (work.status === "open") {
      if (work.owner_member_id) counts.assigned += 1;
      else counts.unassigned += 1;
    } else if (work.status === "in_progress") counts.inProgress += 1;
    else if (work.status === "blocked") counts.blocked += 1;
    else if (work.status === "review") counts.review += 1;
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
  const memberRuns = snapshot.member_runs ?? [];
  const works = snapshot.works ?? [];

  const membersById = organizationMembersById(snapshot);
  const teamsById = new Map(teams.map((team) => [team.id, team]));
  const findings: string[] = [];
  const nodesById = new Map<string, OrgTeamNode>();

  for (const team of teams) {
    const runIds = new Set(
      runs.filter((run) => run.agent_team_id === team.id).map((run) => run.id),
    );
    const nodeFindings: string[] = [];
    const resolvedMembers: OrgMemberIdentity[] = [];
    for (const memberId of team.member_ids ?? []) {
      const member = membersById.get(memberId);
      if (member) resolvedMembers.push(member);
      else nodeFindings.push(`member ${memberId} is not present in the snapshot`);
    }
    resolvedMembers.sort((a, b) => (a.name ?? a.id).localeCompare(b.name ?? b.id));

    let host: OrgMemberIdentity | undefined;
    let compatLeadLabel: string | undefined;
    if (team.host_member_id) {
      host = membersById.get(team.host_member_id);
      if (!host) {
        nodeFindings.push(`host member ${team.host_member_id} is not present in the snapshot`);
      }
    } else if (team.owner_agent_id) {
      compatLeadLabel =
        team.owner_agent_id === "host"
          ? "Host Agent (compatibility)"
          : `${membersById.get(team.owner_agent_id)?.name ?? team.owner_agent_id} (compatibility lead)`;
    }

    nodesById.set(team.id, {
      team,
      depth: 0,
      parentId: team.parent_team_id ?? null,
      childTeamIds: [],
      members: resolvedMembers,
      host,
      compatLeadLabel,
      workCounts: workCountsForTeam(works, runIds),
      runtime: runtimeForTeam(memberRuns, runIds),
      latestRunId: latestRunForTeam(runs, team.id)?.id,
      findings: nodeFindings,
    });
  }

  // Attach children with missing-parent and cycle defenses. The store
  // enforces these invariants on write; the dashboard still validates so a
  // corrupted or partial projection is reported, never silently repaired.
  const roots: OrgTeamNode[] = [];
  for (const node of nodesById.values()) {
    const parentId = node.parentId;
    if (!parentId) {
      roots.push(node);
      continue;
    }
    const parent = nodesById.get(parentId);
    if (!parent) {
      node.findings.push(`parent team ${parentId} is not present in the snapshot`);
      findings.push(`${node.team.name ?? node.team.id}: parent team ${parentId} is missing`);
      node.parentId = null;
      roots.push(node);
      continue;
    }
    // Cycle check: walking ancestors from the parent must never reach this node.
    let cursor: OrgTeamNode | undefined = parent;
    let cyclic = false;
    const seen = new Set<string>([node.team.id]);
    while (cursor) {
      if (seen.has(cursor.team.id)) {
        cyclic = true;
        break;
      }
      seen.add(cursor.team.id);
      cursor = cursor.parentId ? nodesById.get(cursor.parentId) : undefined;
    }
    if (cyclic) {
      node.findings.push(`parent edge to ${parentId} would create a cycle`);
      findings.push(`${node.team.name ?? node.team.id}: parent edge to ${parentId} would create a cycle`);
      node.parentId = null;
      roots.push(node);
      continue;
    }
    parent.childTeamIds.push(node.team.id);

    if (node.team.host_member_id && !(parent.team.member_ids ?? []).includes(node.team.host_member_id)) {
      const detail = `host member ${node.team.host_member_id} is not a direct member of parent team ${parent.team.id}`;
      node.findings.push(detail);
      findings.push(`${node.team.name ?? node.team.id}: ${detail}`);
    }
  }

  // Depth assignment over the now-acyclic forest.
  const assignDepth = (node: OrgTeamNode, depth: number): void => {
    node.depth = depth;
    for (const childId of node.childTeamIds) {
      const child = nodesById.get(childId);
      if (child) assignDepth(child, depth + 1);
    }
  };
  for (const root of roots) assignDepth(root, 0);

  const byName = (a: OrgTeamNode, b: OrgTeamNode): number =>
    (a.team.name ?? a.team.id).localeCompare(b.team.name ?? b.team.id);
  roots.sort(byName);
  for (const node of nodesById.values()) {
    node.childTeamIds.sort((a, b) =>
      (nodesById.get(a)?.team.name ?? a).localeCompare(nodesById.get(b)?.team.name ?? b),
    );
  }

  return {
    roots,
    nodesById,
    findings,
    hasTopologyEdges: teams.some((team) => Boolean(team.parent_team_id)),
  };
}
