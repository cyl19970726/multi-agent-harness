import { useMemo, useState } from "react";
import {
  AlertTriangle,
  ArrowUpRight,
  Bot,
  BriefcaseBusiness,
  ChevronDown,
  ChevronRight,
  Monitor,
  Network,
  Search,
  ShieldCheck,
  Users,
} from "lucide-react";

import type { SelectionState } from "@/app/selection";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";
import {
  buildAgentTeamOrgModel,
  orgTeamPath,
  type AgentTeamOrgModel,
  type OrgTeamNode,
} from "@/model/orgSelectors";
import type { DashboardSnapshot, MemberRun } from "@/types";

interface AgentTeamOrganizationProps {
  snapshot: DashboardSnapshot;
  selection: SelectionState;
  loading?: boolean;
  onSelectionChange?: (selection: Partial<SelectionState>) => void;
}

type RuntimeFilter = "all" | "running" | "idle";

export function AgentTeamOrganization({
  snapshot,
  selection,
  loading = false,
  onSelectionChange,
}: AgentTeamOrganizationProps) {
  const model = useMemo(() => buildAgentTeamOrgModel(snapshot), [snapshot]);
  const [query, setQuery] = useState("");
  const [durableStatus, setDurableStatus] = useState("all");
  const [runtimeFilter, setRuntimeFilter] = useState<RuntimeFilter>("all");
  const [unassignedOnly, setUnassignedOnly] = useState(false);
  const selected = model.nodesById.get(selection.orgTeamId ?? "") ?? model.roots[0];
  const defaultExpanded = new Set([
    ...model.roots.map((node) => node.team.id),
    ...(selected ? orgTeamPath(model, selected.team.id).map((node) => node.team.id) : []),
  ]);
  const expanded = selection.orgExpanded
    ? new Set(selection.orgExpanded.split(",").filter(Boolean))
    : defaultExpanded;
  const normalizedQuery = query.trim().toLowerCase();
  const visible = visibleTeamIds(model, normalizedQuery, durableStatus, runtimeFilter, unassignedOnly);
  const allFindings = [
    ...model.findings,
    ...[...model.nodesById.values()].flatMap((node) =>
      node.findings.map((finding) => `${node.team.name ?? node.team.id}: ${finding}`),
    ),
  ];

  if (loading && model.nodesById.size === 0) return <OrganizationLoading />;

  if (model.roots.length === 0) {
    return (
      <main className="grid h-full min-h-0 place-items-center overflow-auto bg-background p-5" data-agent-team-organization="empty">
        <section className="max-w-xl rounded-2xl border border-dashed border-border bg-card p-8 text-center">
          <Network className="mx-auto size-8 text-muted-foreground" />
          <h1 className="mt-4 text-xl font-semibold">No root Agent Team yet</h1>
          <p className="mt-2 text-sm leading-6 text-muted-foreground">
            Recursive Organization appears after the first durable AgentTeam exists. No fixture Team is substituted.
          </p>
          <Button className="mt-5" disabled title="A governed Team creation transport is not connected">Create Team unavailable</Button>
        </section>
      </main>
    );
  }

  const path = selected ? orgTeamPath(model, selected.team.id) : [];
  const mobileChildren = selected
    ? selected.childTeamIds.map((id) => model.nodesById.get(id)).filter((node): node is OrgTeamNode => Boolean(node))
    : model.roots;

  function selectTeam(teamId: string): void {
    onSelectionChange?.({ orgView: "agent-teams", orgTeamId: teamId });
  }

  function toggleTeam(teamId: string): void {
    const next = new Set(expanded);
    if (next.has(teamId)) next.delete(teamId);
    else next.add(teamId);
    onSelectionChange?.({ orgExpanded: [...next].sort().join(",") || undefined });
  }

  function openTeam(node: OrgTeamNode): void {
    if (!node.latestRunId) return;
    onSelectionChange?.({ surface: "team", teamId: node.latestRunId, memberRunId: undefined, orgView: undefined, orgTeamId: undefined, orgExpanded: undefined });
  }

  return (
    <main className="flex h-full min-h-0 flex-1 flex-col overflow-hidden bg-background" data-agent-team-organization="ready" data-org-topology-edges={model.hasTopologyEdges ? "present" : "compatibility-roots"}>
      <header className="shrink-0 border-b border-border bg-card/70 px-4 py-4 sm:px-6">
        <div className="flex flex-wrap items-start justify-between gap-4">
          <div>
            <p className="text-[10px] font-semibold uppercase tracking-[0.16em] text-primary">Organization · Agent Teams</p>
            <h1 className="mt-1 text-2xl font-semibold tracking-tight">Recursive execution organization</h1>
            <p className="mt-1 max-w-3xl text-xs leading-5 text-muted-foreground">Durable AgentTeam topology, explicit direct Members, TeamRun-scoped Work counts, and separately labelled runtime state.</p>
          </div>
          <div className="flex gap-2">
            <Button disabled title="A governed Team creation transport is not connected">Create Team</Button>
            <Button variant="secondary" disabled title="A governed membership transport is not connected">Add Member</Button>
          </div>
        </div>
        <div className="mt-4 hidden flex-wrap items-center gap-2 md:flex" data-org-filter-bar="true">
          <OrganizationFilterControls query={query} durableStatus={durableStatus} runtimeFilter={runtimeFilter} unassignedOnly={unassignedOnly} onQuery={setQuery} onDurableStatus={setDurableStatus} onRuntimeFilter={setRuntimeFilter} onUnassignedOnly={setUnassignedOnly} />
        </div>
        <details className="group mt-3 rounded-lg border border-border bg-background md:hidden">
          <summary className="flex min-h-11 cursor-pointer list-none items-center gap-2 px-3 text-xs font-medium"><Search className="size-4 text-muted-foreground" />Filters <ChevronDown className="ml-auto size-4 text-muted-foreground transition-transform group-open:rotate-180" /></summary>
          <div className="grid gap-2 border-t border-border p-3" data-org-filter-bar="true">
            <OrganizationFilterControls query={query} durableStatus={durableStatus} runtimeFilter={runtimeFilter} unassignedOnly={unassignedOnly} onQuery={setQuery} onDurableStatus={setDurableStatus} onRuntimeFilter={setRuntimeFilter} onUnassignedOnly={setUnassignedOnly} />
          </div>
        </details>
      </header>

      {allFindings.length > 0 && (
        <section role="alert" className="shrink-0 border-b border-status-warn/30 bg-status-warn/[0.06] px-4 py-3 text-xs" data-org-integrity-count={allFindings.length}>
          <div className="flex items-center gap-2 font-semibold text-status-warn"><AlertTriangle className="size-4" />Topology integrity findings ({allFindings.length})</div>
          <ul className="mt-2 list-disc space-y-1 pl-5 text-muted-foreground">{allFindings.map((finding, index) => <li key={`${finding}-${index}`}>{finding}</li>)}</ul>
        </section>
      )}

      <div className="hidden min-h-0 flex-1 grid-cols-[minmax(19rem,0.9fr)_minmax(24rem,1.3fr)] md:grid">
        <section className="min-h-0 overflow-y-auto border-r border-border p-4" aria-label="Agent Team tree">
          <div className="space-y-2">{model.roots.map((root) => <TeamTreeBranch key={root.team.id} node={root} model={model} selectedId={selected?.team.id} expanded={expanded} visible={visible} onSelect={selectTeam} onToggle={toggleTeam} />)}</div>
        </section>
        <section className="min-h-0 overflow-y-auto p-5 lg:p-7" aria-label="Selected Agent Team detail">
          {selected && <TeamDetail node={selected} snapshot={snapshot} onOpenTeam={() => openTeam(selected)} onOpenMember={(memberRun) => onSelectionChange?.({ surface: "team", teamId: memberRun.team_run_id, memberRunId: memberRun.id, orgView: undefined, orgTeamId: undefined, orgExpanded: undefined })} onSelectionChange={onSelectionChange} />}
        </section>
      </div>

      <section className="min-h-0 flex-1 overflow-y-auto p-4 md:hidden" aria-label="Agent Team drill-down">
        <nav aria-label="Organization breadcrumb" className="mb-3 flex flex-wrap items-center gap-1 text-xs text-muted-foreground">
          <button type="button" onClick={() => onSelectionChange?.({ orgTeamId: undefined })} className="min-h-11 px-1 text-primary">Organization</button>
          {path.map((entry) => <span key={entry.team.id} className="inline-flex items-center gap-1"><ChevronRight className="size-3" /><button type="button" onClick={() => selectTeam(entry.team.id)} className="min-h-11 max-w-40 truncate px-1">{entry.team.name ?? entry.team.id}</button></span>)}
        </nav>
        {selected && <TeamDetail node={selected} snapshot={snapshot} compact onOpenTeam={() => openTeam(selected)} onOpenMember={(memberRun) => onSelectionChange?.({ surface: "team", teamId: memberRun.team_run_id, memberRunId: memberRun.id, orgView: undefined, orgTeamId: undefined, orgExpanded: undefined })} onSelectionChange={onSelectionChange} />}
        <div className="mt-4 space-y-2">
          <p className="text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">Child Teams</p>
          {mobileChildren.length ? mobileChildren.map((child) => <button key={child.team.id} type="button" onClick={() => selectTeam(child.team.id)} className="flex min-h-12 w-full items-center justify-between rounded-xl border border-border bg-card px-3 text-left text-sm"><span>{child.team.name ?? child.team.id}</span><ChevronRight className="size-4 text-muted-foreground" /></button>) : <p className="rounded-xl border border-dashed border-border p-4 text-xs text-muted-foreground">No child Teams</p>}
        </div>
      </section>
    </main>
  );
}

function TeamTreeBranch({ node, model, selectedId, expanded, visible, onSelect, onToggle }: { node: OrgTeamNode; model: AgentTeamOrgModel; selectedId?: string; expanded: Set<string>; visible: Set<string>; onSelect: (id: string) => void; onToggle: (id: string) => void }) {
  const children = node.childTeamIds.map((id) => model.nodesById.get(id)).filter((child): child is OrgTeamNode => Boolean(child));
  const open = expanded.has(node.team.id);
  const dimmed = !visible.has(node.team.id);
  return (
    <div data-org-team-depth={node.depth} data-org-team-id={node.team.id} className={cn("transition-opacity", dimmed && "opacity-35")}>
      <div className={cn("flex items-center gap-1 rounded-xl border bg-card p-2", selectedId === node.team.id ? "border-primary/40 ring-1 ring-primary/15" : "border-border")}>
        <button type="button" aria-label={`${open ? "Collapse" : "Expand"} ${node.team.name ?? node.team.id}`} disabled={!children.length} onClick={() => onToggle(node.team.id)} className="grid size-10 shrink-0 place-items-center rounded-lg text-muted-foreground disabled:opacity-25">{open ? <ChevronDown className="size-4" /> : <ChevronRight className="size-4" />}</button>
        <button type="button" onClick={() => onSelect(node.team.id)} className="min-w-0 flex-1 rounded-lg px-2 py-1 text-left">
          <span className="block truncate text-sm font-semibold">{node.team.name ?? node.team.id}</span>
          <span className="mt-1 flex flex-wrap gap-1"><Badge tone="muted" data-durable-status={node.team.status ?? "unknown"}>Durable · {node.team.status ?? "unknown"}</Badge>{node.team.machine_id && <Badge tone="info"><Monitor className="size-2.5" /> {node.team.machine_id}</Badge>}<Badge tone={node.runtime.running ? "running" : "muted"} data-runtime-state={node.runtime.running ? "running" : "idle"}>Runtime · {node.runtime.running}/{node.runtime.total}</Badge></span>
        </button>
      </div>
      {open && children.length > 0 && <div className="ml-5 mt-2 space-y-2 border-l border-border pl-3">{children.map((child) => <TeamTreeBranch key={child.team.id} node={child} model={model} selectedId={selectedId} expanded={expanded} visible={visible} onSelect={onSelect} onToggle={onToggle} />)}</div>}
    </div>
  );
}

function OrganizationFilterControls({ query, durableStatus, runtimeFilter, unassignedOnly, onQuery, onDurableStatus, onRuntimeFilter, onUnassignedOnly }: { query: string; durableStatus: string; runtimeFilter: RuntimeFilter; unassignedOnly: boolean; onQuery: (value: string) => void; onDurableStatus: (value: string) => void; onRuntimeFilter: (value: RuntimeFilter) => void; onUnassignedOnly: (value: boolean) => void }) {
  return <>
    <label className="flex min-h-11 min-w-0 flex-1 items-center gap-2 rounded-lg border border-border bg-background px-3 sm:min-h-9 sm:min-w-[15rem] sm:max-w-sm">
      <Search className="size-4 text-muted-foreground" />
      <input aria-label="Filter Agent Teams and Members" value={query} onChange={(event) => onQuery(event.target.value)} className="min-w-0 flex-1 bg-transparent text-xs outline-none" placeholder="Filter Team or Member" />
    </label>
    <select aria-label="Durable Team status" value={durableStatus} onChange={(event) => onDurableStatus(event.target.value)} className="min-h-11 rounded-lg border border-border bg-background px-3 text-xs sm:min-h-9">
      <option value="all">All durable states</option><option value="active">Durable · active</option><option value="closed">Durable · closed</option><option value="archived">Durable · archived</option>
    </select>
    <select aria-label="Runtime state" value={runtimeFilter} onChange={(event) => onRuntimeFilter(event.target.value as RuntimeFilter)} className="min-h-11 rounded-lg border border-border bg-background px-3 text-xs sm:min-h-9">
      <option value="all">All runtime states</option><option value="running">Runtime · running</option><option value="idle">Runtime · not running</option>
    </select>
    <label className="flex min-h-11 items-center gap-2 rounded-lg border border-border bg-background px-3 text-xs sm:min-h-9"><input type="checkbox" checked={unassignedOnly} onChange={(event) => onUnassignedOnly(event.target.checked)} /> Has unassigned Work</label>
  </>;
}

function TeamDetail({ node, snapshot, compact = false, onOpenTeam, onOpenMember, onSelectionChange }: { node: OrgTeamNode; snapshot: DashboardSnapshot; compact?: boolean; onOpenTeam: () => void; onOpenMember: (member: MemberRun) => void; onSelectionChange?: (selection: Partial<SelectionState>) => void }) {
  const runIds = new Set((snapshot.team_runs ?? []).filter((run) => run.agent_team_id === node.team.id).map((run) => run.id));
  const latestMemberRun = (memberId: string) => [...(snapshot.member_runs ?? [])]
    .filter((run) => Boolean(run.team_run_id && runIds.has(run.team_run_id)) && run.agent_member_id === memberId)
    .sort((a, b) => (b.started_at ?? "").localeCompare(a.started_at ?? ""))[0];
  return (
    <article className={cn("rounded-2xl border border-border bg-card shadow-sm", compact ? "p-4" : "p-5 lg:p-6")} data-org-selected-team={node.team.id}>
      <div className="flex flex-wrap items-start justify-between gap-4">
        <div className="min-w-0">
          <p className="text-[10px] font-semibold uppercase tracking-[0.14em] text-primary">AgentTeam · depth {node.depth}</p>
          <h2 className="mt-1 break-words text-2xl font-semibold">{node.team.name ?? node.team.id}</h2>
          <p className="mt-2 max-w-2xl text-sm leading-6 text-muted-foreground">{node.team.description || "No durable Team description supplied."}</p>
          {node.team.machine_id && (
            <div className="mt-2 inline-flex items-center gap-1.5 rounded-lg border border-border bg-background px-2.5 py-1 text-xs text-muted-foreground">
              <Monitor className="size-3.5 text-primary" />
              <span>Runs on <span className="font-mono text-foreground">{node.team.machine_id}</span></span>
            </div>
          )}
          <code className="mt-2 block break-all text-[10px] text-muted-foreground">{node.team.id}</code>
        </div>
        <div className="flex flex-wrap gap-2">
          <Button onClick={onOpenTeam} disabled={!node.latestRunId} title={node.latestRunId ? "Open newest TeamRun" : "No TeamRun exists for this Team"}>Open War Room</Button>
          {node.team.id && (
            <Button variant="secondary" onClick={() => onSelectionChange?.({ surface: "work", workView: "team-works", workTeamId: node.team.id, workHostId: undefined, workMemberId: undefined, workStatus: undefined, workPriority: undefined, workSource: undefined, workDemand: undefined })} title="Filter Company Work by this Team">
              <BriefcaseBusiness className="size-3.5" /> View Works
            </Button>
          )}
        </div>
      </div>
      <div className="mt-4 grid gap-2 sm:grid-cols-3">
        <Fact label="Durable status" value={node.team.status ?? "unknown"} icon={<ShieldCheck className="size-3.5" />} dataAttribute="durable" />
        <Fact label="Runtime attempts" value={`${node.runtime.running} running / ${node.runtime.total} total`} icon={<Bot className="size-3.5" />} dataAttribute="runtime" />
        <Fact label="Direct structure" value={`${node.members.length} Members / ${node.childTeamIds.length} child Teams`} icon={<Users className="size-3.5" />} />
      </div>
      <section className="mt-5">
        <h3 className="text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">Current TeamRun Work</h3>
        <div className="mt-2 grid grid-cols-2 gap-2 sm:grid-cols-5">
          <Count label="Assigned" value={node.workCounts.assigned} /><Count label="Unassigned" value={node.workCounts.unassigned} /><Count label="In progress" value={node.workCounts.inProgress} /><Count label="Blocked" value={node.workCounts.blocked} /><Count label="Review" value={node.workCounts.review} />
        </div>
      </section>
      <section className="mt-5">
        <h3 className="text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">Host relation</h3>
        <p className="mt-2 rounded-xl border border-border bg-background p-3 text-sm">{node.host ? `${node.host.name ?? node.host.id} · explicit host_member_id · ${node.host.identitySource} identity` : node.compatLeadLabel ?? "No Host relation recorded"}</p>
      </section>
      <section className="mt-5">
        <h3 className="text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">Direct Members</h3>
        {node.members.length > 0 ? <div className="mt-2 grid gap-2 sm:grid-cols-2">{node.members.map((member) => { const run = latestMemberRun(member.id); return <button key={member.id} type="button" disabled={!run} onClick={() => run && onOpenMember(run)} className="min-h-14 rounded-xl border border-border bg-background p-3 text-left disabled:cursor-default"><span className="block text-sm font-semibold">{member.name ?? member.id}</span><span className="mt-1 block text-xs text-muted-foreground">{member.identitySource === "durable" ? "Durable" : "Compatibility"} · {member.status ?? "unknown"}{run ? ` · Runtime ${run.status ?? "unknown"}` : " · No MemberRun"}</span></button>; })}</div> : <p className="mt-2 rounded-xl border border-dashed border-border p-4 text-xs text-muted-foreground">No direct Members</p>}
      </section>
      {node.findings.length > 0 && <section className="mt-5 rounded-xl border border-status-warn/30 bg-status-warn/[0.05] p-3 text-xs text-muted-foreground"><p className="font-semibold text-status-warn">Node findings</p><ul className="mt-2 list-disc space-y-1 pl-5">{node.findings.map((finding) => <li key={finding}>{finding}</li>)}</ul></section>}
    </article>
  );
}

function Fact({ label, value, icon, dataAttribute }: { label: string; value: string; icon: React.ReactNode; dataAttribute?: "durable" | "runtime" }) {
  return <div className="rounded-xl border border-border bg-background p-3" {...(dataAttribute === "durable" ? { "data-durable-status": value } : dataAttribute === "runtime" ? { "data-runtime-state": value } : {})}><div className="flex items-center gap-2 text-[10px] uppercase tracking-wider text-muted-foreground">{icon}{label}</div><p className="mt-2 text-sm font-semibold">{value}</p></div>;
}

function Count({ label, value }: { label: string; value: number }) {
  return <div className="rounded-lg border border-border bg-background p-2 text-center"><p className="text-xl font-semibold tabular-nums">{value}</p><p className="text-[9px] uppercase tracking-wider text-muted-foreground">{label}</p></div>;
}

function OrganizationLoading() {
  return <main className="h-full overflow-auto bg-background p-5" data-agent-team-organization="loading"><div className="mx-auto max-w-5xl animate-pulse space-y-3"><div className="h-8 w-64 rounded bg-muted" /><div className="h-11 rounded bg-muted" />{[0, 1, 2].map((row) => <div key={row} className="h-24 rounded-xl bg-muted/70" />)}</div></main>;
}

function visibleTeamIds(model: AgentTeamOrgModel, query: string, durable: string, runtime: RuntimeFilter, unassignedOnly: boolean): Set<string> {
  if (!query && durable === "all" && runtime === "all" && !unassignedOnly) return new Set(model.nodesById.keys());
  const direct = new Set<string>();
  for (const node of model.nodesById.values()) {
    const text = `${node.team.name ?? ""} ${node.team.id} ${node.members.map((member) => `${member.name ?? ""} ${member.id}`).join(" ")}`.toLowerCase();
    const matches = (!query || text.includes(query))
      && (durable === "all" || node.team.status === durable)
      && (runtime === "all" || (runtime === "running" ? node.runtime.running > 0 : node.runtime.running === 0))
      && (!unassignedOnly || node.workCounts.unassigned > 0);
    if (matches) direct.add(node.team.id);
  }
  const visible = new Set(direct);
  for (const teamId of direct) for (const ancestor of orgTeamPath(model, teamId)) visible.add(ancestor.team.id);
  return visible;
}
