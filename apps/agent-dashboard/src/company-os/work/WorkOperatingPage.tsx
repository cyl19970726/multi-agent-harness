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

type Json = Record<string, unknown>;
type WorkView = "overview" | "board" | "all" | "milestones" | "timeline" | "workload";

interface ActorRow {
  id: string;
  name: string;
  kind: string;
  role: string;
}

interface WorkRow {
  id: string;
  title: string;
  objective: string;
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

interface WorkModel {
  items: WorkRow[];
  milestones: MilestoneRow[];
  actors: ActorRow[];
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

function refId(value: unknown): string {
  if (typeof value === "string") return value;
  if (!value || typeof value !== "object" || Array.isArray(value)) return "";
  const ref = value as Json;
  return text(ref.actor_id) || text(ref.id);
}

function humanize(value: string): string {
  return value.replace(/[_-]+/g, " ").replace(/\b\w/g, (letter) => letter.toUpperCase());
}

function dateLabel(value?: string): string {
  if (!value) return "No due date";
  const match = value.match(/^(\d{4})-(\d{2})-(\d{2})/);
  if (!match) return value;
  const [, year, month, day] = match;
  return `${day}/${month}/${year}`;
}

function buildModel(source: unknown): WorkModel {
  const root = source && typeof source === "object" && !Array.isArray(source) ? source as Json : {};
  const projection = root.work && typeof root.work === "object" && !Array.isArray(root.work) ? root.work as Json : {};
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
    return actorMap.get(id) ?? { id, name: humanize(id.replace(/^actor-/, "")), kind: text((value as Json | undefined)?.actor_type, "service"), role: "Linked actor" };
  };
  const modules = new Map(objects(root.business_modules).map(unbox).map((record) => [text(record.id), text(record.name, text(record.title, text(record.id))) ]));
  const documents = new Map(objects(root.documents).map(unbox).map((record) => [text(record.id), text(record.title, text(record.id))]));
  const approvals = new Map(objects(root.approvals).map(unbox).map((record) => [text(record.id), text(record.status, "unknown")]));
  const rawItems = objects(projection.work_items).length > 0 ? objects(projection.work_items) : objects(root.work_items);
  const rawMilestones = objects(projection.milestones).length > 0 ? objects(projection.milestones) : objects(root.milestones);

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
      source: documents.get(text(record.source_document_ref)) ?? text(record.source_document_ref, "No source"),
      execution: executionRefs.length > 0 ? text(executionRefs[0].kind, "linked") : text(record.execution_mode, "direct"),
      updatedAt: text(record.updated_at),
    };
  });
  const milestones = milestoneRecords.map(({ boxed, derived }): MilestoneRow => {
    const id = text(boxed.id, "unresolved-milestone");
    const linked = items.filter((item) => item.milestoneId === id);
    const total = Number(derived.total_work_items ?? linked.length);
    const completed = Number(derived.completed_work_items ?? linked.filter((item) => item.status === "completed").length);
    return {
      id,
      title: text(boxed.title, "Untitled milestone"),
      outcome: text(boxed.outcome),
      status: text(boxed.status, "planned"),
      targetAt: text(boxed.target_at) || undefined,
      accountable: resolveActor(boxed.accountable_owner),
      total,
      completed,
      blocked: Number(derived.blocked_work_items ?? linked.filter((item) => item.status === "blocked").length),
      waiting: Number(derived.waiting_for_approval_work_items ?? linked.filter((item) => item.status === "waiting_for_approval").length),
      progress: Number(derived.progress_percent ?? (total > 0 ? Math.floor(completed * 100 / total) : 0)),
    };
  });
  const summaryRecord = projection.summary && typeof projection.summary === "object" ? projection.summary as Json : {};
  const isActive = (status: string) => !new Set(["draft", "completed", "cancelled", "archived"]).has(status);
  return {
    items,
    milestones,
    actors: [...actorMap.values()],
    summary: {
      total: Number(summaryRecord.total ?? items.length),
      active: Number(summaryRecord.active ?? items.filter((item) => isActive(item.status)).length),
      completed: Number(summaryRecord.completed ?? items.filter((item) => item.status === "completed").length),
      blocked: Number(summaryRecord.blocked ?? items.filter((item) => item.status === "blocked").length),
      waiting: Number(summaryRecord.waiting_for_approval ?? items.filter((item) => item.status === "waiting_for_approval").length),
      unassigned: Number(summaryRecord.unassigned ?? items.filter((item) => item.assignees.length === 0).length),
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
  const [activeView, setActiveView] = useState<WorkView>("board");
  const [query, setQuery] = useState("");
  const visible = model.items.filter((item) => `${item.title} ${item.businessLine} ${item.workType} ${item.milestone}`.toLowerCase().includes(query.toLowerCase()));
  return (
    <main className="h-full w-full min-w-0 max-w-full overflow-x-hidden overflow-y-auto bg-[radial-gradient(circle_at_78%_-5%,hsl(var(--primary)/0.09),transparent_28%),linear-gradient(to_bottom,hsl(var(--background)),hsl(var(--muted)/0.24))]" data-work-operating-system="v1" data-work-view={activeView}>
      <header className="sticky top-0 z-20 w-full min-w-0 max-w-full overflow-hidden border-b border-border/80 bg-background/90 px-4 py-4 backdrop-blur-xl sm:px-7">
        <div className="mx-auto flex max-w-[1500px] flex-wrap items-end justify-between gap-4">
          <div className="flex items-center gap-4">
            <ObjectEmblem kind="work" className="size-12 rounded-2xl shadow-sm" />
            <div className="min-w-0"><p className="text-[10px] font-semibold uppercase tracking-[0.22em] text-primary">Company operating ledger</p><h1 className="company-editorial-title mt-1 text-3xl">Work</h1><p className="mt-1 max-w-64 text-xs leading-5 text-muted-foreground sm:max-w-none">One WorkItem truth across business lines, milestones, people, and Agents.</p></div>
          </div>
          <div className="flex w-full min-w-0 items-center gap-2 sm:w-auto">
            <label className="flex h-10 min-w-0 flex-1 items-center gap-2 rounded-xl border border-border bg-card/80 px-3 text-sm shadow-sm sm:min-w-56"><Search className="size-4 shrink-0 text-muted-foreground" /><span className="sr-only">Search work</span><input value={query} onChange={(event) => setQuery(event.target.value)} placeholder="Search WorkItems…" className="min-w-0 flex-1 bg-transparent outline-none placeholder:text-muted-foreground" /></label>
            <button type="button" className="hidden size-10 place-items-center rounded-xl border border-border bg-card text-muted-foreground shadow-sm sm:grid" aria-label="Filters"><Filter className="size-4" /></button>
            <button type="button" disabled aria-label="New work" className="hidden h-10 shrink-0 cursor-not-allowed items-center justify-center gap-2 rounded-xl bg-primary px-4 text-sm font-semibold text-primary-foreground opacity-80 sm:inline-flex"><Sparkles className="size-4" />New work</button>
          </div>
        </div>
        <nav className="mx-auto mt-4 flex w-full min-w-0 max-w-[1500px] gap-1 overflow-x-auto" aria-label="Work views">{viewOptions.map(({ id, label, icon: Icon }) => <button key={id} type="button" onClick={() => setActiveView(id)} className={`inline-flex shrink-0 items-center gap-2 rounded-lg px-3 py-2 text-xs font-medium transition ${activeView === id ? "bg-primary/10 text-primary" : "text-muted-foreground hover:bg-muted"}`} aria-current={activeView === id ? "page" : undefined}><Icon className="size-3.5" />{label}</button>)}</nav>
      </header>
      <div className="mx-auto w-full min-w-0 max-w-[1500px] overflow-hidden p-4 sm:p-7">
        {model.items.length === 0 ? <EmptyWork /> : activeView === "overview" ? <Overview model={model} items={visible} /> : activeView === "board" ? <Board items={visible} /> : activeView === "all" ? <AllWork items={visible} /> : activeView === "milestones" ? <Milestones milestones={model.milestones} items={visible} /> : activeView === "timeline" ? <Timeline items={visible} /> : <Workload items={visible} />}
      </div>
    </main>
  );
}

function Overview({ model, items }: { model: WorkModel; items: WorkRow[] }) {
  const attention = items.filter((item) => item.status === "blocked" || item.status === "waiting_for_approval" || item.assignees.length === 0);
  const businessLines = [...new Set(items.map((item) => item.businessLine))];
  return <div className="w-full min-w-0 max-w-full space-y-6 overflow-hidden">
    <section className="flex w-full min-w-0 max-w-full gap-3 overflow-x-auto sm:grid sm:grid-cols-3 sm:overflow-visible xl:grid-cols-6"><Metric label="Active" value={model.summary.active} icon={CircleDot} tone="primary" /><Metric label="Completed" value={model.summary.completed} icon={CheckCircle2} tone="good" /><Metric label="Blocked" value={model.summary.blocked} icon={AlertTriangle} tone="danger" /><Metric label="Waiting" value={model.summary.waiting} icon={Clock3} tone="warn" /><Metric label="Unassigned" value={model.summary.unassigned} icon={UserRound} tone="quiet" /><Metric label="All Work" value={model.summary.total} icon={ListChecks} tone="quiet" /></section>
    <div className="grid gap-6 xl:grid-cols-[minmax(0,1.45fr)_minmax(20rem,0.55fr)]">
      <section className="overflow-hidden rounded-2xl border border-border bg-card/85 shadow-sm"><SectionTitle eyebrow="Operational pressure" title="Needs attention" detail="The next actor and business consequence stay visible." /><div className="divide-y divide-border">{attention.length > 0 ? attention.map((item) => <WorkListRow key={item.id} item={item} />) : <p className="p-6 text-sm text-muted-foreground">No blocked, approval-bound, or unassigned WorkItems.</p>}</div></section>
      <div className="space-y-6"><section className="rounded-2xl border border-border bg-card/85 p-5 shadow-sm"><p className="text-[10px] font-semibold uppercase tracking-[0.18em] text-muted-foreground">Business lines</p><div className="mt-4 space-y-3">{businessLines.map((line) => { const count = items.filter((item) => item.businessLine === line).length; return <div key={line}><div className="flex items-center justify-between text-sm"><span>{line}</span><span className="font-semibold">{count}</span></div><div className="mt-2 h-1.5 overflow-hidden rounded-full bg-muted"><div className="h-full rounded-full bg-primary" style={{ width: `${Math.max(8, count / Math.max(1, items.length) * 100)}%` }} /></div></div>; })}</div></section><section className="rounded-2xl border border-primary/20 bg-gradient-to-br from-primary/[0.08] to-card p-5 shadow-sm"><div className="flex items-center gap-2 text-primary"><Flag className="size-4" /><p className="text-[10px] font-semibold uppercase tracking-[0.18em]">Milestone pulse</p></div><p className="company-editorial-title mt-4 text-3xl">{model.milestones.length}</p><p className="mt-1 text-xs leading-5 text-muted-foreground">Native business checkpoints. They do not become Mission Waves.</p></section></div>
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
  return <article className="group grid gap-4 p-5 transition hover:bg-muted/30 sm:grid-cols-[minmax(0,1fr)_auto]" data-company-os-ref={item.id} data-work-item-status={item.status}><div className="min-w-0"><div className="flex flex-wrap items-center gap-2"><Status status={item.status} /><span className="text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">{humanize(item.workType)}</span><span className="text-[10px] text-muted-foreground">· {item.businessLine}</span>{item.approvalId && <span data-company-os-ref={item.approvalId} className="text-[10px] text-status-warn">· {item.approval === "requested" ? "Human approval" : humanize(item.approval)}</span>}</div><h3 className="mt-2 truncate font-semibold">{item.title}</h3><p className="mt-1 truncate text-xs text-muted-foreground">{item.milestone} · {item.source}</p></div><div className="flex items-center gap-4"><ActorStack actors={[item.accountable, ...item.assignees, ...item.contributors, item.reviewer]} /><div className="hidden text-right sm:block"><p className="text-xs font-medium">{dateLabel(item.dueAt)}</p><p className="mt-1 text-[10px] text-muted-foreground">{item.approval === "requested" ? "Human approval" : humanize(item.execution)}</p></div><ArrowUpRight className="size-4 text-muted-foreground transition group-hover:text-primary" /></div></article>;
}

function Board({ items }: { items: WorkRow[] }) {
  const columns = ["submitted", "accepted", "in_progress", "blocked", "in_review", "waiting_for_approval", "completed"];
  const labels: Record<string, string> = { submitted: "Inbox", accepted: "Accepted", in_progress: "In progress", blocked: "Blocked", in_review: "In review", waiting_for_approval: "Waiting for approval", completed: "Completed" };
  return <div className="w-full overflow-x-auto pb-4"><div className="grid min-w-[1180px] grid-cols-7 gap-3">{columns.map((status) => { const rows = items.filter((item) => item.status === status || (status === "submitted" && ["draft", "triaged"].includes(item.status))); return <section key={status} className="min-h-[610px] rounded-xl border border-border/80 bg-card/35 p-2.5"><header className="flex items-center justify-between px-1 pb-2.5"><div className="flex items-center gap-2"><span className={`size-1.5 rounded-full ${status === "blocked" ? "bg-destructive" : status === "completed" ? "bg-status-good" : status === "waiting_for_approval" ? "bg-status-warn" : "bg-primary/70"}`} /><h2 className="text-[11px] font-semibold">{labels[status]}</h2></div><span className="text-[10px] text-muted-foreground">{rows.length}</span></header><div className="space-y-2.5">{rows.map((item) => <WorkCard key={item.id} item={item} />)}{rows.length === 0 && <div className="rounded-lg border border-dashed border-border/70 p-4 text-center text-[10px] text-muted-foreground">No work</div>}</div></section>; })}</div></div>;
}

function WorkCard({ item }: { item: WorkRow }) {
  return <article className="rounded-lg border border-border bg-card p-3 shadow-[0_1px_2px_hsl(var(--foreground)/0.04)] transition hover:-translate-y-0.5 hover:border-primary/30 hover:shadow-md" data-company-os-ref={item.id} data-work-item-status={item.status}><div className="flex items-center justify-between gap-2"><span className="rounded-md border border-primary/15 bg-primary/[0.06] px-1.5 py-0.5 text-[9px] font-medium text-primary">{humanize(item.workType)}</span>{item.priority && <span className="text-[9px] text-muted-foreground">{humanize(item.priority)}</span>}</div><h3 className="mt-2.5 text-[13px] font-semibold leading-[1.35]">{item.title}</h3><p className="mt-2 text-[10px] text-muted-foreground">{item.businessLine}</p><div className="mt-3 border-t border-border/70 pt-3"><p className="text-[9px] uppercase tracking-wide text-muted-foreground">Accountable</p><div className="mt-1.5 flex items-center justify-between gap-2"><ActorStack actors={[item.accountable, ...item.assignees]} /><span className="max-w-20 truncate text-[9px] text-muted-foreground">{item.accountable?.name ?? "Unassigned"}</span></div><div className="mt-2 space-y-1 text-[9px] text-muted-foreground"><p className="truncate">⚑ {item.milestone}</p><p>□ {dateLabel(item.dueAt)}</p>{item.approvalId && <p data-company-os-ref={item.approvalId} className="text-status-warn">Human approval</p>}</div></div></article>;
}

function AllWork({ items }: { items: WorkRow[] }) {
  return <section className="overflow-hidden rounded-2xl border border-border bg-card/90 shadow-sm"><SectionTitle eyebrow="Canonical ledger" title="All WorkItems" detail={`${items.length} records · sortable projection`} /><div className="overflow-x-auto"><table className="min-w-[1200px] w-full text-left text-xs"><thead className="bg-muted/45 text-[10px] uppercase tracking-wider text-muted-foreground"><tr>{["WorkItem", "Type", "Business line", "Milestone", "Status", "Accountable", "Assignees", "Approval", "Due", "Execution"].map((label) => <th key={label} className="px-4 py-3 font-semibold">{label}</th>)}</tr></thead><tbody className="divide-y divide-border">{items.map((item) => <tr key={item.id} className="hover:bg-muted/25" data-company-os-ref={item.id}><td className="max-w-xs px-4 py-4"><p className="font-semibold text-foreground">{item.title}</p><p className="mt-1 truncate text-[10px] text-muted-foreground">{item.source}</p></td><td className="px-4 py-4">{humanize(item.workType)}</td><td className="px-4 py-4">{item.businessLine}</td><td className="px-4 py-4">{item.milestone}</td><td className="px-4 py-4"><Status status={item.status} /></td><td className="px-4 py-4">{item.accountable?.name ?? "Unassigned"}</td><td className="px-4 py-4">{item.assignees.map((actor) => actor.name).join(", ") || "Unassigned"}</td><td className="px-4 py-4">{humanize(item.approval)}</td><td className="px-4 py-4">{dateLabel(item.dueAt)}</td><td className="px-4 py-4">{humanize(item.execution)}</td></tr>)}</tbody></table></div></section>;
}

function Milestones({ milestones, items }: { milestones: MilestoneRow[]; items: WorkRow[] }) {
  const unassigned = items.filter((item) => !item.milestoneId);
  return <div className="space-y-5">{milestones.map((milestone) => <section key={milestone.id} className="overflow-hidden rounded-2xl border border-border bg-card/90 shadow-sm" data-company-os-ref={milestone.id}><header className="grid gap-5 border-b border-border p-5 lg:grid-cols-[minmax(0,1fr)_18rem]"><div><div className="flex items-center gap-2"><Flag className="size-4 text-primary" /><Status status={milestone.status} /></div><h2 className="company-editorial-title mt-3 text-2xl">{milestone.title}</h2><p className="mt-2 max-w-2xl text-sm leading-6 text-muted-foreground">{milestone.outcome}</p></div><div><div className="flex items-center justify-between text-xs"><span>{milestone.completed}/{milestone.total} completed</span><strong>{milestone.progress}%</strong></div><div className="mt-3 h-2 overflow-hidden rounded-full bg-muted"><div className="h-full rounded-full bg-gradient-to-r from-primary to-status-good" style={{ width: `${milestone.progress}%` }} /></div><div className="mt-4 flex justify-between text-[10px] text-muted-foreground"><span>{milestone.accountable?.name ?? "No owner"}</span><span>{dateLabel(milestone.targetAt)}</span></div></div></header><div className="divide-y divide-border">{items.filter((item) => item.milestoneId === milestone.id).map((item) => <WorkListRow key={item.id} item={item} />)}</div></section>)}{milestones.length === 0 && <EmptyPanel title="No native Milestones" body="WorkItems remain visible without inventing a checkpoint." />}{unassigned.length > 0 && <section className="overflow-hidden rounded-2xl border border-dashed border-border bg-card/60"><SectionTitle eyebrow="Honest gap" title="No milestone" detail={`${unassigned.length} WorkItems`} /><div className="divide-y divide-border">{unassigned.map((item) => <WorkListRow key={item.id} item={item} />)}</div></section>}</div>;
}

function Timeline({ items }: { items: WorkRow[] }) {
  const dated = [...items].sort((a, b) => (a.dueAt ?? "9999").localeCompare(b.dueAt ?? "9999"));
  return <section className="rounded-2xl border border-border bg-card/90 p-5 shadow-sm"><div className="relative ml-3 border-l border-border pl-7">{dated.map((item) => <article key={item.id} className="relative pb-7 last:pb-0"><span className="absolute -left-[2.1rem] top-1 grid size-4 place-items-center rounded-full border-2 border-card bg-primary"><span className="size-1 rounded-full bg-primary-foreground" /></span><div className="grid gap-3 rounded-xl border border-border bg-background/70 p-4 sm:grid-cols-[8rem_minmax(0,1fr)_auto]"><div><p className="text-xs font-semibold">{dateLabel(item.dueAt)}</p><p className="mt-1 text-[10px] text-muted-foreground">{item.milestone}</p></div><div><h3 className="font-semibold">{item.title}</h3><p className="mt-1 text-xs text-muted-foreground">{item.businessLine} · {humanize(item.workType)}</p></div><div className="flex items-center gap-3"><Status status={item.status} /><ActorStack actors={[item.accountable, ...item.assignees]} /></div></div></article>)}</div></section>;
}

function Workload({ items }: { items: WorkRow[] }) {
  const actors = new Map<string, { actor: ActorRow; accountable: number; assigned: number; active: WorkRow[] }>();
  for (const item of items) {
    if (item.accountable) { const row = actors.get(item.accountable.id) ?? { actor: item.accountable, accountable: 0, assigned: 0, active: [] }; row.accountable += 1; row.active.push(item); actors.set(item.accountable.id, row); }
    for (const assignee of item.assignees) { const row = actors.get(assignee.id) ?? { actor: assignee, accountable: 0, assigned: 0, active: [] }; row.assigned += 1; if (!row.active.some((work) => work.id === item.id)) row.active.push(item); actors.set(assignee.id, row); }
  }
  const unassigned = items.filter((item) => item.assignees.length === 0);
  return <div className="grid gap-5 xl:grid-cols-2">{[...actors.values()].map((row) => <section key={row.actor.id} className="rounded-2xl border border-border bg-card/90 p-5 shadow-sm"><header className="flex items-center justify-between gap-4"><div className="flex items-center gap-3"><ActorAvatar identity={`${row.actor.id} ${row.actor.role}`} name={row.actor.name} size="lg" /><div><h2 className="font-semibold">{row.actor.name}</h2><p className="mt-1 text-xs text-muted-foreground">{row.actor.role}</p></div></div><div className="text-right"><p className="company-editorial-title text-2xl">{row.active.length}</p><p className="text-[10px] text-muted-foreground">active links</p></div></header><div className="mt-4 grid grid-cols-2 gap-2 text-xs"><div className="rounded-lg bg-muted/50 p-3"><span className="text-muted-foreground">Accountable</span><strong className="float-right">{row.accountable}</strong></div><div className="rounded-lg bg-muted/50 p-3"><span className="text-muted-foreground">Assigned</span><strong className="float-right">{row.assigned}</strong></div></div><div className="mt-4 space-y-2">{row.active.slice(0, 3).map((item) => <div key={item.id} className="flex items-center justify-between gap-3 rounded-lg border border-border p-3 text-xs"><span className="truncate font-medium">{item.title}</span><Status status={item.status} /></div>)}</div></section>)}{unassigned.length > 0 && <section className="rounded-2xl border border-dashed border-status-warn/40 bg-status-warn/[0.04] p-5"><div className="flex items-center gap-2 text-status-warn"><AlertTriangle className="size-4" /><h2 className="font-semibold">Unassigned lane</h2></div><p className="mt-2 text-xs text-muted-foreground">{unassigned.length} WorkItems need an explicit executor.</p></section>}</div>;
}

function Status({ status }: { status: string }) {
  const tone = status === "completed" || status === "achieved" ? "border-status-good/25 bg-status-good/10 text-status-good" : status === "blocked" || status === "at_risk" ? "border-destructive/25 bg-destructive/8 text-destructive" : status === "waiting_for_approval" ? "border-status-warn/30 bg-status-warn/10 text-status-warn" : "border-primary/20 bg-primary/8 text-primary";
  return <span className={`inline-flex w-fit items-center rounded-full border px-2 py-1 text-[9px] font-semibold uppercase tracking-wider ${tone}`}>{humanize(status)}</span>;
}

function ActorStack({ actors }: { actors: Array<ActorRow | undefined> }) {
  const unique = actors.filter((actor): actor is ActorRow => Boolean(actor)).filter((actor, index, all) => all.findIndex((candidate) => candidate.id === actor.id) === index).slice(0, 4);
  return <div className="flex -space-x-2">{unique.map((actor) => <span key={actor.id} data-company-os-ref={actor.id} data-actor-type={actor.kind} className="rounded-full"><ActorAvatar identity={`${actor.id} ${actor.role}`} name={actor.name} size="sm" ring={actor.kind === "human" ? "warm" : actor.kind === "agent" ? "good" : "neutral"} /></span>)}</div>;
}

function EmptyWork() { return <EmptyPanel title="No WorkItems yet" body="Create work from a durable document or typed business record. Work will appear here without creating a Project or task graph." />; }
function EmptyPanel({ title, body }: { title: string; body: string }) { return <div className="grid min-h-80 place-items-center rounded-2xl border border-dashed border-border bg-card/50 p-8 text-center"><div><Bot className="mx-auto size-8 text-primary" /><h2 className="company-editorial-title mt-4 text-2xl">{title}</h2><p className="mx-auto mt-2 max-w-md text-sm leading-6 text-muted-foreground">{body}</p></div></div>; }
