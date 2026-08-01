import { useMemo, useState } from "react";
import {
  AlertTriangle,
  ArrowUpRight,
  Bot,
  CalendarDays,
  CheckCircle2,
  CircleDot,
  Clock3,
  Columns3,
  Filter,
  Flag,
  LayoutDashboard,
  ListChecks,
  Search,
  Sparkles,
  Table2,
  UserRound,
  UsersRound,
} from "lucide-react";

import { ActorAvatar, ObjectEmblem } from "../visuals";
import { preserveCompanyOsWorkbenchContext } from "../docs/url";
import { acceptanceCriteriaPresentation, projectBusinessLineDimensions } from "./projection";

type Json = Record<string, unknown>;
type WorkView = "overview" | "board" | "all" | "milestones" | "timeline" | "workload";

interface ActorRow {
  id: string;
  name: string;
  kind: string;
  role: string;
}

interface EntityLink {
  kind: string;
  id: string;
}

interface WorkRow {
  id: string;
  title: string;
  objective: string;
  description?: string;
  acceptanceCriteria: string[];
  contextRefs: EntityLink[];
  deliverableRefs: EntityLink[];
  status: string;
  workType: string;
  businessLineId?: string;
  businessLine: string;
  milestoneId?: string;
  milestone: string;
  accountable?: ActorRow;
  assignees: ActorRow[];
  contributors: ActorRow[];
  reviewer?: ActorRow;
  approvalId?: string;
  approval: string;
  dueAt?: string;
  priority?: string;
  source: string;
  sourceId?: string;
  /** Explicit lifecycle of the resolved source Document; archived stays navigable history. */
  sourceLifecycle?: string;
  execution: string;
  updatedAt: string;
}

interface MilestoneRow {
  id: string;
  title: string;
  outcome: string;
  status: string;
  targetAt?: string;
  accountable?: ActorRow;
  total: number;
  completed: number;
  blocked: number;
  waiting: number;
  progress: number;
}

interface WorkDimensionRow {
  id: string;
  label: string;
  workItemIds: string[];
}

interface WorkloadRow {
  actorId: string;
  actor?: ActorRow;
  accountable: number;
  assigned: number;
  active: number;
  workItemIds: string[];
}

interface WorkModel {
  provenance: "company_os.work" | "legacy_raw_records";
  integrity: string[];
  items: WorkRow[];
  milestones: MilestoneRow[];
  actors: ActorRow[];
  board: WorkDimensionRow[];
  businessLines: WorkDimensionRow[];
  workTypes: WorkDimensionRow[];
  workload: WorkloadRow[];
  summary: {
    total: number;
    active: number;
    completed: number;
    blocked: number;
    waiting: number;
    unassigned: number;
  };
}

function objects(value: unknown): Json[] {
  if (!Array.isArray(value)) return [];
  return value.filter((item): item is Json => Boolean(item) && typeof item === "object" && !Array.isArray(item));
}

function unbox(value: Json): Json {
  const record = value.record;
  return record && typeof record === "object" && !Array.isArray(record)
    ? { ...(record as Json), ...value }
    : value;
}

function text(value: unknown, fallback = ""): string {
  return typeof value === "string" ? value : fallback;
}

function finiteNumber(value: unknown): number | undefined {
  return typeof value === "number" && Number.isFinite(value) ? value : undefined;
}

function object(value: unknown): Json {
  return value && typeof value === "object" && !Array.isArray(value) ? value as Json : {};
}

function stringArray(value: unknown): string[] {
  return Array.isArray(value) ? value.map((entry) => text(entry)).filter(Boolean) : [];
}

function refId(value: unknown): string {
  if (typeof value === "string") return value;
  if (!value || typeof value !== "object" || Array.isArray(value)) return "";
  const ref = value as Json;
  return text(ref.actor_id) || text(ref.id);
}

function entityLinks(value: unknown): EntityLink[] {
  return objects(value).map((entry) => ({
    kind: text(entry.kind, "record"),
    id: text(entry.id),
  })).filter((entry) => entry.id);
}

function humanize(value: string): string {
  return value.replace(/[_-]+/g, " ").replace(/\b\w/g, (letter) => letter.toUpperCase());
}

function workItemHref(id: string): string {
  const href = `?surface=work&workItem=${encodeURIComponent(id)}`;
  return preserveCompanyOsWorkbenchContext(href) ?? href;
}

function documentHref(id: string): string {
  const href = `?surface=docs&document=${encodeURIComponent(id)}`;
  return preserveCompanyOsWorkbenchContext(href) ?? href;
}

/**
 * Every visible Work source resolves to an active Document title or explicit
 * archived-source history (title + lifecycle); a missing source says so.
 */
function sourceLabel(item: WorkRow): string {
  if (item.sourceLifecycle === "archived") return `${item.source} · archived history`;
  if (item.sourceLifecycle === "missing") return `${item.source} · missing source`;
  return item.source;
}

function dateLabel(value?: string): string {
  if (!value) return "No due date";
  const match = value.match(/^(\d{4})-(\d{2})-(\d{2})/);
  if (!match) return value;
  const [, year, month, day] = match;
  return `${day}/${month}/${year}`;
}

function isActiveWork(status: string): boolean {
  return !new Set(["draft", "completed", "cancelled", "archived"]).has(status);
}

function priorityWeight(priority?: string): number {
  return priority === "critical" ? 4 : priority === "high" ? 3 : priority === "medium" ? 2 : priority === "low" ? 1 : 0;
}

function buildModel(source: unknown): WorkModel {
  const root = source && typeof source === "object" && !Array.isArray(source) ? source as Json : {};
  const hasAggregate = Object.prototype.hasOwnProperty.call(root, "work");
  const projection = object(root.work);
  const integrity: string[] = [];
  if (hasAggregate && Object.keys(projection).length === 0 && root.work !== undefined) {
    integrity.push("company_os.work is present but is not a readable aggregate object.");
  }
  const actorRecords = objects(root.actors).map(unbox);
  const actorMap = new Map<string, ActorRow>();
  for (const actor of actorRecords) {
    const id = text(actor.id);
    if (!id) continue;
    actorMap.set(id, {
      id,
      name: text(actor.display_name, id),
      kind: text(actor.actor_type, "service").toLowerCase(),
      role: text(actor.role, text(actor.responsibility_summary, "Company participant")),
    });
  }
  const resolveActor = (value: unknown): ActorRow | undefined => {
    const id = refId(value);
    if (!id) return undefined;
    return actorMap.get(id) ?? { id, name: id, kind: text((value as Json | undefined)?.actor_type, "service"), role: "Unresolved actor reference" };
  };
  const modules = new Map(objects(root.business_modules).map(unbox).map((record) => [text(record.id), text(record.name, text(record.title, text(record.id))) ]));
  const documents = new Map(objects(root.documents).map(unbox).map((record) => [
    text(record.id),
    {
      title: text(record.title, text(record.id)),
      lifecycle: text(record.lifecycle_status, text(record.status)).toLowerCase(),
    },
  ]));
  const approvals = new Map(objects(root.approvals).map(unbox).map((record) => [text(record.id), text(record.status, "unknown")]));
  const rawItems = hasAggregate ? objects(projection.work_items) : objects(root.work_items);
  const rawMilestones = hasAggregate ? objects(projection.milestones) : objects(root.milestones);

  const milestoneRecords = rawMilestones.map((entry) => {
    const boxed = entry.milestone && typeof entry.milestone === "object" && !Array.isArray(entry.milestone)
      ? entry.milestone as Json
      : entry;
    return { boxed, derived: entry };
  });
  const milestoneNames = new Map(milestoneRecords.map(({ boxed }) => [text(boxed.id), text(boxed.title, text(boxed.id))]));
  const items = rawItems.map(unbox).map((record): WorkRow => {
    const businessLineId = text(record.business_module_ref) || undefined;
    const milestoneId = text(record.milestone_ref) || undefined;
    const approvalRefs = Array.isArray(record.approval_refs) ? record.approval_refs.map((value) => text(value)).filter(Boolean) : [];
    const assigneeValues = Array.isArray(record.assignees)
      ? record.assignees
      : Array.isArray(record.assignee_refs) ? record.assignee_refs : [];
    const contributorValues = Array.isArray(record.contributors)
      ? record.contributors
      : Array.isArray(record.contributor_refs) ? record.contributor_refs : [];
    const executionRefs = objects(record.execution_refs);
    return {
      id: text(record.id, "unresolved-work"),
      title: text(record.title, "Untitled WorkItem"),
      objective: text(record.objective),
      description: text(record.description) || undefined,
      acceptanceCriteria: Array.isArray(record.acceptance_criteria) ? record.acceptance_criteria.map((value) => text(value)).filter(Boolean) : [],
      contextRefs: entityLinks(record.context_refs),
      deliverableRefs: entityLinks(record.deliverable_refs),
      status: text(record.status, "submitted"),
      workType: text(record.work_type, "general"),
      businessLineId,
      businessLine: businessLineId ? modules.get(businessLineId) ?? businessLineId : "Unclassified",
      milestoneId,
      milestone: milestoneId ? milestoneNames.get(milestoneId) ?? milestoneId : "No milestone",
      accountable: resolveActor(record.accountable_owner ?? record.accountable_owner_ref),
      assignees: assigneeValues.map(resolveActor).filter((actor): actor is ActorRow => Boolean(actor)),
      contributors: contributorValues.map(resolveActor).filter((actor): actor is ActorRow => Boolean(actor)),
      reviewer: resolveActor(record.reviewer ?? record.reviewer_ref),
      approvalId: approvalRefs[0],
      approval: approvalRefs.length === 0 ? "none" : approvals.get(approvalRefs[0]) ?? "linked",
      dueAt: text(record.due_at) || undefined,
      priority: text(record.priority) || undefined,
      source: (() => {
        const sourceId = text(record.source_document_ref);
        const resolved = documents.get(sourceId);
        return resolved?.title ?? (sourceId || "No source");
      })(),
      sourceId: text(record.source_document_ref) || undefined,
      sourceLifecycle: (() => {
        const sourceId = text(record.source_document_ref);
        if (!sourceId) return undefined;
        return documents.get(sourceId)?.lifecycle ?? "missing";
      })(),
      execution: executionRefs.length > 0 ? text(executionRefs[0].kind, "linked") : text(record.execution_mode, "direct"),
      updatedAt: text(record.updated_at),
    };
  });
  const milestones = milestoneRecords.map(({ boxed, derived }): MilestoneRow => {
    const id = text(boxed.id, "unresolved-milestone");
    const linked = items.filter((item) => item.milestoneId === id);
    const total = hasAggregate ? finiteNumber(derived.total_work_items) ?? 0 : linked.length;
    const completed = hasAggregate ? finiteNumber(derived.completed_work_items) ?? 0 : linked.filter((item) => item.status === "completed").length;
    return {
      id,
      title: text(boxed.title, "Untitled milestone"),
      outcome: text(boxed.outcome),
      status: text(boxed.status, "planned"),
      targetAt: text(boxed.target_at) || undefined,
      accountable: resolveActor(boxed.accountable_owner),
      total,
      completed,
      blocked: hasAggregate ? finiteNumber(derived.blocked_work_items) ?? 0 : linked.filter((item) => item.status === "blocked").length,
      waiting: hasAggregate ? finiteNumber(derived.waiting_for_approval_work_items) ?? 0 : linked.filter((item) => item.status === "waiting_for_approval").length,
      progress: hasAggregate ? finiteNumber(derived.progress_percent) ?? 0 : (total > 0 ? Math.floor(completed * 100 / total) : 0),
    };
  });
  const itemById = new Map(items.map((item) => [item.id, item]));
  const dimension = (name: "board" | "work_types"): WorkDimensionRow[] => {
    if (!hasAggregate) {
      const keyFor = (item: WorkRow) => name === "board"
        ? item.status
        : item.workType;
      const grouped = new Map<string, string[]>();
      for (const item of items) {
        const key = keyFor(item);
        const refs = grouped.get(key) ?? [];
        refs.push(item.id);
        grouped.set(key, refs);
      }
      return [...grouped].map(([id, workItemIds]) => ({ id, label: id, workItemIds }));
    }
    const raw = object(projection[name]);
    return Object.entries(raw).map(([id, refs]) => {
      const workItemIds = stringArray(refs);
      for (const workItemId of workItemIds) {
        if (!itemById.has(workItemId)) integrity.push(`${name}.${id} references missing WorkItem ${workItemId}.`);
      }
      return { id, label: id, workItemIds };
    });
  };
  const board = dimension("board");
  const rawBusinessLines = hasAggregate
    ? object(projection.business_lines)
    : items.reduce<Record<string, string[]>>((lines, item) => {
        const id = item.businessLineId ?? "unclassified";
        (lines[id] ??= []).push(item.id);
        return lines;
      }, {});
  const businessLineProjection = projectBusinessLineDimensions(rawBusinessLines, items, modules);
  integrity.push(...businessLineProjection.integrityFindings);
  const businessLines = businessLineProjection.dimensions;
  const workTypes = dimension("work_types");
  const workload = hasAggregate
    ? objects(projection.workload).map((entry): WorkloadRow => {
        const actorId = refId(entry.actor);
        const workItemIds = stringArray(entry.work_item_refs);
        if (!actorId) integrity.push("company_os.work workload row lacks an actor reference.");
        for (const workItemId of workItemIds) {
          if (!itemById.has(workItemId)) integrity.push(`workload ${actorId || "unknown"} references missing WorkItem ${workItemId}.`);
        }
        return {
          actorId,
          actor: resolveActor(entry.actor),
          accountable: finiteNumber(entry.accountable_count) ?? 0,
          assigned: finiteNumber(entry.assigned_count) ?? 0,
          active: finiteNumber(entry.active_count) ?? 0,
          workItemIds,
        };
      }).filter((entry) => entry.actorId)
    : [];
  const summaryRecord = object(projection.summary);
  const summaryValue = (key: string, legacy: number): number => {
    if (!hasAggregate) return legacy;
    const value = finiteNumber(summaryRecord[key]);
    if (value === undefined) {
      integrity.push(`company_os.work summary.${key} is unavailable.`);
      return 0;
    }
    return value;
  };
  if (hasAggregate && finiteNumber(summaryRecord.total) !== undefined && finiteNumber(summaryRecord.total) !== items.length) {
    integrity.push(`company_os.work summary.total=${summaryRecord.total} does not match ${items.length} projected WorkItems.`);
  }
  return {
    provenance: hasAggregate ? "company_os.work" : "legacy_raw_records",
    integrity,
    items,
    milestones,
    actors: [...actorMap.values()],
    board,
    businessLines,
    workTypes,
    workload,
    summary: {
      total: summaryValue("total", items.length),
      active: summaryValue("active", items.filter((item) => isActiveWork(item.status)).length),
      completed: summaryValue("completed", items.filter((item) => item.status === "completed").length),
      blocked: summaryValue("blocked", items.filter((item) => item.status === "blocked").length),
      waiting: summaryValue("waiting_for_approval", items.filter((item) => item.status === "waiting_for_approval").length),
      unassigned: summaryValue("unassigned", items.filter((item) => item.assignees.length === 0).length),
    },
  };
}

const viewOptions: Array<{ id: WorkView; label: string; icon: typeof LayoutDashboard }> = [
  { id: "overview", label: "Overview", icon: LayoutDashboard },
  { id: "board", label: "Board", icon: Columns3 },
  { id: "all", label: "All Work", icon: Table2 },
  { id: "milestones", label: "Milestones", icon: Flag },
  { id: "timeline", label: "Timeline", icon: CalendarDays },
  { id: "workload", label: "Workload", icon: UsersRound },
];

export function WorkOperatingPage({ source }: { source: unknown }) {
  const model = useMemo(() => buildModel(source), [source]);
  const [activeView, setActiveView] = useState<WorkView>("overview");
  const [query, setQuery] = useState("");
  const visible = model.items.filter((item) => `${item.title} ${item.objective} ${item.description ?? ""} ${item.acceptanceCriteria.join(" ")} ${item.businessLine} ${item.workType} ${item.milestone}`.toLowerCase().includes(query.toLowerCase()));
  return (
    <main className="h-full w-full min-w-0 max-w-full overflow-x-hidden overflow-y-auto bg-[radial-gradient(circle_at_78%_-5%,hsl(var(--primary)/0.09),transparent_28%),linear-gradient(to_bottom,hsl(var(--background)),hsl(var(--muted)/0.24))]" data-work-operating-system="v1" data-work-view={activeView} data-work-provenance={model.provenance}>
      <header className="sticky top-0 z-20 w-full min-w-0 max-w-full overflow-hidden border-b border-border/80 bg-background/90 px-4 py-4 backdrop-blur-xl sm:px-7">
        <div className="mx-auto flex max-w-[1500px] flex-wrap items-end justify-between gap-4">
          <div className="flex items-center gap-4">
            <ObjectEmblem kind="work" className="size-12 rounded-2xl shadow-sm" />
            <div className="min-w-0"><p className="text-[10px] font-semibold uppercase tracking-[0.22em] text-primary">Company operating ledger · {model.provenance === "company_os.work" ? "Store aggregate" : "Prototype compatibility"}</p><h1 className="company-editorial-title mt-1 text-3xl">Work</h1><p className="mt-1 max-w-64 text-xs leading-5 text-muted-foreground sm:max-w-none">One WorkItem truth across business lines, milestones, people, and Agents.</p></div>
          </div>
          <div className="flex w-full min-w-0 items-center gap-2 sm:w-auto">
            <label className="flex h-10 min-w-0 flex-1 items-center gap-2 rounded-xl border border-border bg-card/80 px-3 text-sm shadow-sm sm:min-w-56"><Search className="size-4 shrink-0 text-muted-foreground" /><span className="sr-only">Search work</span><input value={query} onChange={(event) => setQuery(event.target.value)} placeholder="Search WorkItems…" className="min-w-0 flex-1 bg-transparent outline-none placeholder:text-muted-foreground" /></label>
            <button type="button" className="hidden size-10 place-items-center rounded-xl border border-border bg-card text-muted-foreground shadow-sm sm:grid" aria-label="Filters"><Filter className="size-4" /></button>
            <button type="button" disabled aria-label="New work unavailable" title="No governed WorkItem creation transport is connected." className="hidden h-10 shrink-0 cursor-not-allowed items-center justify-center gap-2 rounded-xl border border-border bg-muted/70 px-4 text-sm font-semibold text-muted-foreground opacity-60 sm:inline-flex"><Sparkles className="size-4" />New work</button>
          </div>
        </div>
        <nav className="mx-auto mt-4 flex w-full min-w-0 max-w-[1500px] gap-1 overflow-x-auto" aria-label="Work views">{viewOptions.map(({ id, label, icon: Icon }) => <button key={id} type="button" onClick={() => setActiveView(id)} className={`inline-flex shrink-0 items-center gap-2 rounded-lg px-3 py-2 text-xs font-medium transition ${activeView === id ? "bg-primary/10 text-primary" : "text-muted-foreground hover:bg-muted"}`} aria-current={activeView === id ? "page" : undefined}><Icon className="size-3.5" />{label}</button>)}</nav>
      </header>
      <div className="mx-auto w-full min-w-0 max-w-[1500px] overflow-hidden p-4 sm:p-7">
        {model.integrity.length > 0 && <WorkIntegrity findings={model.integrity} />}
        {model.items.length === 0 ? <EmptyWork provenance={model.provenance} /> : activeView === "overview" ? <Overview model={model} items={visible} /> : activeView === "board" ? <Board model={model} items={visible} /> : activeView === "all" ? <AllWork items={visible} /> : activeView === "milestones" ? <Milestones milestones={model.milestones} items={visible} /> : activeView === "timeline" ? <Timeline items={visible} /> : <Workload model={model} items={visible} />}
      </div>
    </main>
  );
}

function Overview({ model, items }: { model: WorkModel; items: WorkRow[] }) {
  const attention = items.filter((item) => item.status === "blocked" || item.status === "waiting_for_approval" || item.assignees.length === 0);
  const activeItems = items.filter((item) => isActiveWork(item.status));
  const itemById = new Map(items.map((item) => [item.id, item]));
  const operatingQueue = (attention.length > 0 ? attention : activeItems)
    .sort((left, right) => priorityWeight(right.priority) - priorityWeight(left.priority) || right.updatedAt.localeCompare(left.updatedAt))
    .slice(0, 10);
  const businessLines = model.businessLines.map((dimension) => {
    const linked = dimension.workItemIds.map((id) => itemById.get(id)).filter((item): item is WorkRow => Boolean(item));
    return {
      line: dimension.label,
      count: linked.length,
      active: linked.filter((item) => isActiveWork(item.status)).length,
    };
  }).filter((dimension) => dimension.count > 0);
  const workTypes = model.workTypes.map((dimension) => ({
    type: dimension.id,
    count: dimension.workItemIds.filter((id) => itemById.has(id)).length,
  })).filter((dimension) => dimension.count > 0);
  const activeMilestones = [...model.milestones].sort((left, right) => {
    const statusWeight = (status: string) => status === "active" ? 3 : status === "at_risk" ? 2 : status === "planned" ? 1 : 0;
    return statusWeight(right.status) - statusWeight(left.status) || right.total - left.total;
  });
  return <div className="w-full min-w-0 max-w-full space-y-6 overflow-hidden">
    <section className="flex w-full min-w-0 max-w-full gap-3 overflow-x-auto sm:grid sm:grid-cols-3 sm:overflow-visible xl:grid-cols-6"><Metric label="Active" value={model.summary.active} icon={CircleDot} tone="primary" /><Metric label="Completed" value={model.summary.completed} icon={CheckCircle2} tone="good" /><Metric label="Blocked" value={model.summary.blocked} icon={AlertTriangle} tone="danger" /><Metric label="Waiting" value={model.summary.waiting} icon={Clock3} tone="warn" /><Metric label="Unassigned" value={model.summary.unassigned} icon={UserRound} tone="quiet" /><Metric label="All Work" value={model.summary.total} icon={ListChecks} tone="quiet" /></section>
    <div className="grid gap-6 xl:grid-cols-[minmax(0,1.45fr)_minmax(20rem,0.55fr)]">
      <section className="overflow-hidden rounded-2xl border border-border bg-card/85 shadow-sm"><SectionTitle eyebrow={attention.length > 0 ? "Operational pressure" : "Operating queue"} title={attention.length > 0 ? "Needs attention" : "Next WorkItems"} detail="The first screen shows what the company has committed to, who owns it, and which business line it affects." /><div className="divide-y divide-border">{operatingQueue.length > 0 ? operatingQueue.map((item) => <WorkListRow key={item.id} item={item} />) : <p className="p-6 text-sm text-muted-foreground">No active WorkItems match the current filter.</p>}</div></section>
      <div className="space-y-6">
        <section className="rounded-2xl border border-border bg-card/85 p-5 shadow-sm"><p className="text-[10px] font-semibold uppercase tracking-[0.18em] text-muted-foreground">Business lines</p><div className="mt-4 space-y-3">{businessLines.map(({ line, count, active }) => <div key={line}><div className="flex items-center justify-between gap-3 text-sm"><span className="min-w-0 truncate">{line}</span><span className="shrink-0 font-semibold">{active}/{count}</span></div><div className="mt-2 h-1.5 overflow-hidden rounded-full bg-muted"><div className="h-full rounded-full bg-primary" style={{ width: `${Math.max(8, count / Math.max(1, items.length) * 100)}%` }} /></div></div>)}</div></section>
        <section className="rounded-2xl border border-primary/20 bg-gradient-to-br from-primary/[0.08] to-card p-5 shadow-sm"><div className="flex items-center gap-2 text-primary"><Flag className="size-4" /><p className="text-[10px] font-semibold uppercase tracking-[0.18em]">Milestone pulse</p></div><div className="mt-4 space-y-3">{activeMilestones.slice(0, 3).map((milestone) => <div key={milestone.id} data-company-os-ref={milestone.id} className="rounded-xl border border-border/80 bg-background/60 p-3"><div className="flex items-center justify-between gap-3"><p className="min-w-0 truncate text-sm font-semibold">{milestone.title}</p><Status status={milestone.status} /></div><div className="mt-3 flex items-center justify-between text-[10px] text-muted-foreground"><span>{milestone.completed}/{milestone.total} completed</span><span>{milestone.progress}%</span></div><div className="mt-2 h-1.5 overflow-hidden rounded-full bg-muted"><div className="h-full rounded-full bg-gradient-to-r from-primary to-status-good" style={{ width: `${milestone.progress}%` }} /></div></div>)}</div><p className="mt-4 text-xs leading-5 text-muted-foreground">Milestones group WorkItems. They are not Project objects, Task Graphs, or Mission Waves.</p></section>
        <section className="rounded-2xl border border-border bg-card/85 p-5 shadow-sm"><p className="text-[10px] font-semibold uppercase tracking-[0.18em] text-muted-foreground">Work types</p><div className="mt-4 grid grid-cols-2 gap-2">{workTypes.slice(0, 8).map(({ type, count }) => <div key={type} className="rounded-lg border border-border/70 bg-background/55 p-3"><p className="truncate text-xs font-medium">{humanize(type)}</p><p className="mt-2 company-editorial-title text-xl">{count}</p></div>)}</div></section>
      </div>
    </div>
  </div>;
}

function Metric({ label, value, icon: Icon, tone }: { label: string; value: number; icon: typeof CircleDot; tone: "primary" | "good" | "danger" | "warn" | "quiet" }) {
  const colors = tone === "primary" ? "border-primary/25 bg-primary/[0.06] text-primary" : tone === "good" ? "border-status-good/25 bg-status-good/[0.06] text-status-good" : tone === "danger" ? "border-destructive/25 bg-destructive/[0.05] text-destructive" : tone === "warn" ? "border-status-warn/30 bg-status-warn/[0.06] text-status-warn" : "border-border bg-card/80 text-muted-foreground";
  return <div className={`w-40 shrink-0 overflow-hidden rounded-2xl border p-3 shadow-sm sm:w-auto sm:min-w-0 sm:p-4 ${colors}`}><div className="flex min-w-0 items-center justify-between gap-2"><p className="truncate text-[10px] font-semibold uppercase tracking-wider">{label}</p><Icon className="size-4 shrink-0" /></div><p className="company-editorial-title mt-3 text-2xl text-foreground sm:mt-4 sm:text-3xl">{value}</p></div>;
}

function SectionTitle({ eyebrow, title, detail }: { eyebrow: string; title: string; detail: string }) {
  return <header className="flex min-w-0 flex-wrap items-end justify-between gap-3 border-b border-border px-5 py-4"><div className="min-w-0"><p className="text-[10px] font-semibold uppercase tracking-[0.18em] text-primary">{eyebrow}</p><h2 className="company-editorial-title mt-1 text-2xl">{title}</h2></div><p className="hidden max-w-sm text-xs leading-5 text-muted-foreground sm:block">{detail}</p></header>;
}

function WorkListRow({ item }: { item: WorkRow }) {
  return <a href={workItemHref(item.id)} aria-label={`Open WorkItem ${item.title}`} className="group grid gap-4 p-5 transition hover:bg-muted/30 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-ring sm:grid-cols-[minmax(0,1fr)_auto]" data-company-os-ref={item.id} data-work-item-status={item.status}><div className="min-w-0"><div className="flex flex-wrap items-center gap-2"><Status status={item.status} /><span className="text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">{humanize(item.workType)}</span><span className="text-[10px] text-muted-foreground">· {item.businessLine}</span><AcceptanceCriteriaCount item={item} prefix="· " />{item.approvalId && <span data-company-os-ref={item.approvalId} className="text-[10px] text-status-warn">· {item.approval === "requested" ? "Human approval" : humanize(item.approval)}</span>}</div><h3 className="mt-2 truncate font-semibold">{item.title}</h3><p className="mt-1 line-clamp-2 text-xs leading-5 text-muted-foreground">{item.description || item.objective || `${item.milestone} · ${item.source}`}</p><p className="mt-1 truncate text-[10px] text-muted-foreground">{item.milestone} · {sourceLabel(item)}{item.contextRefs.length > 0 ? ` · ${item.contextRefs.length} context refs` : ""}{item.deliverableRefs.length > 0 ? ` · ${item.deliverableRefs.length} deliverables` : ""}</p></div><div className="flex items-center gap-4"><ActorStack actors={[item.accountable, ...item.assignees, ...item.contributors, item.reviewer]} /><div className="hidden text-right sm:block"><p className="text-xs font-medium">{dateLabel(item.dueAt)}</p><p className="mt-1 text-[10px] text-muted-foreground">{item.approval === "requested" ? "Human approval" : humanize(item.execution)}</p></div><ArrowUpRight className="size-4 text-muted-foreground transition group-hover:text-primary" /></div></a>;
}

function AcceptanceCriteriaCount({ item, prefix = "", className = "" }: { item: WorkRow; prefix?: string; className?: string }) {
  if (item.acceptanceCriteria.length === 0) return null;
  const presentation = acceptanceCriteriaPresentation(item.acceptanceCriteria.length, item.status);
  return <span data-work-acceptance-semantic={presentation.semantic} data-work-item-status={presentation.workItemStatus} data-work-acceptance-tone={presentation.tone} className={`text-[10px] text-muted-foreground ${className}`}>{prefix}{presentation.label}</span>;
}

function Board({ model, items }: { model: WorkModel; items: WorkRow[] }) {
  const visibleById = new Map(items.map((item) => [item.id, item]));
  return <div className="w-full overflow-x-auto pb-4"><div className="grid min-w-max gap-3" style={{ gridTemplateColumns: `repeat(${Math.max(1, model.board.length)}, minmax(10.5rem, 1fr))` }}>{model.board.map((column) => { const rows = column.workItemIds.map((id) => visibleById.get(id)).filter((item): item is WorkRow => Boolean(item)); const status = column.id; return <section key={status} data-work-board-column={status} className="min-h-[610px] w-[11rem] rounded-xl border border-border/80 bg-card/35 p-2.5 sm:w-[12rem]"><header className="flex items-center justify-between px-1 pb-2.5"><div className="flex items-center gap-2"><span className={`size-1.5 rounded-full ${status === "blocked" ? "bg-destructive" : status === "completed" ? "bg-status-good" : status === "waiting_for_approval" ? "bg-status-warn" : "bg-primary/70"}`} /><h2 className="text-[11px] font-semibold">{humanize(status)}</h2></div><span className="text-[10px] text-muted-foreground">{rows.length}</span></header><div className="space-y-2.5">{rows.map((item) => <WorkCard key={item.id} item={item} />)}{rows.length === 0 && <div className="rounded-lg border border-dashed border-border/70 p-4 text-center text-[10px] text-muted-foreground">No matching work</div>}</div></section>; })}{model.board.length === 0 && <EmptyPanel title="No board columns" body="company_os.work.board is empty; the UI does not invent lifecycle lanes." />}</div></div>;
}

function WorkCard({ item }: { item: WorkRow }) {
  return <a href={workItemHref(item.id)} aria-label={`Open WorkItem ${item.title}`} className="block rounded-lg border border-border bg-card p-3 shadow-[0_1px_2px_hsl(var(--foreground)/0.04)] transition hover:-translate-y-0.5 hover:border-primary/30 hover:shadow-md focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring" data-company-os-ref={item.id} data-work-item-status={item.status}><div className="flex items-center justify-between gap-2"><span className="rounded-md border border-primary/15 bg-primary/[0.06] px-1.5 py-0.5 text-[9px] font-medium text-primary">{humanize(item.workType)}</span>{item.priority && <span className="text-[9px] text-muted-foreground">{humanize(item.priority)}</span>}</div><h3 className="mt-2.5 text-[13px] font-semibold leading-[1.35]">{item.title}</h3><p className="mt-2 line-clamp-2 text-[10px] leading-4 text-muted-foreground">{item.description || item.objective || item.businessLine}</p><div className="mt-3 border-t border-border/70 pt-3"><p className="text-[9px] uppercase tracking-wide text-muted-foreground">Accountable</p><div className="mt-1.5 flex items-center justify-between gap-2"><ActorStack actors={[item.accountable, ...item.assignees]} /><span className="max-w-20 truncate text-[9px] text-muted-foreground">{item.accountable?.name ?? "Unassigned"}</span></div><div className="mt-2 space-y-1 text-[9px] text-muted-foreground"><p className="truncate">⚑ {item.milestone}</p><p>□ {dateLabel(item.dueAt)}</p><AcceptanceCriteriaCount item={item} className="block text-[9px]" />{item.approvalId && <p data-company-os-ref={item.approvalId} className="text-status-warn">Human approval</p>}</div></div></a>;
}

function AllWork({ items }: { items: WorkRow[] }) {
  return <section className="overflow-hidden rounded-2xl border border-border bg-card/90 shadow-sm"><SectionTitle eyebrow="Canonical ledger" title="All WorkItems" detail={`${items.length} records · sortable projection`} /><div className="overflow-x-auto"><table className="min-w-[1380px] w-full text-left text-xs"><thead className="bg-muted/45 text-[10px] uppercase tracking-wider text-muted-foreground"><tr>{["WorkItem", "Detail", "Type", "Business line", "Milestone", "Status", "Accountable", "Assignees", "Acceptance", "Refs", "Due", "Execution"].map((label) => <th key={label} className="px-4 py-3 font-semibold">{label}</th>)}</tr></thead><tbody className="divide-y divide-border">{items.map((item) => <tr key={item.id} className="hover:bg-muted/25" data-company-os-ref={item.id}><td className="max-w-xs px-4 py-4"><a href={workItemHref(item.id)} className="font-semibold text-foreground hover:text-primary focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring">{item.title}</a><p className="mt-1 truncate text-[10px] text-muted-foreground" data-work-source-lifecycle={item.sourceLifecycle}>{item.sourceId ? <a href={documentHref(item.sourceId)} data-company-os-ref={item.sourceId} className="hover:text-primary focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring">{sourceLabel(item)}</a> : sourceLabel(item)}</p></td><td className="max-w-sm px-4 py-4 text-muted-foreground"><p className="line-clamp-2">{item.description || item.objective || "No detail supplied"}</p></td><td className="px-4 py-4">{humanize(item.workType)}</td><td className="px-4 py-4">{item.businessLine}</td><td className="px-4 py-4">{item.milestone}</td><td className="px-4 py-4"><Status status={item.status} /></td><td className="px-4 py-4">{item.accountable?.name ?? "Unassigned"}</td><td className="px-4 py-4">{item.assignees.map((actor) => actor.name).join(", ") || "Unassigned"}</td><td className="px-4 py-4">{item.acceptanceCriteria.length > 0 ? <AcceptanceCriteriaCount item={item} /> : "—"}</td><td className="px-4 py-4">{item.contextRefs.length || item.deliverableRefs.length ? `${item.contextRefs.length} context / ${item.deliverableRefs.length} deliverable` : "—"}</td><td className="px-4 py-4">{dateLabel(item.dueAt)}</td><td className="px-4 py-4">{humanize(item.execution)}</td></tr>)}</tbody></table></div></section>;
}

function Milestones({ milestones, items }: { milestones: MilestoneRow[]; items: WorkRow[] }) {
  const unassigned = items.filter((item) => !item.milestoneId);
  return <div className="space-y-5">{milestones.map((milestone) => <section key={milestone.id} className="overflow-hidden rounded-2xl border border-border bg-card/90 shadow-sm" data-company-os-ref={milestone.id}><header className="grid gap-5 border-b border-border p-5 lg:grid-cols-[minmax(0,1fr)_18rem]"><div><div className="flex items-center gap-2"><Flag className="size-4 text-primary" /><Status status={milestone.status} /></div><h2 className="company-editorial-title mt-3 text-2xl">{milestone.title}</h2><p className="mt-2 max-w-2xl text-sm leading-6 text-muted-foreground">{milestone.outcome}</p></div><div><div className="flex items-center justify-between text-xs"><span>{milestone.completed}/{milestone.total} completed</span><strong>{milestone.progress}%</strong></div><div className="mt-3 h-2 overflow-hidden rounded-full bg-muted"><div className="h-full rounded-full bg-gradient-to-r from-primary to-status-good" style={{ width: `${milestone.progress}%` }} /></div><div className="mt-4 flex justify-between text-[10px] text-muted-foreground"><span>{milestone.accountable?.name ?? "No owner"}</span><span>{dateLabel(milestone.targetAt)}</span></div></div></header><div className="divide-y divide-border">{items.filter((item) => item.milestoneId === milestone.id).map((item) => <WorkListRow key={item.id} item={item} />)}</div></section>)}{milestones.length === 0 && <EmptyPanel title="No native Milestones" body="WorkItems remain visible without inventing a checkpoint." />}{unassigned.length > 0 && <section className="overflow-hidden rounded-2xl border border-dashed border-border bg-card/60"><SectionTitle eyebrow="Honest gap" title="No milestone" detail={`${unassigned.length} WorkItems`} /><div className="divide-y divide-border">{unassigned.map((item) => <WorkListRow key={item.id} item={item} />)}</div></section>}</div>;
}

function Timeline({ items }: { items: WorkRow[] }) {
  const dated = [...items].sort((a, b) => (a.dueAt ?? "9999").localeCompare(b.dueAt ?? "9999"));
  return <section className="rounded-2xl border border-border bg-card/90 p-5 shadow-sm"><div className="relative ml-3 border-l border-border pl-7">{dated.map((item) => <article key={item.id} className="relative pb-7 last:pb-0"><span className="absolute -left-[2.1rem] top-1 grid size-4 place-items-center rounded-full border-2 border-card bg-primary"><span className="size-1 rounded-full bg-primary-foreground" /></span><div className="grid gap-3 rounded-xl border border-border bg-background/70 p-4 sm:grid-cols-[8rem_minmax(0,1fr)_auto]"><div><p className="text-xs font-semibold">{dateLabel(item.dueAt)}</p><p className="mt-1 text-[10px] text-muted-foreground">{item.milestone}</p></div><div><h3 className="font-semibold">{item.title}</h3><p className="mt-1 text-xs text-muted-foreground">{item.businessLine} · {humanize(item.workType)}</p></div><div className="flex items-center gap-3"><Status status={item.status} /><ActorStack actors={[item.accountable, ...item.assignees]} /></div></div></article>)}</div></section>;
}

function Workload({ model, items }: { model: WorkModel; items: WorkRow[] }) {
  const visibleById = new Map(items.map((item) => [item.id, item]));
  return <div className="grid gap-5 xl:grid-cols-2">{model.workload.map((row) => {
    const actor = row.actor ?? { id: row.actorId, name: row.actorId, kind: "service", role: "Unresolved actor reference" };
    const linked = row.workItemIds.map((id) => visibleById.get(id)).filter((item): item is WorkRow => Boolean(item));
    return <section key={row.actorId} data-workload-actor={row.actorId} className="rounded-2xl border border-border bg-card/90 p-5 shadow-sm"><header className="flex items-center justify-between gap-4"><div className="flex items-center gap-3"><ActorAvatar identity={`${actor.id} ${actor.role}`} name={actor.name} size="lg" /><div><h2 className="font-semibold">{actor.name}</h2><p className="mt-1 text-xs text-muted-foreground">{actor.role}</p></div></div><div className="text-right"><p className="company-editorial-title text-2xl">{row.active}</p><p className="text-[10px] text-muted-foreground">aggregate active</p></div></header><div className="mt-4 grid grid-cols-2 gap-2 text-xs"><div className="rounded-lg bg-muted/50 p-3"><span className="text-muted-foreground">Accountable</span><strong className="float-right">{row.accountable}</strong></div><div className="rounded-lg bg-muted/50 p-3"><span className="text-muted-foreground">Assigned</span><strong className="float-right">{row.assigned}</strong></div></div><div className="mt-4 space-y-2">{linked.slice(0, 3).map((item) => <div key={item.id} className="flex items-center justify-between gap-3 rounded-lg border border-border p-3 text-xs"><span className="truncate font-medium">{item.title}</span><Status status={item.status} /></div>)}{linked.length === 0 && <p className="text-xs text-muted-foreground">No linked WorkItems match the current filter.</p>}</div></section>;
  })}{model.summary.unassigned > 0 && <section className="rounded-2xl border border-dashed border-status-warn/40 bg-status-warn/[0.04] p-5"><div className="flex items-center gap-2 text-status-warn"><AlertTriangle className="size-4" /><h2 className="font-semibold">Unassigned lane</h2></div><p className="mt-2 text-xs text-muted-foreground">{model.summary.unassigned} WorkItems are unassigned in aggregate truth.</p></section>}{model.workload.length === 0 && model.summary.unassigned === 0 && <EmptyPanel title="No workload rows" body="company_os.work.workload is empty; responsibility is not inferred from names or display order." />}</div>;
}

function Status({ status }: { status: string }) {
  const tone = status === "completed" || status === "achieved" ? "border-status-good/25 bg-status-good/10 text-status-good" : status === "blocked" || status === "at_risk" ? "border-destructive/25 bg-destructive/8 text-destructive" : status === "waiting_for_approval" ? "border-status-warn/30 bg-status-warn/10 text-status-warn" : "border-primary/20 bg-primary/8 text-primary";
  return <span className={`inline-flex w-fit items-center rounded-full border px-2 py-1 text-[9px] font-semibold uppercase tracking-wider ${tone}`}>{humanize(status)}</span>;
}

function ActorStack({ actors }: { actors: Array<ActorRow | undefined> }) {
  const unique = actors.filter((actor): actor is ActorRow => Boolean(actor)).filter((actor, index, all) => all.findIndex((candidate) => candidate.id === actor.id) === index).slice(0, 4);
  return <div className="flex -space-x-2">{unique.map((actor) => <span key={actor.id} data-company-os-ref={actor.id} data-actor-type={actor.kind} className="rounded-full"><ActorAvatar identity={`${actor.id} ${actor.role}`} name={actor.name} size="sm" ring={actor.kind === "human" ? "warm" : actor.kind === "agent" ? "good" : "neutral"} /></span>)}</div>;
}

function WorkIntegrity({ findings }: { findings: string[] }) { return <div role="alert" data-work-integrity-count={findings.length} className="mb-5 rounded-xl border border-status-warn/35 bg-status-warn/[0.05] p-4"><div className="flex items-center gap-2 text-sm font-semibold text-status-warn"><AlertTriangle className="size-4" />Work projection integrity ({findings.length})</div><ul className="mt-3 space-y-1 text-xs leading-5 text-muted-foreground">{findings.map((finding) => <li key={finding}>{finding}</li>)}</ul></div>; }
function EmptyWork({ provenance }: { provenance: WorkModel["provenance"] }) { return <EmptyPanel title="No WorkItems in aggregate truth" body={provenance === "company_os.work" ? "company_os.work is empty. Raw WorkItem rows are not used as a first-row or list fallback." : "The prototype projection contains no WorkItems; no company commitment is invented."} />; }
function EmptyPanel({ title, body }: { title: string; body: string }) { return <div className="grid min-h-80 place-items-center rounded-2xl border border-dashed border-border bg-card/50 p-8 text-center"><div><Bot className="mx-auto size-8 text-primary" /><h2 className="company-editorial-title mt-4 text-2xl">{title}</h2><p className="mx-auto mt-2 max-w-md text-sm leading-6 text-muted-foreground">{body}</p></div></div>; }
