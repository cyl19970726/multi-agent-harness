import { useEffect, useMemo, useRef, useState } from "react";
import { ArrowRight, CheckCircle2, CircleSlash, FileCheck2, Flag, ListFilter, Search, ShieldCheck, Users, X } from "lucide-react";

import { cn } from "@/lib/utils";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Avatar } from "@/components/workbench/Avatar";
import { Markdown } from "@/components/workbench/Markdown";
import type { MemberCapacitySummary, WorkSummary } from "../../../model/roleViews";

type OwnerFilter = "all" | "unassigned" | string;
type AttentionFilter = "all" | "blocked" | "review";
type ConditionFilter = "all" | string;
type PriorityFilter = "all" | string;
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
  const [condition, setCondition] = useState<ConditionFilter>("all");
  const [priority, setPriority] = useState<PriorityFilter>("all");
  const owner = ownerFilter ?? localOwner;
  const attention = attentionFilter ?? localAttention;
  const query = queryFilter ?? localQuery;
  const [filtersOpen, setFiltersOpen] = useState(false);
  const [compactSheet, setCompactSheet] = useState(false);
  const closeRef = useRef<HTMLButtonElement>(null);
  const sheetRef = useRef<HTMLElement>(null);
  const filterTriggerRef = useRef<HTMLButtonElement>(null);
  const filterCloseRef = useRef<HTMLButtonElement>(null);
  const filterSheetRef = useRef<HTMLDivElement>(null);
  const workCardRefs = useRef(new Map<string, HTMLButtonElement>());
  const focusReturnWorkId = useRef<string>();
  const selected = works.find((work) => work.work_id === selectedWorkId);
  const membersById = useMemo(() => new Map(members.map((member) => [member.agent_member_ref.id, member])), [members]);
  const conditions = useMemo(() => Array.from(new Set(works.map((work) => work.condition?.trim()).filter((value): value is string => Boolean(value)))), [works]);
  const priorities = useMemo(() => Array.from(new Set(works.map((work) => work.priority?.trim()).filter((value): value is string => Boolean(value)))), [works]);
  const visible = works.filter((work) => {
    const ownerMatch = owner === "all" || (owner === "unassigned" ? !work.owner_actor_ref : work.owner_actor_ref?.id === owner);
    const attentionMatch = attention === "all" || (attention === "blocked" ? work.condition === "blocked" : work.phase === "review");
    const conditionMatch = condition === "all" || work.condition === condition;
    const priorityMatch = priority === "all" || work.priority === priority;
    const searchable = [work.work_id, work.title, work.context_markdown, work.completion_criteria_markdown, work.owner_actor_ref?.id].filter(Boolean).join(" ").toLowerCase();
    return ownerMatch && attentionMatch && conditionMatch && priorityMatch && (!query.trim() || searchable.includes(query.trim().toLowerCase()));
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
      const workId=focusReturnWorkId.current;
      const visibleCard=[...document.querySelectorAll<HTMLButtonElement>(`[data-work-card="${CSS.escape(workId)}"], [data-priority-work="${CSS.escape(workId)}"]`)].find((node) => node.offsetParent !== null);
      visibleCard?.focus();
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
  const closeFilters = () => {
    setFiltersOpen(false);
    filterTriggerRef.current?.focus();
  };
  useEffect(() => {
    if (!compactSheet || !filtersOpen) return;
    filterCloseRef.current?.focus();
    const handleKeyDown = (event:KeyboardEvent) => {
      if (event.key === "Escape") { event.preventDefault(); closeFilters(); return; }
      if (event.key !== "Tab") return;
      const focusable = [...(filterSheetRef.current?.querySelectorAll<HTMLElement>('button:not([disabled]), input:not([disabled]), select:not([disabled]), [tabindex]:not([tabindex="-1"])') ?? [])];
      if (!focusable.length) return;
      const first = focusable[0];
      const last = focusable[focusable.length - 1];
      if (event.shiftKey && document.activeElement === first) { event.preventDefault(); last.focus(); }
      else if (!event.shiftKey && document.activeElement === last) { event.preventDefault(); first.focus(); }
    };
    document.addEventListener("keydown",handleKeyDown);
    return () => document.removeEventListener("keydown",handleKeyDown);
  },[compactSheet,filtersOpen]);

  const updateFilters = (next:Partial<{owner:OwnerFilter;attention:AttentionFilter;query:string}>) => {
    const filters={owner:next.owner ?? owner,attention:next.attention ?? attention,query:next.query ?? query};
    if(onFiltersChange)onFiltersChange(filters);else {setLocalOwner(filters.owner);setLocalAttention(filters.attention);setLocalQuery(filters.query);}
  };
  const clearFilters = () => { updateFilters({owner:"all",attention:"all",query:""}); setCondition("all"); setPriority("all"); };
  const filtersApplied = owner !== "all" || attention !== "all" || condition !== "all" || priority !== "all" || Boolean(query);
  const priorityWork = [...visible].sort((left,right) => workAttentionRank(right) - workAttentionRank(left))[0];
  return (
    <section aria-labelledby="team-works-title" data-testid="role-view-team-works">
      <header className="flex flex-wrap items-end justify-between gap-2 pt-4 lg:hidden">
        <div><h2 id="team-works-title" className="text-base font-semibold">Shared Works</h2><p className="text-xs text-muted-foreground">Durable responsibility grouped by canonical lifecycle.</p></div>
        <Button ref={filterTriggerRef} size="sm" variant="secondary" className="min-h-11 sm:min-h-0" aria-expanded={filtersOpen} aria-controls="team-work-filters" onClick={() => filtersOpen ? closeFilters() : setFiltersOpen(true)}><ListFilter className="size-3.5" /> Filters</Button>
      </header>

      {filtersOpen && compactSheet && <button type="button" aria-label="Close Work filters" className="fixed inset-0 z-40 bg-foreground/15" onClick={closeFilters}/>}
      <div ref={filterSheetRef} id="team-work-filters" role={compactSheet ? "dialog" : undefined} aria-modal={compactSheet && filtersOpen ? true : undefined} className={cn("grid gap-2 border-b border-border/80 bg-card/70 py-3 lg:grid-cols-[minmax(16rem,1fr)_10rem_9rem_9rem_12rem]", !filtersOpen && "hidden lg:grid", compactSheet && filtersOpen && "agent-team-sheet-enter fixed inset-x-0 bottom-0 z-50 max-h-[88dvh] overflow-y-auto rounded-t-xl border bg-background p-4 shadow-xl")} aria-label="Work filters">
        <div className="mb-1 flex items-center lg:hidden"><h3 className="text-sm font-semibold">Filter Works</h3><button ref={filterCloseRef} type="button" className="ml-auto grid size-11 place-items-center rounded-md" aria-label="Close Work filters" onClick={closeFilters}><X className="size-4"/></button></div>
        <label className="relative min-w-0"><span className="sr-only">Search Works</span><Search className="pointer-events-none absolute left-3 top-3 size-4 text-muted-foreground"/><input value={query} onChange={(event) => updateFilters({query:event.target.value})} placeholder="Search title, context or ID" className="agent-team-control h-10 w-full pl-9 pr-3 text-sm outline-none focus:border-primary" /></label>
        <label><span className="sr-only">Owner</span><select value={owner} onChange={(event) => updateFilters({owner:event.target.value})} className="agent-team-control h-10 w-full px-3 text-xs"><option value="all">All owners</option><option value="unassigned">Unassigned</option>{members.map((member) => <option key={member.agent_member_ref.id} value={member.agent_member_ref.id}>{member.display_name}</option>)}</select></label>
        <label><span className="sr-only">Condition</span><select value={condition} onChange={(event) => setCondition(event.target.value as ConditionFilter)} className="agent-team-control h-10 w-full px-3 text-xs"><option value="all">All conditions</option>{conditions.map((value) => <option key={value} value={value}>{value.replace(/_/g," ")}</option>)}</select></label>
        <label><span className="sr-only">Priority</span><select value={priority} onChange={(event) => setPriority(event.target.value as PriorityFilter)} className="agent-team-control h-10 w-full px-3 text-xs"><option value="all">All priorities</option>{priorities.map((value) => <option key={value} value={value}>{value.replace(/_/g," ")}</option>)}</select></label>
        <div className="agent-team-control flex min-w-0 gap-0.5 p-0.5" role="group" aria-label="Attention filter">{(["all", "blocked", "review"] as const).map((value) => <button key={value} type="button" data-active={attention === value} className="agent-team-filter-choice min-w-0 flex-1 px-2 text-xs font-medium" onClick={() => updateFilters({attention:value})}>{value}</button>)}</div>
      </div>

      {visible.length ? <>
      {priorityWork && <section className="mt-3 border-y border-border py-3 lg:hidden" aria-labelledby="priority-work-title"><div className="mb-2 flex items-center gap-2"><Flag className="size-3.5 text-primary"/><h3 id="priority-work-title" className="text-[10px] font-semibold uppercase tracking-[.13em]">Attention preview</h3><span className="ml-auto text-[9px] text-muted-foreground">Display ordering · canonical phase below</span></div><WorkCard work={priorityWork} ownerMember={priorityWork.owner_actor_ref ? membersById.get(priorityWork.owner_actor_ref.id) : undefined} selected={selectedWorkId === priorityWork.work_id} onSelect={onSelectWork} register={(node) => { if (node) workCardRefs.current.set(priorityWork.work_id,node); else workCardRefs.current.delete(priorityWork.work_id); }} prominent/></section>}
      <div className="mt-2 grid gap-x-4 lg:hidden md:grid-cols-2" data-testid="team-work-mobile-phases">{LANES.map((lane) => { const laneWorks=visible.filter(lane.matches); return <details key={lane.id} className="border-b border-border" open><summary className="flex min-h-11 cursor-pointer list-none items-center text-xs font-semibold uppercase tracking-[.1em]"><span>{lane.label}</span><span className="ml-auto tabular-nums text-muted-foreground">{laneWorks.length}</span></summary><div className="space-y-2 pb-3">{lane.id === "open" && <OpenGroupCounts works={laneWorks}/>} {laneWorks.map((work) => <WorkCard key={work.work_id} work={work} ownerMember={work.owner_actor_ref ? membersById.get(work.owner_actor_ref.id) : undefined} selected={selectedWorkId === work.work_id} onSelect={onSelectWork} register={(node) => { if(node) workCardRefs.current.set(work.work_id,node); else workCardRefs.current.delete(work.work_id); }}/>)}</div></details>;})}</div>
      <div className="hidden min-h-[590px] grid-cols-4 items-start bg-[color-mix(in_srgb,var(--secondary)_30%,transparent)] lg:grid" data-testid="team-work-lanes">{LANES.map((lane) => {
        const laneWorks = visible.filter(lane.matches);
        const orderedLaneWorks = lane.id === "open" ? [...laneWorks].sort((left,right) => Number(Boolean(left.owner_actor_ref)) - Number(Boolean(right.owner_actor_ref))) : laneWorks;
        return <section key={lane.id} data-work-lane={lane.id} className="min-w-0 border-l border-border px-3 py-4 first:border-l-0"><header className="mb-3 flex items-center border-b border-border pb-2"><h3 className="company-editorial-title text-[17px] uppercase tracking-[.07em]">{lane.label}</h3><span className="ml-2 text-xs tabular-nums text-muted-foreground">{laneWorks.length}</span></header><div className="grid content-start gap-3">{orderedLaneWorks.map((work,index) => {
          const ownerMember = work.owner_actor_ref ? membersById.get(work.owner_actor_ref.id) : undefined;
          const firstAssigned = lane.id === "open" && Boolean(work.owner_actor_ref) && (index === 0 || !orderedLaneWorks[index-1]?.owner_actor_ref);
          return <div key={work.work_id} className="contents">{lane.id === "open" && (index === 0 || firstAssigned) && <p className="px-1 pt-1 text-[9px] font-semibold uppercase tracking-[.12em] text-muted-foreground">{work.owner_actor_ref ? "Assigned" : "Unassigned"}</p>}<WorkCard work={work} ownerMember={ownerMember} selected={selectedWorkId === work.work_id} onSelect={onSelectWork} register={(node) => { if(node) workCardRefs.current.set(work.work_id,node); else workCardRefs.current.delete(work.work_id); }}/></div>;
        })}{!laneWorks.length && <p className="py-4 text-center text-[10px] text-muted-foreground">No {lane.label.toLowerCase()} Work</p>}</div></section>;
      })}</div></> : <div className="mt-3 border-y border-dashed border-border px-5 py-12 text-center"><CircleSlash className="mx-auto size-6 text-muted-foreground"/><h3 className="mt-3 text-sm font-medium">{works.length ? "No Work matches these filters" : "This Team has no durable Work yet"}</h3><p className="mt-1 text-xs text-muted-foreground">{works.length ? "Reset the filters to return to the Team's full responsibility view." : "Open Host tools to create the first Work or coordinate with an available member."}</p>{filtersApplied ? <Button className="mt-4" size="sm" variant="secondary" onClick={clearFilters}>Reset filters</Button> : onOpenHostTools ? <Button className="mt-4" size="sm" onClick={onOpenHostTools}><ShieldCheck className="size-3.5"/>Open Host tools</Button> : null}</div>}

      {selected && <div className="fixed inset-0 z-50 bg-foreground/15 lg:static lg:z-auto lg:mt-3 lg:bg-transparent" onMouseDown={(event) => { if (compactSheet && event.target === event.currentTarget) onSelectWork(undefined); }}><aside ref={sheetRef} role="dialog" aria-modal={compactSheet || undefined} aria-labelledby="selected-work-title" data-testid="role-view-work-sheet" className="agent-team-sheet-enter absolute inset-x-0 bottom-0 max-h-[88dvh] overflow-y-auto rounded-t-xl border border-border bg-background p-4 shadow-xl lg:static lg:max-h-none lg:w-full lg:rounded-xl lg:shadow-none lg:animate-none">
        <div className="flex items-start gap-3"><div className="min-w-0 flex-1"><div className="flex flex-wrap gap-2"><Badge>{selected.phase}</Badge>{selected.condition !== "normal" && <Badge tone={selected.condition === "blocked" ? "bad" : "warn"}>{selected.condition.replace(/_/g," ")}</Badge>}{selected.phase === "closed" && selected.resolution && <Badge tone={selected.resolution === "accepted" ? "good" : selected.resolution === "failed" ? "bad" : "muted"}>{selected.resolution}</Badge>}<span className="font-mono text-[10px] text-muted-foreground">{selected.work_id} · v{selected.work_revision}</span></div><h2 id="selected-work-title" className="mt-2 break-words text-lg font-semibold">{selected.title || selected.work_id}</h2></div><button ref={closeRef} className="grid size-11 shrink-0 place-items-center rounded-md hover:bg-muted" onClick={() => onSelectWork(undefined)} aria-label="Close Work details"><X className="size-4"/></button></div>
        <div className="mt-5 space-y-4 text-sm"><Detail title="Context" source={selected.context_markdown}/><Detail title="Completion criteria" source={selected.completion_criteria_markdown}/><FactGrid work={selected}/>{selected.blocker_reason && <Detail title="Blocker" source={selected.blocker_reason} tone="warn"/>}{selected.result_summary && <Detail title="Submitted result" source={selected.result_summary}/>}<ReferenceList title="Artifacts" refs={selected.artifact_refs}/><ReferenceList title="Checks and evidence" refs={selected.check_refs}/>{selected.latest_event && <section><h3 className="text-[10px] font-semibold uppercase tracking-[.12em] text-muted-foreground">Latest Work event</h3><p className="mt-1 rounded-lg bg-muted/35 p-3 text-xs">{selected.latest_event.kind.replace(/_/g, " ")} · {new Date(selected.latest_event.created_at).toLocaleString()}</p></section>}</div>
        <div className="mt-5 flex flex-wrap gap-2 border-t border-border pt-4">{selected.current_member_run_ref && <Button size="sm" variant="secondary" onClick={() => onOpenMember(selected.current_member_run_ref!)}>Open member context <ArrowRight className="size-3.5"/></Button>}{onOpenHost && <Button size="sm" onClick={() => onOpenHost(selected.work_id)}><ShieldCheck className="size-3.5"/>Host controls</Button>}</div>
      </aside></div>}
    </section>
  );
}

function CircleDotLabel({ text }: { text: string }) { return <span className="min-w-0 truncate">{text}</span>; }
function workAttentionRank(work:WorkSummary) { return (work.condition === "blocked" ? 100 : 0) + (work.phase === "review" ? 80 : work.phase === "active" ? 40 : work.phase === "open" ? 20 : 0) + (work.priority === "critical" ? 12 : work.priority === "high" ? 8 : 0); }
function OpenGroupCounts({works}:{works:WorkSummary[]}) { const assigned=works.filter((work) => work.owner_actor_ref).length; return <div className="grid grid-cols-2 gap-px border-y border-border py-2 text-[10px]"><span>Unassigned <b className="ml-1">{works.length-assigned}</b></span><span>Assigned <b className="ml-1">{assigned}</b></span></div>; }
function WorkCard({work,ownerMember,selected,onSelect,register,prominent=false}:{work:WorkSummary;ownerMember?:MemberCapacitySummary;selected:boolean;onSelect:(id:string)=>void;register:(node:HTMLButtonElement|null)=>void;prominent?:boolean}) { return <button ref={register} type="button" {...(prominent ? {"data-priority-work":work.work_id} : {"data-work-card":work.work_id})} onClick={() => onSelect(work.work_id)} className={cn("agent-team-panel min-w-0 rounded-[10px] p-3.5 text-left hover:-translate-y-px hover:border-primary/25 hover:shadow-[0_10px_28px_rgb(83_57_38_/_0.065)]",selected && "agent-team-selected",prominent && "w-full bg-accent/35")}><div className="flex flex-wrap items-center justify-between gap-2"><PhaseMark work={work}/><span className="text-[9px] font-semibold uppercase tracking-[.09em] text-muted-foreground">{work.priority}</span></div><h4 className="company-editorial-title mt-2.5 break-words text-[17px] leading-[1.18]">{work.title || work.work_id}</h4><p className="mt-1 line-clamp-2 text-[12px] leading-[1.45] text-muted-foreground">{work.completion_criteria_markdown || "No projected completion criteria."}</p><div className="mt-3 grid grid-cols-3 gap-2 text-[10px] text-muted-foreground"><span className="flex items-center gap-1.5"><FileCheck2 className="size-3.5"/>{work.artifact_refs.length + work.check_refs.length}</span><span className="flex items-center gap-1.5"><CheckCircle2 className="size-3.5"/>{work.gate_summary.passed}/{work.gate_summary.required}</span><span className="text-right">v{work.work_revision}</span></div><div className="mt-3 flex min-w-0 items-center gap-2 border-t border-border/75 pt-2.5 text-[10px] text-muted-foreground">{ownerMember ? <><Avatar name={ownerMember.display_name} identity={`${ownerMember.agent_member_ref.id} ${ownerMember.role}`} size="xs" tone={ownerMember.runtime_state === "running" ? "running" : ownerMember.capacity === "available" ? "good" : "idle"}/><CircleDotLabel text={memberLabel(ownerMember)}/></> : <><Users className="size-3.5"/>Unassigned</>}<span className="ml-auto flex items-center gap-1.5 font-medium"><WorkStateMark work={work}/></span></div></button>; }
function PhaseMark({work}:{work:WorkSummary}) { const tone=work.condition === "blocked" ? "bad" : work.phase === "review" ? "warn" : work.phase === "active" ? "running" : work.phase === "closed" && work.resolution === "accepted" ? "good" : undefined; const label=work.condition !== "normal" ? work.condition : work.phase === "closed" && work.resolution ? work.resolution : work.phase; return <span className={tone ? "agent-team-state-label" : "agent-team-phase-label"} data-tone={tone}>{label.replace(/_/g," ")}</span>; }
function WorkStateMark({work}:{work:WorkSummary}) { if(work.condition === "blocked")return <><CircleSlash className="size-3.5 text-status-bad"/><span className="text-status-bad">Blocked</span></>; if(work.phase === "closed" && work.resolution)return <><CheckCircle2 className={cn("size-3.5",work.resolution === "accepted" ? "text-status-good" : work.resolution === "failed" ? "text-status-bad" : "text-muted-foreground")}/><span className={work.resolution === "accepted" ? "text-status-good" : work.resolution === "failed" ? "text-status-bad" : "text-muted-foreground"}>{work.resolution[0].toUpperCase()+work.resolution.slice(1)}</span></>; if(work.phase === "review")return <><CheckCircle2 className="size-3.5 text-status-warn"/><span className="text-status-warn">Awaiting review</span></>; if(work.gate_summary.required > 0 && work.gate_summary.passed === work.gate_summary.required)return <><CheckCircle2 className="size-3.5 text-status-good"/><span className="text-status-good">Gates satisfied</span></>; return <><CheckCircle2 className="size-3.5 text-muted-foreground"/><span className="text-muted-foreground">{work.phase === "active" ? "In execution" : "Ready for responsibility"}</span></>; }
function Detail({ title, source, tone }: { title:string; source:string; tone?:"warn" }) { return <section className={cn(tone === "warn" && "rounded-lg border border-status-warn/30 bg-status-warn/5 p-3")}><h3 className="mb-1 text-[10px] font-semibold uppercase tracking-[.12em] text-muted-foreground">{title}</h3>{source ? <Markdown source={source} compact/> : <p className="text-xs text-muted-foreground">Not provided.</p>}</section>; }
function ReferenceList({ title, refs }: {title:string;refs:string[]}) { if (!refs.length) return null; return <section><h3 className="text-[10px] font-semibold uppercase tracking-[.12em] text-muted-foreground">{title}</h3><ul className="mt-1 space-y-1">{refs.map((ref) => <li key={ref} className="break-all rounded-md bg-muted/35 px-2 py-1.5 font-mono text-[10px]">{ref}</li>)}</ul></section>; }
function FactGrid({ work }: {work:WorkSummary}) { return <dl className="grid grid-cols-2 gap-px overflow-hidden rounded-lg border border-border bg-border text-xs sm:grid-cols-3"><Fact label="Phase" value={work.phase}/><Fact label="Condition" value={work.condition}/><Fact label="Resolution" value={work.phase === "closed" ? work.resolution ?? "not recorded" : "not applicable"}/><Fact label="Owner" value={work.owner_actor_ref?.id ?? "Unassigned"}/><Fact label="Claim" value={work.claim_mode}/><Fact label="Parent" value={work.parent_work_id ?? "None"}/><Fact label="Prerequisites" value={`${work.prerequisite_work_ids.length}`}/><Fact label="Gates" value={`${work.gate_summary.passed}/${work.gate_summary.required} passed`}/><Fact label="Delivery" value={String(work.delivery_summary.recovery_class ?? "not observed")}/></dl>; }
function Fact({label,value}:{label:string;value:string}) { return <div className="min-w-0 bg-card p-2.5"><dt className="text-[9px] uppercase tracking-wider text-muted-foreground">{label}</dt><dd className="mt-1 break-words font-medium">{value}</dd></div>; }
