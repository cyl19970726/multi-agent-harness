import { useMemo } from "react";
import dagre from "@dagrejs/dagre";
import {
  Background,
  BackgroundVariant,
  Controls,
  Handle,
  MarkerType,
  MiniMap,
  Position,
  ReactFlow,
  type Edge,
  type Node,
  type NodeProps,
} from "@xyflow/react";
import "@xyflow/react/dist/style.css";
import { AlertTriangle, CheckCircle2, CircleSlash, GitBranch, Hourglass, Users } from "lucide-react";

import { Avatar } from "@/components/workbench/Avatar";
import { cn } from "@/lib/utils";
import type { MemberCapacitySummary, WorkGraph, WorkSummary } from "../../../model/roleViews";

const NODE_WIDTH=236;
const NODE_HEIGHT=124;

export function workReadinessPresentation(work:WorkSummary){
  const state=work.readiness?.state;
  if(state==="ready")return {label:"Ready",tone:"text-status-good",minimap:"#238a63",icon:CheckCircle2};
  if(state==="waiting_prerequisites")return {label:"Waiting",tone:"text-muted-foreground",minimap:"#8f877e",icon:Hourglass};
  if(state==="requires_host_attention")return {label:"Host attention",tone:"text-status-warn",minimap:"#c77a24",icon:AlertTriangle};
  if(state==="not_claimable")return {label:"Not claimable",tone:"text-muted-foreground",minimap:"#aaa39b",icon:CircleSlash};
  return {label:"Readiness not projected",tone:"text-muted-foreground",minimap:"#aaa39b",icon:CircleSlash};
}

interface WorkNodeData extends Record<string,unknown>{work:WorkSummary;owner?:MemberCapacitySummary;active:boolean}
type WorkFlowNode=Node<WorkNodeData,"work">;

/** Dagre supplies deterministic display coordinates; it does not validate or authorize the graph. */
function layoutGraph(graph:WorkGraph,visibleWorks:WorkSummary[],membersById:Map<string,MemberCapacitySummary>,selectedWorkId?:string){
  const visibleIds=new Set(visibleWorks.map((work)=>work.work_id));
  const projectedEdges=graph.edges.filter((edge)=>visibleIds.has(edge.prerequisite_work_id)&&visibleIds.has(edge.dependent_work_id));
  const positions=layoutComponents(visibleWorks.map((work)=>work.work_id),projectedEdges);
  const nodes:WorkFlowNode[]=visibleWorks.map((work)=>{const point=positions.get(work.work_id)??{x:0,y:0};const ownerId=work.assignee_ref?.agent_member_id??work.owner_actor_ref?.id??"";return {id:work.work_id,type:"work",position:point,data:{work,owner:membersById.get(ownerId),active:work.work_id===selectedWorkId},draggable:false,connectable:false,selectable:true,focusable:true,ariaLabel:`${work.title||work.work_id}. ${workReadinessPresentation(work).label}. ${work.prerequisite_work_ids.length} prerequisites and ${work.successor_work_ids?.length??0} successors.`,style:{width:NODE_WIDTH,height:NODE_HEIGHT}};});
  const edges:Edge[]=projectedEdges.map((edge)=>({id:`${edge.prerequisite_work_id}:${edge.dependent_work_id}`,source:edge.prerequisite_work_id,target:edge.dependent_work_id,type:"smoothstep",focusable:true,selectable:true,animated:false,markerEnd:{type:MarkerType.ArrowClosed,width:14,height:14,color:"var(--border)"},style:{stroke:"var(--border)",strokeWidth:1.5},ariaLabel:`Hard dependency from ${edge.prerequisite_work_id} to ${edge.dependent_work_id}`}));
  return {nodes,edges};
}

function layoutComponents(ids:string[],edges:WorkGraph["edges"]){
  const adjacent=new Map(ids.map((id)=>[id,new Set<string>()]));
  for(const edge of edges){adjacent.get(edge.prerequisite_work_id)?.add(edge.dependent_work_id);adjacent.get(edge.dependent_work_id)?.add(edge.prerequisite_work_id);}
  const unseen=new Set(ids);const components:string[][]=[];
  for(const root of ids){if(!unseen.delete(root))continue;const component=[root];for(let index=0;index<component.length;index+=1){for(const next of adjacent.get(component[index])??[]){if(unseen.delete(next))component.push(next);}}components.push(component);}
  const positions=new Map<string,{x:number;y:number}>();let shelfX=0;let shelfY=0;let shelfHeight=0;const shelfWidth=1040;const gap=42;
  for(const component of components){const layout=new dagre.graphlib.Graph().setDefaultEdgeLabel(()=>({}));layout.setGraph({rankdir:"LR",ranksep:92,nodesep:34,marginx:0,marginy:0});for(const id of component)layout.setNode(id,{width:NODE_WIDTH,height:NODE_HEIGHT});for(const edge of edges){if(component.includes(edge.prerequisite_work_id)&&component.includes(edge.dependent_work_id))layout.setEdge(edge.prerequisite_work_id,edge.dependent_work_id);}dagre.layout(layout);const points=component.map((id)=>({id,...(layout.node(id)??{x:0,y:0})}));const minX=Math.min(...points.map((point)=>point.x-NODE_WIDTH/2));const minY=Math.min(...points.map((point)=>point.y-NODE_HEIGHT/2));const width=Math.max(...points.map((point)=>point.x+NODE_WIDTH/2))-minX;const height=Math.max(...points.map((point)=>point.y+NODE_HEIGHT/2))-minY;if(shelfX>0&&shelfX+width>shelfWidth){shelfX=0;shelfY+=shelfHeight+gap;shelfHeight=0;}for(const point of points)positions.set(point.id,{x:shelfX+point.x-NODE_WIDTH/2-minX,y:shelfY+point.y-NODE_HEIGHT/2-minY});shelfX+=width+gap;shelfHeight=Math.max(shelfHeight,height);}
  return positions;
}

const nodeTypes={work:WorkNode};

export function WorkGraphView({graph,visibleWorks,membersById,selectedWorkId,onSelectWork}:{graph:WorkGraph;visibleWorks:WorkSummary[];membersById:Map<string,MemberCapacitySummary>;selectedWorkId?:string;onSelectWork:(workId:string)=>void}){
  const elements=useMemo(()=>layoutGraph(graph,visibleWorks,membersById,selectedWorkId),[graph,visibleWorks,membersById,selectedWorkId]);
  return <section aria-labelledby="work-graph-title" data-testid="team-work-graph" className="mt-3 min-w-0">
    <header className="mb-2 flex flex-wrap items-end justify-between gap-2"><div><h3 id="work-graph-title" className="company-editorial-title text-lg">Work graph</h3><p className="text-[11px] text-muted-foreground">Pan, zoom and inspect hard dependencies. Layout is local; lifecycle and readiness remain server-authoritative.</p></div><p className="text-[10px] text-muted-foreground">Tab reaches nodes and edges · Enter selects</p></header>
    <div className="h-[62dvh] min-h-[28rem] overflow-hidden rounded-xl border border-border bg-card lg:h-[min(70vh,46rem)]" data-testid="work-graph-infinite-canvas">
      <ReactFlow<WorkFlowNode,Edge> nodes={elements.nodes} edges={elements.edges} nodeTypes={nodeTypes} fitView fitViewOptions={{padding:0.2,minZoom:0.35,maxZoom:1.2}} minZoom={0.15} maxZoom={2.2} panOnScroll zoomOnPinch zoomOnDoubleClick={false} nodesDraggable={false} nodesConnectable={false} elementsSelectable deleteKeyCode={null} onNodeClick={(_,node)=>onSelectWork(node.id)} proOptions={{hideAttribution:true}} aria-label="Work dependency graph infinite canvas" colorMode="system">
        <MiniMap pannable zoomable ariaLabel="Work graph minimap" nodeColor={(node)=>workReadinessPresentation((node.data as WorkNodeData).work).minimap} maskColor="color-mix(in srgb, var(--background) 68%, transparent)"/>
        <Controls showInteractive={false} aria-label="Work graph viewport controls"/>
        <Background variant={BackgroundVariant.Dots} gap={20} size={1} color="var(--border)"/>
      </ReactFlow>
    </div>
  </section>;
}

function WorkNode({data,selected}:NodeProps<WorkFlowNode>){const {work,owner,active}=data;const readiness=workReadinessPresentation(work);const Icon=readiness.icon;return <div data-work-graph-node={work.work_id} className={cn("agent-team-panel h-full rounded-[10px] border bg-card p-3 text-left shadow-sm transition-[border-color,box-shadow]",(selected||active)&&"agent-team-selected ring-2 ring-primary/20")}>
  <Handle type="target" position={Position.Left} isConnectable={false} className="!size-2 !border-background !bg-border"/>
  <div className="flex items-center gap-2"><span className="agent-team-phase-label shrink-0">{work.condition!=="normal"?work.condition:work.phase}</span><span className="ml-auto text-[9px] uppercase tracking-wider text-muted-foreground">{work.priority}</span></div>
  <strong className="mt-2 block truncate text-[13px]" title={work.title||work.work_id}>{work.title||work.work_id}</strong><span className={cn("mt-1.5 flex items-center gap-1.5 text-[10px] font-medium",readiness.tone)}><Icon className="size-3"/>{readiness.label}</span>
  <span className="mt-2 flex min-w-0 items-center gap-1.5 border-t border-border/70 pt-2 text-[9px] text-muted-foreground">{owner?<><Avatar name={owner.display_name} identity={owner.agent_member_ref.id} size="xs"/><span className="truncate">{owner.display_name}</span></>:<><Users className="size-3"/>Unassigned</>}<span className="ml-auto flex items-center gap-1"><GitBranch className="size-3"/>{work.prerequisite_work_ids.length} in · {work.successor_work_ids?.length??0} out</span></span>
  <Handle type="source" position={Position.Right} isConnectable={false} className="!size-2 !border-background !bg-border"/>
</div>}
