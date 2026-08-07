import { useMemo } from "react";
import { AlertTriangle, ArrowUpRight, BriefcaseBusiness, FilterX, Search } from "lucide-react";

import type { SelectionState } from "@/app/selection";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  buildTeamWorksModel,
  filterTeamWorks,
  type TeamWorkDemandClass,
  type TeamWorksFilters,
} from "@/model/teamWorksSelectors";
import type { DashboardSnapshot, Work } from "@/types";

interface TeamWorksProps {
  snapshot: DashboardSnapshot;
  selection: SelectionState;
  loading?: boolean;
  onSelectionChange?: (selection: Partial<SelectionState>) => void;
}

const demandGroups: Array<{ id: TeamWorkDemandClass; label: string; detail: string }> = [
  { id: "unassigned", label: "Discovered · unassigned", detail: "Open Work with no durable owner" },
  { id: "owned", label: "Owned", detail: "Assigned or operator-created Work" },
  { id: "follow-up", label: "Follow-up", detail: "Child Work with explicit parent_work_id" },
];

export function TeamWorks({ snapshot, selection, loading = false, onSelectionChange }: TeamWorksProps) {
  const model = useMemo(() => buildTeamWorksModel(snapshot), [snapshot]);
  const filters: TeamWorksFilters = {
    teamId: selection.workTeamId,
    hostId: selection.workHostId,
    memberId: selection.workMemberId,
    status: selection.workStatus,
    priority: selection.workPriority,
    source: selection.workSource,
    demand: isDemand(selection.workDemand) ? selection.workDemand : undefined,
  };
  const rows = filterTeamWorks(model.rows, filters);
  const hasFilters = Object.values(filters).some(Boolean);

  if (loading && model.rows.length === 0) return <TeamWorksLoading />;

  function updateFilter(key: keyof SelectionState, value: string): void {
    onSelectionChange?.({ [key]: value || undefined });
  }

  function clearFilters(): void {
    onSelectionChange?.({ workTeamId: undefined, workHostId: undefined, workMemberId: undefined, workStatus: undefined, workPriority: undefined, workSource: undefined, workDemand: undefined });
  }

  function openWork(work: Work): void {
    onSelectionChange?.({ surface: "team", teamId: work.team_run_id, teamWorkId: work.id, memberRunId: undefined, workView: undefined, workTeamId: undefined, workHostId: undefined, workMemberId: undefined, workStatus: undefined, workPriority: undefined, workSource: undefined, workDemand: undefined });
  }

  return (
    <main className="flex h-full min-h-0 flex-1 flex-col overflow-hidden bg-background" data-team-works="ready" data-team-works-scope={model.scopedToSingleRun ? "single-run" : "aggregate"}>
      <header className="shrink-0 border-b border-border bg-card/70 px-4 py-4 sm:px-6">
        <div className="flex flex-wrap items-start justify-between gap-4">
          <div>
            <p className="text-[10px] font-semibold uppercase tracking-[0.16em] text-primary">Work · Company Work</p>
            <h1 className="mt-1 text-2xl font-semibold tracking-tight">All Works</h1>
            <p className="mt-1 max-w-3xl text-xs leading-5 text-muted-foreground">Cross-Team Company Work projection. Filter by Team, Status, or Priority. Individual Works belong to their owning War Room.</p>
          </div>
          <div className="flex flex-wrap gap-2">
            <Badge tone={model.counts.unassigned ? "warn" : "muted"}>{model.counts.unassigned} unassigned</Badge>
            <Badge tone="info">{model.counts.total} total</Badge>
          </div>
        </div>
        {model.scopedToSingleRun && <div role="status" className="mt-3 rounded-lg border border-status-warn/30 bg-status-warn/[0.05] px-3 py-2 text-xs text-muted-foreground">This snapshot contains Work for one TeamRun only ({model.singleRunId}). The page is not claiming a global aggregate.</div>}
      </header>

      <section className="sticky top-0 z-10 shrink-0 border-b border-border bg-background/95 px-4 py-3 backdrop-blur sm:px-6" data-team-works-filter-bar="true">
        <div className="flex gap-2 overflow-x-auto pb-1">
          <label className="flex min-h-11 min-w-[13rem] items-center gap-2 rounded-lg border border-border bg-card px-3 sm:min-h-9">
            <Search className="size-3.5 text-muted-foreground" />
            <span className="text-xs text-muted-foreground">Filters</span>
          </label>
          <Filter label="Team path" value={selection.workTeamId} onChange={(value) => updateFilter("workTeamId", value)} options={model.facets.teams} />
          <Filter label="Host" value={selection.workHostId} onChange={(value) => updateFilter("workHostId", value)} options={model.facets.hosts} />
          <Filter label="Member" value={selection.workMemberId} onChange={(value) => updateFilter("workMemberId", value)} options={model.facets.members} />
          <Filter label="Status" value={selection.workStatus} onChange={(value) => updateFilter("workStatus", value)} options={model.facets.statuses.map((status) => ({ id: status, label: status }))} />
          <Filter label="Priority" value={selection.workPriority} onChange={(value) => updateFilter("workPriority", value)} options={model.facets.priorities.map((p) => ({ id: p, label: p }))} />
          <Filter label="Source" value={selection.workSource} onChange={(value) => updateFilter("workSource", value)} options={model.facets.sources} />
          <Filter label="Demand" value={selection.workDemand} onChange={(value) => updateFilter("workDemand", value)} options={demandGroups.map((group) => ({ id: group.id, label: group.label }))} />
          <button type="button" disabled title="Milestone filter requires the target WorkRelation projection" className="min-h-11 shrink-0 rounded-lg border border-dashed border-border px-3 text-xs text-muted-foreground opacity-65 sm:min-h-9">Milestone unavailable</button>
          {hasFilters && <Button variant="secondary" className="min-h-11 shrink-0 sm:min-h-9" onClick={clearFilters}><FilterX className="size-3.5" />Reset</Button>}
        </div>
      </section>

      <div className="min-h-0 flex-1 overflow-y-auto px-4 py-4 sm:px-6">
        {model.rows.length === 0 ? (
          <EmptyTeamWorks />
        ) : rows.length === 0 ? (
          <section className="mx-auto mt-12 max-w-lg rounded-2xl border border-dashed border-border bg-card p-8 text-center" data-team-works-empty="filtered">
            <FilterX className="mx-auto size-7 text-muted-foreground" />
            <h2 className="mt-3 text-lg font-semibold">No Team Work matches these filters</h2>
            <p className="mt-2 text-sm text-muted-foreground">The aggregate still contains {model.rows.length} rows.</p>
            <Button variant="secondary" className="mt-4" onClick={clearFilters}>Reset filters</Button>
          </section>
        ) : (
          <div className="space-y-5">
            {demandGroups.map((group) => {
              const grouped = rows.filter((row) => row.demandClass === group.id);
              if (!grouped.length) return null;
              return <section key={group.id} aria-labelledby={`team-work-${group.id}`} data-team-work-demand={group.id}>
                <div className="mb-2 flex items-end justify-between gap-3">
                  <div><h2 id={`team-work-${group.id}`} className="text-sm font-semibold">{group.label}</h2><p className="mt-0.5 text-[11px] text-muted-foreground">{group.detail}</p></div>
                  <Badge tone={group.id === "unassigned" ? "warn" : "muted"}>{grouped.length}</Badge>
                </div>
                <div className="grid gap-2">{grouped.map((row) => <TeamWorkRow key={row.work.id} row={row} onOpen={() => openWork(row.work)} />)}</div>
              </section>;
            })}
            <section className="rounded-xl border border-dashed border-border p-4 text-xs leading-5 text-muted-foreground" data-team-work-demand="delegated-unavailable">
              <span className="font-semibold text-foreground">Delegated demand unavailable.</span> WorkDelegation is not present in the dashboard snapshot, so no delegated rows are inferred from child TeamRuns or names.
            </section>
          </div>
        )}
      </div>
    </main>
  );
}

function TeamWorkRow({ row, onOpen }: { row: ReturnType<typeof buildTeamWorksModel>["rows"][number]; onOpen: () => void }) {
  const statusLabel = row.work.status === "review" ? "Awaiting Host acceptance" : row.work.status;
  const created = row.work.created_at ? formatShortDate(row.work.created_at) : undefined;
  const due = row.work.due_at ? formatShortDate(row.work.due_at) : undefined;
  return (
    <button type="button" onClick={onOpen} className="group grid min-h-20 w-full gap-3 rounded-xl border border-border bg-card p-3 text-left shadow-sm transition-colors hover:border-primary/35 sm:grid-cols-[minmax(0,1fr)_auto]" data-team-work-id={row.work.id}>
      <span className="min-w-0">
        <span className="flex flex-wrap items-center gap-2"><span className="font-semibold text-foreground">{row.work.title}</span><Badge tone={workTone(row.work.status)}>{statusLabel}</Badge><Badge tone="muted">{row.work.priority}</Badge></span>
        <span className="mt-1 block break-words text-xs text-muted-foreground">{row.teamPath} · {row.ownerLabel ? `Owner ${row.ownerLabel}` : "Unassigned"}{created ? ` · Created ${created}` : ""}{due ? ` · Due ${due}` : ""}</span>
        <span className="mt-1 block break-all font-mono text-[9px] text-muted-foreground">{row.work.id} · {row.sourceLabel}</span>
        {row.work.blocker_reason && <span className="mt-2 flex items-start gap-1.5 text-xs text-status-bad"><AlertTriangle className="mt-0.5 size-3.5 shrink-0" />{row.work.blocker_reason}</span>}
      </span>
      <span className="flex items-center gap-2 self-center text-xs font-medium text-primary">Open owning War Room <ArrowUpRight className="size-4" /></span>
    </button>
  );
}

function Filter({ label, value, options, onChange }: { label: string; value?: string; options: Array<{ id: string; label: string }>; onChange: (value: string) => void }) {
  return <select aria-label={label} value={value ?? ""} onChange={(event) => onChange(event.target.value)} className="min-h-11 shrink-0 rounded-lg border border-border bg-card px-3 text-xs sm:min-h-9"><option value="">{label} · all</option>{options.map((option) => <option key={option.id} value={option.id}>{option.label}</option>)}</select>;
}

function EmptyTeamWorks() {
  return <section className="mx-auto mt-12 max-w-lg rounded-2xl border border-dashed border-border bg-card p-8 text-center" data-team-works-empty="all"><BriefcaseBusiness className="mx-auto size-8 text-muted-foreground" /><h2 className="mt-4 text-lg font-semibold">No Team Work anywhere in this snapshot</h2><p className="mt-2 text-sm leading-6 text-muted-foreground">No fixture rows are substituted. Create or discover Work through a governed Team flow.</p></section>;
}

function TeamWorksLoading() {
  return <main className="h-full overflow-auto bg-background p-5" data-team-works="loading"><div className="mx-auto max-w-5xl animate-pulse space-y-3"><div className="h-8 w-64 rounded bg-muted" /><div className="h-11 rounded bg-muted" />{[0, 1, 2].map((row) => <div key={row} className="h-24 rounded-xl bg-muted/70" />)}</div></main>;
}

function isDemand(value?: string): value is TeamWorkDemandClass {
  return value === "unassigned" || value === "owned" || value === "follow-up";
}

function workTone(status: string): "good" | "warn" | "bad" | "info" | "muted" {
  if (status === "done") return "good";
  if (status === "blocked") return "bad";
  if (status === "review") return "warn";
  if (status === "in_progress") return "info";
  return "muted";
}

function formatShortDate(value: string): string {
  try {
    if (value.startsWith("unix-ms:")) {
      const ms = Number(value.slice("unix-ms:".length));
      if (Number.isFinite(ms)) return new Date(ms).toLocaleDateString();
    }
    const d = new Date(value);
    if (Number.isNaN(d.getTime())) return value;
    return d.toLocaleDateString();
  } catch {
    return value;
  }
}
