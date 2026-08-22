import { useMemo, useRef } from "react";
import { AlertTriangle, ArrowRight, CheckCircle2, CircleSlash, GitBranch, Hourglass, Users } from "lucide-react";

import { Avatar } from "@/components/workbench/Avatar";
import { cn } from "@/lib/utils";
import type { MemberCapacitySummary, WorkGraph, WorkSummary } from "../../../model/roleViews";

const NODE_WIDTH = 224;
const NODE_HEIGHT = 118;
const COLUMN_GAP = 88;
const ROW_GAP = 24;
const PADDING = 20;

interface PositionedNode { work:WorkSummary; x:number; y:number; level:number }

/** Compute display coordinates only. Readiness and graph validity always come from the RoleView. */
function positionNodes(graph:WorkGraph, visibleIds:Set<string>):PositionedNode[] {
  const nodes=graph.nodes.filter((work)=>visibleIds.has(work.work_id));
  const ids=new Set(nodes.map((work)=>work.work_id));
  const levels=new Map(nodes.map((work)=>[work.work_id,0]));
  const edges=graph.edges.filter((edge)=>ids.has(edge.prerequisite_work_id)&&ids.has(edge.dependent_work_id));
  for(let pass=0;pass<nodes.length;pass+=1){
    let moved=false;
    for(const edge of edges){
      const next=(levels.get(edge.prerequisite_work_id)??0)+1;
      if(next>(levels.get(edge.dependent_work_id)??0)){levels.set(edge.dependent_work_id,next);moved=true;}
    }
    if(!moved)break;
  }
  const columns=new Map<number,WorkSummary[]>();
  for(const work of nodes){const level=Math.min(levels.get(work.work_id)??0,nodes.length);columns.set(level,[...(columns.get(level)??[]),work]);}
  return [...columns.entries()].sort(([a],[b])=>a-b).flatMap(([level,works])=>works.map((work,row)=>({work,level,x:PADDING+level*(NODE_WIDTH+COLUMN_GAP),y:PADDING+row*(NODE_HEIGHT+ROW_GAP)})));
}

function readinessPresentation(work:WorkSummary){
  const state=work.readiness?.state;
  if(state==="ready")return {label:"Ready",tone:"text-status-good",icon:CheckCircle2};
  if(state==="waiting_prerequisites")return {label:"Waiting",tone:"text-muted-foreground",icon:Hourglass};
  if(state==="requires_host_attention")return {label:"Host attention",tone:"text-status-warn",icon:AlertTriangle};
  if(state==="not_claimable")return {label:"Not claimable",tone:"text-muted-foreground",icon:CircleSlash};
  return {label:"Readiness not projected",tone:"text-muted-foreground",icon:CircleSlash};
}

export function WorkGraphView({graph,visibleWorks,membersById,selectedWorkId,onSelectWork}:{
  graph:WorkGraph;
  visibleWorks:WorkSummary[];
  membersById:Map<string,MemberCapacitySummary>;
  selectedWorkId?:string;
  onSelectWork:(workId:string)=>void;
}){
  const visibleIds=useMemo(()=>new Set(visibleWorks.map((work)=>work.work_id)),[visibleWorks]);
  const positioned=useMemo(()=>positionNodes(graph,visibleIds),[graph,visibleIds]);
  const byId=useMemo(()=>new Map(positioned.map((node)=>[node.work.work_id,node])),[positioned]);
  const refs=useRef(new Map<string,HTMLButtonElement>());
  const width=Math.max(640,...positioned.map((node)=>node.x+NODE_WIDTH+PADDING));
  const height=Math.max(180,...positioned.map((node)=>node.y+NODE_HEIGHT+PADDING));
  const edges=graph.edges.filter((edge)=>byId.has(edge.prerequisite_work_id)&&byId.has(edge.dependent_work_id));
  const focusNode=(id?:string)=>{if(id)refs.current.get(id)?.focus();};
  const handleKey=(event:React.KeyboardEvent<HTMLButtonElement>,node:PositionedNode)=>{
    const sameColumn=positioned.filter((candidate)=>candidate.level===node.level).sort((a,b)=>a.y-b.y);
    const index=sameColumn.findIndex((candidate)=>candidate.work.work_id===node.work.work_id);
    const predecessors=edges.filter((edge)=>edge.dependent_work_id===node.work.work_id).map((edge)=>edge.prerequisite_work_id);
    const successors=edges.filter((edge)=>edge.prerequisite_work_id===node.work.work_id).map((edge)=>edge.dependent_work_id);
    const target=event.key==="ArrowUp"?sameColumn[index-1]?.work.work_id:event.key==="ArrowDown"?sameColumn[index+1]?.work.work_id:event.key==="ArrowLeft"?predecessors[0]:event.key==="ArrowRight"?successors[0]:event.key==="Home"?positioned[0]?.work.work_id:event.key==="End"?positioned[positioned.length-1]?.work.work_id:undefined;
    if(target){event.preventDefault();focusNode(target);}
  };
  return <section aria-labelledby="work-graph-title" data-testid="team-work-graph" className="mt-3 hidden lg:block">
    <header className="mb-2 flex items-end justify-between gap-4"><div><h3 id="work-graph-title" className="company-editorial-title text-lg">Work graph</h3><p className="text-[11px] text-muted-foreground">Hard dependencies flow left to right. Readiness and attention are server-authoritative.</p></div><p className="text-[10px] text-muted-foreground">Arrow keys navigate connected Work</p></header>
    <div className="overflow-auto rounded-xl border border-border bg-[radial-gradient(circle_at_1px_1px,color-mix(in_srgb,var(--border)_55%,transparent)_1px,transparent_0)] [background-size:20px_20px]" tabIndex={0} aria-label="Scrollable Work dependency graph">
      <div className="relative" style={{width,height}}>
        <svg className="pointer-events-none absolute inset-0" width={width} height={height} aria-hidden="true"><defs><marker id="work-graph-arrow" markerWidth="7" markerHeight="7" refX="6" refY="3.5" orient="auto"><path d="M0 0 7 3.5 0 7z" className="fill-border"/></marker></defs>{edges.map((edge)=>{const from=byId.get(edge.prerequisite_work_id)!;const to=byId.get(edge.dependent_work_id)!;const x1=from.x+NODE_WIDTH;const y1=from.y+NODE_HEIGHT/2;const x2=to.x;const y2=to.y+NODE_HEIGHT/2;const bend=(x1+x2)/2;return <path key={`${edge.prerequisite_work_id}:${edge.dependent_work_id}`} d={`M ${x1} ${y1} C ${bend} ${y1}, ${bend} ${y2}, ${x2} ${y2}`} fill="none" className="stroke-border" strokeWidth="1.5" markerEnd="url(#work-graph-arrow)"/>;})}</svg>
        {positioned.map((node)=><GraphNode key={node.work.work_id} node={node} selected={node.work.work_id===selectedWorkId} owner={membersById.get(node.work.assignee_ref?.agent_member_id??node.work.owner_actor_ref?.id??"")} register={(element)=>{if(element)refs.current.set(node.work.work_id,element);else refs.current.delete(node.work.work_id);}} onSelect={onSelectWork} onKeyDown={handleKey}/>) }
      </div>
    </div>
  </section>;
}

function GraphNode({node,selected,owner,register,onSelect,onKeyDown}:{node:PositionedNode;selected:boolean;owner?:MemberCapacitySummary;register:(element:HTMLButtonElement|null)=>void;onSelect:(id:string)=>void;onKeyDown:(event:React.KeyboardEvent<HTMLButtonElement>,node:PositionedNode)=>void}){
  const readiness=readinessPresentation(node.work);const ReadinessIcon=readiness.icon;
  return <button ref={register} type="button" data-work-graph-node={node.work.work_id} aria-pressed={selected} onClick={()=>onSelect(node.work.work_id)} onKeyDown={(event)=>onKeyDown(event,node)} className={cn("agent-team-panel absolute rounded-[10px] p-3 text-left transition-[border-color,box-shadow]",selected&&"agent-team-selected ring-2 ring-primary/20")} style={{left:node.x,top:node.y,width:NODE_WIDTH,height:NODE_HEIGHT}}>
    <div className="flex items-center gap-2"><span className="agent-team-phase-label shrink-0">{node.work.condition!=="normal"?node.work.condition:node.work.phase}</span><span className="ml-auto text-[9px] uppercase tracking-wider text-muted-foreground">{node.work.priority}</span></div>
    <strong className="mt-2 block truncate text-[13px]" title={node.work.title||node.work.work_id}>{node.work.title||node.work.work_id}</strong>
    <span className={cn("mt-1.5 flex items-center gap-1.5 text-[10px] font-medium",readiness.tone)}><ReadinessIcon className="size-3"/>{readiness.label}</span>
    <span className="mt-2 flex min-w-0 items-center gap-1.5 border-t border-border/70 pt-2 text-[9px] text-muted-foreground">{owner?<><Avatar name={owner.display_name} identity={owner.agent_member_ref.id} size="xs"/><span className="truncate">{owner.display_name}</span></>:<><Users className="size-3"/>Unassigned</>}<span className="ml-auto flex items-center gap-1"><GitBranch className="size-3"/>{node.work.prerequisite_work_ids.length} in · {node.work.successor_work_ids?.length??0} out</span></span>
  </button>;
}

export function WorkGraphCompactList({works,selectedWorkId,onSelectWork}:{works:WorkSummary[];selectedWorkId?:string;onSelectWork:(id:string)=>void}){
  return <div className="mt-3 divide-y divide-border border-y border-border lg:hidden" data-testid="team-work-graph-compact">{works.map((work)=>{const readiness=readinessPresentation(work);const Icon=readiness.icon;return <button type="button" key={work.work_id} data-work-compact-node={work.work_id} onClick={()=>onSelectWork(work.work_id)} aria-pressed={work.work_id===selectedWorkId} className={cn("grid min-h-16 w-full grid-cols-[minmax(0,1fr)_auto] items-center gap-3 py-3 text-left",work.work_id===selectedWorkId&&"bg-primary/[0.04]")}><span className="min-w-0"><strong className="block truncate text-sm">{work.title||work.work_id}</strong><span className="mt-1 block truncate text-[10px] text-muted-foreground">{work.prerequisite_work_ids.length?`After ${work.prerequisite_work_ids.join(", ")}`:"No prerequisites"} · {work.successor_work_ids?.length??0} successors</span></span><span className={cn("flex items-center gap-1 text-[10px] font-medium",readiness.tone)}><Icon className="size-3.5"/>{readiness.label}<ArrowRight className="size-3"/></span></button>;})}</div>;
}
