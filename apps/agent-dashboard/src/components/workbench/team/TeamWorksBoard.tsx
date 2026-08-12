import { useEffect, useMemo, useRef, useState } from "react";
import { ArrowRight, CheckCircle2, CircleSlash, FileCheck2, ListFilter, Search, ShieldCheck, Users, X } from "lucide-react";

import { cn } from "@/lib/utils";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Avatar } from "@/components/workbench/Avatar";
import { Markdown } from "@/components/workbench/Markdown";
import type { MemberCapacitySummary, WorkSummary } from "../../../model/roleViews";

type OwnerFilter = "all" | "unassigned" | string;
type AttentionFilter = "all" | "blocked" | "review";
const LANES = [
  { id: "open", label: "Open", matches: (work: WorkSummary) => work.phase === "open" },
  { id: "active", label: "Active", matches: (work: WorkSummary) => work.phase === "active" },
  { id: "review", label: "Review", matches: (work: WorkSummary) => work.phase === "review" },
  { id: "closed", label: "Closed", matches: (work: WorkSummary) => work.phase === "closed" },
] as const;

function memberLabel(member: MemberCapacitySummary | undefined, fallback?: string | null) {
  return member?.display_name || fallback || "Unassigned";
}

export function TeamWorksBoard({ works, members, selectedWorkId, onSelectWork, onOpenMember, onOpenHost, ownerFilter, attentionFilter, queryFilter, onFiltersChange, onOpenHostTools }: {
  works: WorkSummary[];
  members: MemberCapacitySummary[];
  selectedWorkId?: string;
  onSelectWork: (workId: string | undefined) => void;
  onOpenMember: (memberRunId: string) => void;
  onOpenHost?: (workId: string) => void;
  ownerFilter?: OwnerFilter;
  attentionFilter?: AttentionFilter;
  queryFilter?: string;
  onFiltersChange?: (filters:{owner:OwnerFilter;attention:AttentionFilter;query:string})=>void;
  onOpenHostTools?: () => void;
}) {
  const [localOwner, setLocalOwner] = useState<OwnerFilter>("all");
  const [localAttention, setLocalAttention] = useState<AttentionFilter>("all");
  const [localQuery, setLocalQuery] = useState("");
  const owner = ownerFilter ?? localOwner;
  const attention = attentionFilter ?? localAttention;
  const query = queryFilter ?? localQuery;
  const [filtersOpen, setFiltersOpen] = useState(false);
  const [compactSheet, setCompactSheet] = useState(false);
  const closeRef = useRef<HTMLButtonElement>(null);
  const sheetRef = useRef<HTMLElement>(null);
  const workCardRefs = useRef(new Map<string, HTMLButtonElement>());
  const focusReturnWorkId = useRef<string>();
  const selected = works.find((work) => work.work_id === selectedWorkId);
  const membersById = useMemo(() => new Map(members.map((member) => [member.agent_member_ref.id, member])), [members]);
  const visible = works.filter((work) => {
    const ownerMatch = owner === "all" || (owner === "unassigned" ? !work.owner_actor_ref : work.owner_actor_ref?.id === owner);
    const attentionMatch = attention === "all" || (attention === "blocked" ? work.condition === "blocked" : work.phase === "review");
    const searchable = [work.work_id, work.title, work.context_markdown, work.completion_criteria_markdown, work.owner_actor_ref?.id].filter(Boolean).join(" ").toLowerCase();
    return ownerMatch && attentionMatch && (!query.trim() || searchable.includes(query.trim().toLowerCase()));
  });

  useEffect(() => {
    const media = window.matchMedia("(max-width: 1023px)");
    const sync = () => setCompactSheet(media.matches);
    sync();
    media.addEventListener("change",sync);
    return () => media.removeEventListener("change",sync);
  },[]);

  useEffect(() => {
    if (selected) {
      focusReturnWorkId.current = selected.work_id;
      closeRef.current?.focus();
      return;
    }
    if (focusReturnWorkId.current) {
      workCardRefs.current.get(focusReturnWorkId.current)?.focus();
      focusReturnWorkId.current = undefined;
    }
  }, [selected?.work_id]);
  useEffect(() => {
    if (!selected) return;
    const close = (event: KeyboardEvent) => {
      if (event.key === "Escape") { event.preventDefault(); onSelectWork(undefined); return; }
      if (!compactSheet) return;
      if (event.key !== "Tab") return;
      const focusable = [...(sheetRef.current?.querySelectorAll<HTMLElement>('button:not([disabled]), [href], input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])') ?? [])];
      if (!focusable.length) return;
      const first = focusable[0];
      const last = focusable[focusable.length - 1];
      if (event.shiftKey && document.activeElement === first) { event.preventDefault(); last.focus(); }
      else if (!event.shiftKey && document.activeElement === last) { event.preventDefault(); first.focus(); }
    };
    document.addEventListener("keydown", close);
    return () => document.removeEventListener("keydown", close);
  }, [selected, onSelectWork, compactSheet]);

  const updateFilters = (next:Partial<{owner:OwnerFilter;attention:AttentionFilter;query:string}>) => {
    const filters={owner:next.owner ?? owner,attention:next.attention ?? attention,query:next.query ?? query};
    if(onFiltersChange)onFiltersChange(filters);else {setLocalOwner(filters.owner);setLocalAttention(filters.attention);setLocalQuery(filters.query);}
  };
  const clearFilters = () => updateFilters({owner:"all",attention:"all",query:""});
  const filtersApplied = owner !== "all" || attention !== "all" || Boolean(query);
  return (
    <section aria-labelledby="team-works-title" data-testid="role-view-team-works">
      <header className="flex flex-wrap items-end justify-between gap-2">
        <div><h2 id="team-works-title" className="text-base font-semibold">Shared Works</h2><p className="text-xs text-muted-foreground">Durable responsibility grouped by canonical lifecycle.</p></div>
        <Button size="sm" variant="secondary" className="min-h-11 sm:min-h-0" aria-expanded={filtersOpen} onClick={() => setFiltersOpen((open) => !open)}><ListFilter className="size-3.5" /> Filters</Button>
      </header>

      <div className={cn("mt-3 grid gap-2 border-y border-border py-3 md:grid-cols-[minmax(12rem,1fr)_auto_auto]", !filtersOpen && "hidden md:grid")} aria-label="Work filters">
        <label className="relative min-w-0"><span className="sr-only">Search Works</span><Search className="pointer-events-none absolute left-3 top-3 size-4 text-muted-foreground"/><input value={query} onChange={(event) => updateFilters({query:event.target.value})} placeholder="Search title, context or ID" className="h-10 w-full rounded-md border border-border bg-background pl-9 pr-3 text-sm outline-none focus:border-primary" /></label>
        <label><span className="sr-only">Owner</span><select value={owner} onChange={(event) => updateFilters({owner:event.target.value})} className="h-10 w-full rounded-md border border-border bg-background px-3 text-xs"><option value="all">All owners</option><option value="unassigned">Unassigned</option>{members.map((member) => <option key={member.agent_member_ref.id} value={member.agent_member_ref.id}>{member.display_name}</option>)}</select></label>
        <div className="flex min-w-0 flex-wrap gap-1" role="group" aria-label="Attention filter">{(["all", "blocked", "review"] as const).map((value) => <Button key={value} size="sm" variant={attention === value ? "default" : "secondary"} className="min-h-10 flex-1" onClick={() => updateFilters({attention:value})}>{value}</Button>)}</div>
      </div>

      {visible.length ? <div className="mt-3 grid gap-3 lg:grid-cols-4" data-testid="team-work-lanes">{LANES.map((lane) => {
        const laneWorks = visible.filter(lane.matches);
        const orderedLaneWorks = lane.id === "open" ? [...laneWorks].sort((left,right) => Number(Boolean(left.owner_actor_ref)) - Number(Boolean(right.owner_actor_ref))) : laneWorks;
        return <section key={lane.id} data-work-lane={lane.id} className={cn("agent-team-recessed min-w-0 rounded-xl p-2.5", !laneWorks.length && "hidden sm:block")}><header className="mb-2 flex items-center justify-between px-1"><div><h3 className="text-[11px] font-semibold uppercase tracking-[.12em] text-foreground/70">{lane.label}</h3>{lane.id === "open" && <p className="mt-0.5 text-[9px] text-muted-foreground">Unassigned and assigned responsibility</p>}</div><span className="rounded-full bg-card px-2 py-0.5 text-xs tabular-nums text-muted-foreground ring-1 ring-border">{laneWorks.length}</span></header><div className="grid gap-2">{orderedLaneWorks.map((work,index) => {
          const ownerMember = work.owner_actor_ref ? membersById.get(work.owner_actor_ref.id) : undefined;
          const firstAssigned = lane.id === "open" && Boolean(work.owner_actor_ref) && (index === 0 || !orderedLaneWorks[index-1]?.owner_actor_ref);
          return <div key={work.work_id} className="contents">{lane.id === "open" && (index === 0 || firstAssigned) && <p className="col-span-full px-1 pt-1 text-[9px] font-semibold uppercase tracking-[.12em] text-muted-foreground">{work.owner_actor_ref ? "Assigned" : "Unassigned"}</p>}<button ref={(node) => { if (node) workCardRefs.current.set(work.work_id, node); else workCardRefs.current.delete(work.work_id); }} type="button" data-work-card={work.work_id} onClick={() => onSelectWork(work.work_id)} className={cn("agent-team-panel min-w-0 rounded-lg p-3 text-left hover:border-primary/30 hover:bg-accent/35", selectedWorkId === work.work_id && "agent-team-selected")}><div className="flex flex-wrap items-center justify-between gap-1"><div className="flex flex-wrap gap-1"><Badge>{work.phase}</Badge>{work.condition !== "normal" && <Badge tone={work.condition === "blocked" ? "bad" : "warn"}>{work.condition.replace(/_/g," ")}</Badge>}{work.phase === "closed" && work.resolution && <Badge tone={work.resolution === "accepted" ? "good" : work.resolution === "failed" ? "bad" : "muted"}>{work.resolution}</Badge>}</div><span className="text-[9px] font-semibold uppercase tracking-wide text-muted-foreground">{work.priority}</span></div><h4 className="mt-2 break-words text-sm font-semibold leading-snug">{work.title || work.work_id}</h4><p className="mt-1 line-clamp-2 text-[11px] leading-relaxed text-muted-foreground">{work.completion_criteria_markdown || "No projected completion criteria."}</p><div className="mt-3 grid grid-cols-2 gap-2 text-[9px] text-muted-foreground"><span className="flex items-center gap-1"><FileCheck2 className="size-3"/>{work.artifact_refs.length + work.check_refs.length} evidence refs</span><span className="flex items-center justify-end gap-1"><CheckCircle2 className={cn("size-3",work.gate_summary.required > 0 && work.gate_summary.passed === work.gate_summary.required && "text-status-good")}/>{work.gate_summary.passed}/{work.gate_summary.required} gates</span></div><div className="mt-2 flex min-w-0 items-center gap-1.5 border-t border-border pt-2 text-[10px] text-muted-foreground">{ownerMember ? <><Avatar name={ownerMember.display_name} identity={`${ownerMember.agent_member_ref.id} ${ownerMember.role}`} size="xs" tone={ownerMember.runtime_state === "running" ? "running" : ownerMember.capacity === "available" ? "good" : "idle"}/><CircleDotLabel text={memberLabel(ownerMember)} /></> : <><Users className="size-3.5"/>Unassigned</>}<span className="ml-auto shrink-0 font-mono">v{work.work_revision}</span></div></button></div>;
        })}{!laneWorks.length && <div className="hidden min-h-20 place-items-center rounded-lg border border-dashed border-border text-[10px] text-muted-foreground sm:grid">No {lane.label.toLowerCase()} Work</div>}</div></section>;
      })}</div> : <div className="mt-3 rounded-xl border border-dashed border-border px-5 py-12 text-center"><CircleSlash className="mx-auto size-6 text-muted-foreground"/><h3 className="mt-3 text-sm font-medium">{works.length ? "No Work matches these filters" : "This Team has no durable Work yet"}</h3><p className="mt-1 text-xs text-muted-foreground">{works.length ? "Reset the filters to return to the Team's full responsibility view." : "Open Host tools to create the first Work or coordinate with an available member."}</p>{filtersApplied ? <Button className="mt-4" size="sm" variant="secondary" onClick={clearFilters}>Reset filters</Button> : onOpenHostTools ? <Button className="mt-4" size="sm" onClick={onOpenHostTools}><ShieldCheck className="size-3.5"/>Open Host tools</Button> : null}</div>}

      {selected && <div className="fixed inset-0 z-50 bg-foreground/15 lg:static lg:z-auto lg:mt-3 lg:bg-transparent" onMouseDown={(event) => { if (compactSheet && event.target === event.currentTarget) onSelectWork(undefined); }}><aside ref={sheetRef} role="dialog" aria-modal={compactSheet || undefined} aria-labelledby="selected-work-title" data-testid="role-view-work-sheet" className="agent-team-sheet-enter absolute inset-x-0 bottom-0 max-h-[88dvh] overflow-y-auto rounded-t-xl border border-border bg-background p-4 shadow-xl lg:static lg:max-h-none lg:w-full lg:rounded-xl lg:shadow-none lg:animate-none">
        <div className="flex items-start gap-3"><div className="min-w-0 flex-1"><div className="flex flex-wrap gap-2"><Badge>{selected.phase}</Badge>{selected.condition === "blocked" && <Badge tone="warn">blocked</Badge>}<span className="font-mono text-[10px] text-muted-foreground">{selected.work_id} · v{selected.work_revision}</span></div><h2 id="selected-work-title" className="mt-2 break-words text-lg font-semibold">{selected.title || selected.work_id}</h2></div><button ref={closeRef} className="grid size-11 shrink-0 place-items-center rounded-md hover:bg-muted" onClick={() => onSelectWork(undefined)} aria-label="Close Work details"><X className="size-4"/></button></div>
        <div className="mt-5 space-y-4 text-sm"><Detail title="Context" source={selected.context_markdown}/><Detail title="Completion criteria" source={selected.completion_criteria_markdown}/><FactGrid work={selected}/>{selected.blocker_reason && <Detail title="Blocker" source={selected.blocker_reason} tone="warn"/>}{selected.result_summary && <Detail title="Submitted result" source={selected.result_summary}/>}<ReferenceList title="Artifacts" refs={selected.artifact_refs}/><ReferenceList title="Checks and evidence" refs={selected.check_refs}/>{selected.latest_event && <section><h3 className="text-[10px] font-semibold uppercase tracking-[.12em] text-muted-foreground">Latest Work event</h3><p className="mt-1 rounded-lg bg-muted/35 p-3 text-xs">{selected.latest_event.kind.replace(/_/g, " ")} · {new Date(selected.latest_event.created_at).toLocaleString()}</p></section>}</div>
        <div className="mt-5 flex flex-wrap gap-2 border-t border-border pt-4">{selected.current_member_run_ref && <Button size="sm" variant="secondary" onClick={() => onOpenMember(selected.current_member_run_ref!)}>Open member context <ArrowRight className="size-3.5"/></Button>}{onOpenHost && <Button size="sm" onClick={() => onOpenHost(selected.work_id)}><ShieldCheck className="size-3.5"/>Host controls</Button>}</div>
      </aside></div>}
    </section>
  );
}

function CircleDotLabel({ text }: { text: string }) { return <span className="min-w-0 truncate">{text}</span>; }
function Detail({ title, source, tone }: { title:string; source:string; tone?:"warn" }) { return <section className={cn(tone === "warn" && "rounded-lg border border-status-warn/30 bg-status-warn/5 p-3")}><h3 className="mb-1 text-[10px] font-semibold uppercase tracking-[.12em] text-muted-foreground">{title}</h3>{source ? <Markdown source={source} compact/> : <p className="text-xs text-muted-foreground">Not provided.</p>}</section>; }
function ReferenceList({ title, refs }: {title:string;refs:string[]}) { if (!refs.length) return null; return <section><h3 className="text-[10px] font-semibold uppercase tracking-[.12em] text-muted-foreground">{title}</h3><ul className="mt-1 space-y-1">{refs.map((ref) => <li key={ref} className="break-all rounded-md bg-muted/35 px-2 py-1.5 font-mono text-[10px]">{ref}</li>)}</ul></section>; }
function FactGrid({ work }: {work:WorkSummary}) { return <dl className="grid grid-cols-2 gap-px overflow-hidden rounded-lg border border-border bg-border text-xs"><Fact label="Owner" value={work.owner_actor_ref?.id ?? "Unassigned"}/><Fact label="Claim" value={work.claim_mode}/><Fact label="Parent" value={work.parent_work_id ?? "None"}/><Fact label="Prerequisites" value={`${work.prerequisite_work_ids.length}`}/><Fact label="Gates" value={`${work.gate_summary.passed}/${work.gate_summary.required} passed`}/><Fact label="Delivery" value={String(work.delivery_summary.recovery_class ?? "not observed")}/></dl>; }
function Fact({label,value}:{label:string;value:string}) { return <div className="min-w-0 bg-card p-2.5"><dt className="text-[9px] uppercase tracking-wider text-muted-foreground">{label}</dt><dd className="mt-1 break-words font-medium">{value}</dd></div>; }
