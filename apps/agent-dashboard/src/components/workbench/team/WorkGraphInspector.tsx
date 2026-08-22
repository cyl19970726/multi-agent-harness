import { useEffect, useState, type RefObject } from "react";
import { AlertTriangle, ArrowRight, CheckCircle2, GitBranch, Save, ShieldCheck, X } from "lucide-react";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Markdown } from "@/components/workbench/Markdown";
import { cn } from "@/lib/utils";
import { formatAbsolute, formatDate, isoTime } from "./teamFormat";
import { prepareRoleAction, type AllowedAction, type RoleActionExecutor, type WorkSummary } from "../../../model/roleViews";

export function WorkGraphInspector({work,allWorks,dependencyAction,teamId,actionsCurrent,onAction,onCompleted,onClose,onNavigate,onOpenMember,onOpenHost,closeRef}:{
  work:WorkSummary;
  allWorks:WorkSummary[];
  dependencyAction?:AllowedAction;
  teamId:string;
  actionsCurrent:boolean;
  onAction:RoleActionExecutor;
  onCompleted:()=>void;
  onClose:()=>void;
  onNavigate:(workId:string)=>void;
  onOpenMember:(memberRunId:string)=>void;
  onOpenHost?:(workId:string)=>void;
  closeRef:RefObject<HTMLButtonElement>;
}){
  return <>
    <header className="flex items-start gap-3"><div className="min-w-0 flex-1"><div className="flex flex-wrap gap-2"><Badge>{work.phase}</Badge>{work.condition!=="normal"&&<Badge tone={work.condition==="blocked"?"bad":"warn"}>{work.condition.replace(/_/g," ")}</Badge>}{work.phase==="closed"&&work.resolution&&<Badge tone={work.resolution==="accepted"?"good":work.resolution==="failed"?"bad":"muted"}>{work.resolution}</Badge>}<span className="font-mono text-[10px] text-muted-foreground">{work.work_id} · v{work.work_revision}</span></div><h2 id="selected-work-title" className="mt-2 break-words text-lg font-semibold">{work.title||work.work_id}</h2></div><button ref={closeRef} className="grid size-11 shrink-0 place-items-center rounded-md hover:bg-muted" onClick={onClose} aria-label="Close Work details"><X className="size-4"/></button></header>
    <div className="mt-4 space-y-4 text-sm">
      <ReadinessPanel work={work}/>
      <Relations work={work} allWorks={allWorks} onNavigate={onNavigate}/>
      <Detail title="Context" source={work.context_markdown}/><Detail title="Completion criteria" source={work.completion_criteria_markdown}/>
      <FactGrid work={work}/>
      {work.blocker_reason&&<Detail title="Blocker" source={work.blocker_reason} tone="warn"/>}{work.result_summary&&<Detail title="Submitted result" source={work.result_summary}/>}<ReferenceList title="Artifacts" refs={work.artifact_refs}/><ReferenceList title="Checks and evidence" refs={work.check_refs}/>
      {work.latest_event&&<section><h3 className="text-[10px] font-semibold uppercase tracking-[.12em] text-muted-foreground">Latest Work event</h3><p className="mt-1 rounded-lg bg-muted/35 p-3 text-xs">{work.latest_event.kind.replace(/_/g," ")} · <time dateTime={isoTime(work.latest_event.created_at)} title={formatAbsolute(work.latest_event.created_at)}>{formatDate(work.latest_event.created_at)}</time></p></section>}
      {dependencyAction&&<DependencyEditor work={work} allWorks={allWorks} action={dependencyAction} teamId={teamId} actionsCurrent={actionsCurrent} onAction={onAction} onCompleted={onCompleted}/>}
    </div>
    <footer className="mt-5 flex flex-wrap gap-2 border-t border-border pt-4">{work.current_member_run_ref&&<Button size="sm" variant="secondary" onClick={()=>onOpenMember(work.current_member_run_ref!)}>Open member context <ArrowRight className="size-3.5"/></Button>}{onOpenHost&&<Button size="sm" onClick={()=>onOpenHost(work.work_id)}><ShieldCheck className="size-3.5"/>Host controls</Button>}</footer>
  </>;
}

function ReadinessPanel({work}:{work:WorkSummary}){
  const readiness=work.readiness;
  if(!readiness)return <section className="rounded-lg border border-dashed border-border p-3"><h3 className="text-xs font-semibold">Readiness not projected</h3><p className="mt-1 text-[11px] text-muted-foreground">The dashboard will not infer claimability from lifecycle or prerequisite counts.</p></section>;
  const attention=readiness.state==="requires_host_attention";
  return <section className={cn("rounded-lg border p-3",attention?"border-status-warn/35 bg-status-warn/5":"border-border bg-muted/20")} aria-label="Server-authoritative readiness"><div className="flex items-center gap-2">{attention?<AlertTriangle className="size-4 text-status-warn"/>:<CheckCircle2 className={cn("size-4",readiness.state==="ready"?"text-status-good":"text-muted-foreground")}/>}<h3 className="text-xs font-semibold">{humanize(readiness.state)}</h3><span className="ml-auto text-[9px] uppercase tracking-wider text-muted-foreground">Server projected</span></div>{readiness.reason_codes.length>0&&<p className="mt-2 text-[11px] text-muted-foreground">{readiness.reason_codes.map(humanize).join(" · ")}</p>}{readiness.failed_or_cancelled_prerequisite_work_ids.length>0&&<p className="mt-2 text-[11px] text-status-warn">Host decision required for: {readiness.failed_or_cancelled_prerequisite_work_ids.join(", ")}</p>}</section>;
}

function Relations({work,allWorks,onNavigate}:{work:WorkSummary;allWorks:WorkSummary[];onNavigate:(id:string)=>void}){
  const byId=new Map(allWorks.map((candidate)=>[candidate.work_id,candidate]));
  return <section aria-labelledby="work-relations-title"><div className="flex items-center gap-2"><GitBranch className="size-3.5 text-primary"/><h3 id="work-relations-title" className="text-[10px] font-semibold uppercase tracking-[.12em] text-muted-foreground">Dependency graph</h3></div><div className="mt-2 grid gap-3 sm:grid-cols-2"><RelationList label="Prerequisites" ids={work.prerequisite_work_ids} byId={byId} onNavigate={onNavigate}/><RelationList label="Successors" ids={work.successor_work_ids??[]} byId={byId} onNavigate={onNavigate}/></div></section>;
}
function RelationList({label,ids,byId,onNavigate}:{label:string;ids:string[];byId:Map<string,WorkSummary>;onNavigate:(id:string)=>void}){return <div className="rounded-lg border border-border p-2.5"><p className="text-[9px] font-semibold uppercase tracking-wider text-muted-foreground">{label} · {ids.length}</p>{ids.length?<ul className="mt-1 space-y-1">{ids.map((id)=><li key={id}><button type="button" className="flex min-h-8 w-full items-center gap-2 rounded px-1.5 text-left text-xs hover:bg-muted focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary" onClick={()=>onNavigate(id)}><span className="min-w-0 flex-1 truncate">{byId.get(id)?.title||id}</span><ArrowRight className="size-3 shrink-0"/></button></li>)}</ul>:<p className="mt-2 text-[11px] text-muted-foreground">None</p>}</div>}

function DependencyEditor({work,allWorks,action,teamId,actionsCurrent,onAction,onCompleted}:{work:WorkSummary;allWorks:WorkSummary[];action:AllowedAction;teamId:string;actionsCurrent:boolean;onAction:RoleActionExecutor;onCompleted:()=>void}){
  const [ids,setIds]=useState(()=>new Set(work.prerequisite_work_ids));const [reason,setReason]=useState("");const [busy,setBusy]=useState(false);const [status,setStatus]=useState<string|null>(null);
  useEffect(()=>{setIds(new Set(work.prerequisite_work_ids));setReason("");setStatus(null);},[work.work_id,work.work_revision]);
  const execute=async()=>{if(!actionsCurrent||busy)return;const prepared=prepareRoleAction(action,{teamId},{prerequisite_work_ids:[...ids].join(","),reason},false);if("error" in prepared){setStatus(prepared.error);return;}setBusy(true);const result=await onAction(prepared.path,prepared.body,{headers:prepared.headers});setBusy(false);if(result.ok){setStatus("Dependencies updated. Refreshing authoritative graph…");onCompleted();return;}setStatus(result.error?`${result.error.code}: ${result.error.message}`:"Canonical service rejected the dependency change.");};
  const disabled=!actionsCurrent||Boolean(action.disabled_reason)||busy;
  return <section className="rounded-lg border border-border p-3" aria-labelledby="dependency-editor-title"><div className="flex items-center gap-2"><GitBranch className="size-3.5 text-primary"/><h3 id="dependency-editor-title" className="text-xs font-semibold">Edit hard dependencies</h3></div><p className="mt-1 text-[10px] text-muted-foreground">The server validates Work existence, cycles, lifecycle and authority before committing.</p><fieldset className="mt-3 max-h-44 space-y-1 overflow-y-auto" disabled={disabled}>{allWorks.filter((candidate)=>candidate.work_id!==work.work_id).map((candidate)=><label key={candidate.work_id} className="flex min-h-9 items-center gap-2 rounded px-2 text-xs hover:bg-muted"><input type="checkbox" checked={ids.has(candidate.work_id)} onChange={(event)=>setIds((current)=>{const next=new Set(current);if(event.target.checked)next.add(candidate.work_id);else next.delete(candidate.work_id);return next;})}/><span className="min-w-0 flex-1 truncate">{candidate.title||candidate.work_id}</span><span className="font-mono text-[9px] text-muted-foreground">{candidate.work_id}</span></label>)}</fieldset><label className="mt-3 block text-xs font-medium">Reason<textarea value={reason} onChange={(event)=>setReason(event.target.value)} className="mt-1 min-h-16 w-full rounded-md border border-border bg-background p-2 text-xs" disabled={disabled} placeholder="Why this dependency set is changing"/></label><Button size="sm" className="mt-3" disabled={disabled||!reason.trim()} onClick={execute}><Save className="size-3.5"/>{busy?"Saving…":"Replace dependencies"}</Button>{(!actionsCurrent||action.disabled_reason)&&<p className="mt-2 text-[10px] text-muted-foreground">Unavailable: {!actionsCurrent?"awaiting a current authoritative view":action.disabled_reason}</p>}{status&&<p role="status" className="mt-2 text-[11px] text-muted-foreground">{status}</p>}</section>;
}

function Detail({title,source,tone}:{title:string;source:string;tone?:"warn"}){return <section className={cn(tone==="warn"&&"rounded-lg border border-status-warn/30 bg-status-warn/5 p-3")}><h3 className="mb-1 text-[10px] font-semibold uppercase tracking-[.12em] text-muted-foreground">{title}</h3>{source?<Markdown source={source} compact/>:<p className="text-xs text-muted-foreground">Not provided.</p>}</section>}
function ReferenceList({title,refs}:{title:string;refs:string[]}){if(!refs.length)return null;return <section><h3 className="text-[10px] font-semibold uppercase tracking-[.12em] text-muted-foreground">{title}</h3><ul className="mt-1 space-y-1">{refs.map((ref)=><li key={ref} className="break-all rounded-md bg-muted/35 px-2 py-1.5 font-mono text-[10px]">{ref}</li>)}</ul></section>}
function FactGrid({work}:{work:WorkSummary}){return <dl className="grid grid-cols-2 gap-px overflow-hidden rounded-lg border border-border bg-border text-xs sm:grid-cols-3"><Fact label="Phase" value={work.phase}/><Fact label="Condition" value={work.condition}/><Fact label="Resolution" value={work.phase==="closed"?work.resolution??"not recorded":"not applicable"}/><Fact label="Owner" value={work.owner_actor_ref?.id??"Unassigned"}/><Fact label="Claim" value={work.claim_mode}/><Fact label="Readiness" value={humanize(work.readiness?.state??"not projected")}/><Fact label="Prerequisites" value={`${work.prerequisite_work_ids.length}`}/><Fact label="Successors" value={`${work.successor_work_ids?.length??0}`}/><Fact label="Gates" value={`${work.gate_summary.passed}/${work.gate_summary.required} passed`}/></dl>}
function Fact({label,value}:{label:string;value:string}){return <div className="min-w-0 bg-card p-2.5"><dt className="text-[9px] uppercase tracking-wider text-muted-foreground">{label}</dt><dd className="mt-1 break-words font-medium">{value}</dd></div>}
function humanize(value:string){return value.replace(/_/g," ").replace(/^./,(letter)=>letter.toUpperCase())}
