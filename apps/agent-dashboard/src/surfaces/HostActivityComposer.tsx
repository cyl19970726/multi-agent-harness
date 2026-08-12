import { useEffect, useState } from "react";

import { TeamMessageComposer } from "@/components/workbench/team/TeamMessageComposer";
import {
  fetchRoleView,
  type HostConsoleData,
  type MessageSummary,
  type RoleActionExecutor,
  type RoleView,
} from "../model/roleViews";

/** Keeps Activity conversational without weakening TeamWorkspace read authority. */
export function HostActivityComposer({apiUrl,space,project,routeIdentity,teamRunId,replyTo,refreshKey,actionsCurrent,onAction,onClearReply,fixedRecipient,variant="panel",collapsibleOnMobile=true}:{
  apiUrl:string;
  space:string;
  project:string;
  routeIdentity:string;
  teamRunId?:string;
  replyTo:MessageSummary|null;
  refreshKey?:string;
  actionsCurrent:boolean;
  onAction:RoleActionExecutor;
  onClearReply:()=>void;
  fixedRecipient?:{id:string;label:string};
  variant?:"panel"|"conversation";
  collapsibleOnMobile?:boolean;
}) {
  const [view,setView] = useState<RoleView<HostConsoleData>|null>(null);
  const [error,setError] = useState<string|null>(null);
  const [refresh,setRefresh] = useState(0);
  useEffect(() => {
    let live=true;
    fetchRoleView<HostConsoleData>(apiUrl,`/v1/views/host-console/${encodeURIComponent(routeIdentity)}`,{space,project})
      .then((value) => { if(live){setView(value);setError(null);} })
      .catch((reason) => { if(live)setError(String(reason)); });
    return () => { live=false; };
  },[apiUrl,space,project,routeIdentity,refreshKey,refresh]);
  if (error) return <p role="alert" className="rounded-lg border border-status-warn/35 bg-status-warn/10 p-3 text-xs">Activity composer unavailable: {error}</p>;
  if (!view) return <div role="status" className="h-28 animate-pulse rounded-xl border border-border bg-muted/25" aria-label="Loading authenticated Activity composer"/>;
  const resolvedRunId = teamRunId ?? view.data.team_supervisor?.team_run_id;
  const scopeMismatch = Boolean(resolvedRunId && view.allowed_actions.some((action) => action.target_ref.kind === "team_run" && action.target_ref.id !== resolvedRunId));
  if (scopeMismatch) return <p role="alert" className="rounded-lg border border-destructive/35 bg-destructive/5 p-3 text-xs">Message actions do not match the selected TeamRun. Activity writes are disabled.</p>;
  return <TeamMessageComposer collapsibleOnMobile={collapsibleOnMobile && variant !== "conversation"} variant={variant} fixedRecipient={fixedRecipient} actions={view.allowed_actions} members={view.data.member_capacity} works={view.data.all_works} replyTo={replyTo} teamId={view.data.team_ref} teamRunId={resolvedRunId} actionsCurrent={actionsCurrent && view.freshness === "current"} onAction={onAction} onClearReply={onClearReply} onCompleted={() => setRefresh((value) => value+1)}/>;
}
