import * as ScrollArea from "@radix-ui/react-scroll-area";
import { AlertCircle, CheckCircle2, CircleDot, ExternalLink, FileCheck2 } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";

import { Markdown } from "@/components/workbench/Markdown";
import { WorkspaceState } from "@/components/workbench/agent/AgentWorkspacePrimitives";
import type { AgentWorkspaceRosterItem, AllowedAction, WorkSummary } from "@/model/roleViews";
import { DockModuleState } from "./DockModuleState";
import type { DockModuleStatus } from "./types";

type WorkLens = "current" | "all" | "attention";

export function WorkDock({
  works,
  roster,
  selectedAgentId,
  currentWorkId,
  initialWorkId,
  expanded,
  status,
  allowedActions = [],
  renderAuthorizedActions,
  onSelectWork,
}: {
  works: WorkSummary[];
  roster: AgentWorkspaceRosterItem[];
  selectedAgentId: string;
  currentWorkId: string | null;
  initialWorkId?: string;
  expanded: boolean;
  status?: DockModuleStatus;
  allowedActions?: AllowedAction[];
  renderAuthorizedActions?: (actions: AllowedAction[], work: WorkSummary) => React.ReactNode;
  onSelectWork?: (work: WorkSummary) => void;
}) {
  const [lens, setLens] = useState<WorkLens>("current");
  const [selectedWorkId, setSelectedWorkId] = useState(initialWorkId ?? currentWorkId ?? works[0]?.work_id ?? "");
  const listViewportRef = useRef<HTMLDivElement>(null);
  const detailViewportRef = useRef<HTMLDivElement>(null);
  const owns = (work: WorkSummary) => work.owner_actor_ref?.id === selectedAgentId;
  const visible = useMemo(() => works.filter((work) => {
    if (lens === "all") return true;
    if (lens === "attention") return work.phase === "review" || work.condition === "blocked";
    return owns(work) && work.phase !== "closed";
  }).sort((left, right) => {
    if (left.work_id === currentWorkId) return -1;
    if (right.work_id === currentWorkId) return 1;
    return Date.parse(right.updated_at) - Date.parse(left.updated_at);
  }), [works, lens, selectedAgentId, currentWorkId]);
  useEffect(() => {
    if (works.some((work) => work.work_id === selectedWorkId)) return;
    setSelectedWorkId(currentWorkId ?? works[0]?.work_id ?? "");
  }, [works, currentWorkId, selectedWorkId]);
  const selected = works.find((work) => work.work_id === selectedWorkId) ?? visible[0];
  const select = (work: WorkSummary) => {
    setSelectedWorkId(work.work_id);
    onSelectWork?.(work);
    if (!expanded) detailViewportRef.current?.scrollTo({ top: 0, behavior: "auto" });
  };

  if (status && status.kind !== "ready" && status.kind !== "stale") return <DockModuleState status={status}/>;
  return <div className="agent-work-dock" data-expanded={expanded || undefined}>
    {status?.kind === "stale" && (
      <DockModuleState status={status}/>
    )}
    <div className="agent-dock-filterbar" aria-label="Work filters">
      {(["current", "all", "attention"] as const).map((item) => <button key={item} type="button" aria-pressed={lens === item} onClick={() => setLens(item)}>{item}</button>)}
      <span>{visible.length}</span>
    </div>
    <div className="agent-dock-split">
      <ScrollArea.Root className="agent-dock-list" data-testid="work-dock-list"><ScrollArea.Viewport ref={listViewportRef} className="size-full">
        {visible.length ? <ol>{visible.map((work) => {
          const owner = roster.find((item) => item.agent_member_ref.id === work.owner_actor_ref?.id);
          const attention = work.phase === "review" || work.condition === "blocked";
          return <li key={work.work_id}><button type="button" aria-current={selected?.work_id === work.work_id || undefined} onClick={() => select(work)}>
            <span className="agent-dock-row-title">{work.title || "Untitled Work"}</span>
            <span className="agent-dock-row-meta"><span>{humanize(work.phase)}</span><span>{owner?.display_name ?? (work.owner_actor_ref ? "Assigned" : "Unassigned")}</span><time>{formatTime(work.updated_at)}</time></span>
            {attention && <span className="agent-dock-attention"><AlertCircle aria-hidden="true"/>{work.condition === "blocked" ? "Blocked" : "Host review"}</span>}
          </button></li>;
        })}</ol> : <DockModuleState emptyTitle="No Work in this view" emptyDetail="Change the filter to inspect another canonical Work set."/>}
      </ScrollArea.Viewport></ScrollArea.Root>
      <ScrollArea.Root className="agent-dock-detail" data-testid="work-dock-detail"><ScrollArea.Viewport ref={detailViewportRef} className="size-full">
        {selected ? <WorkDetail work={selected} actions={allowedActions.filter((action) => action.target_ref.kind === "work" && action.target_ref.id === selected.work_id)} renderAuthorizedActions={renderAuthorizedActions}/> : <DockModuleState emptyTitle="Select Work" emptyDetail="Choose a Work to read its outcome, contract and evidence."/>}
      </ScrollArea.Viewport></ScrollArea.Root>
    </div>
  </div>;
}

function WorkDetail({ work, actions, renderAuthorizedActions }: { work: WorkSummary; actions: AllowedAction[]; renderAuthorizedActions?: (actions: AllowedAction[], work: WorkSummary) => React.ReactNode }) {
  const outcome = work.phase === "review" ? "Awaiting Host review" : work.phase === "closed" ? humanize(work.resolution ?? "Closed") : work.condition === "blocked" ? "Blocked" : humanize(work.phase);
  return <article className="agent-work-detail" aria-label={`${work.title || "Work"} details`}>
    <header><p>Current outcome</p><h2>{work.title || "Untitled Work"}</h2><WorkspaceState label={outcome} tone={work.condition === "blocked" ? "bad" : work.phase === "review" ? "warn" : work.phase === "closed" ? "good" : "running"}/><span>{work.work_id} · revision {work.work_revision}</span></header>
    <WorkDetailSection title="Objective"><Markdown source={work.context_markdown || "No objective was recorded."}/></WorkDetailSection>
    <WorkDetailSection title="Acceptance"><Markdown source={work.completion_criteria_markdown || "No acceptance contract was recorded."}/></WorkDetailSection>
    <WorkDetailSection title="Result" icon={<CheckCircle2/>}>{work.latest_report_ref ? <><p>{work.result_summary || "Result submitted."}</p><small>{work.latest_report_ref}</small></> : <p className="agent-dock-muted">No Member Result submitted.</p>}</WorkDetailSection>
    <WorkDetailSection title="Evidence" icon={<FileCheck2/>}>{work.artifact_refs.length || work.check_refs.length ? <ul>{[...work.artifact_refs, ...work.check_refs].map((ref) => <li key={ref}><ExternalLink aria-hidden="true"/>{ref}</li>)}</ul> : <p className="agent-dock-muted">No artifact or check evidence recorded.</p>}</WorkDetailSection>
    <WorkDetailSection title="Review"><p>Gates {work.gate_summary.passed}/{work.gate_summary.required}</p><p>{work.phase === "review" ? "Host review required." : work.phase === "closed" ? `Host resolution: ${humanize(work.resolution ?? "closed")}.` : "No Host review is currently required."}</p></WorkDetailSection>
    <WorkDetailSection title="History" icon={<CircleDot/>}>{work.latest_event ? <p>{humanize(work.latest_event.kind)} · {formatTime(work.latest_event.created_at)}</p> : <p className="agent-dock-muted">No latest Work event projected.</p>}</WorkDetailSection>
    {renderAuthorizedActions && actions.some((action) => !action.disabled_reason) && <WorkDetailSection title="Authorized actions">{renderAuthorizedActions(actions, work)}</WorkDetailSection>}
  </article>;
}

function WorkDetailSection({ title, icon, children }: { title: string; icon?: React.ReactNode; children: React.ReactNode }) {
  return <section><h3>{icon}{title}</h3><div>{children}</div></section>;
}

function humanize(value: string) { return value.split(/[_-]+/).filter(Boolean).map((part) => part[0]?.toUpperCase() + part.slice(1)).join(" "); }
function formatTime(value: string) { const parsed = Date.parse(value); return Number.isFinite(parsed) ? new Intl.DateTimeFormat(undefined, { month: "short", day: "numeric", hour: "2-digit", minute: "2-digit" }).format(parsed) : "Time unavailable"; }
