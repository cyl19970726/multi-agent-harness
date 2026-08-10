/**
 * Retired route compatibility shell.
 *
 * Team Workspace is the only product read authority. This export remains so
 * old deep links/extensions compile while they migrate; it performs no legacy
 * joins and owns no mutation semantics.
 */
import type { WorkbenchModel } from "../model/readModel";
import type { SelectionState } from "../app/selection";
import { TeamWorkspace } from "./TeamWorkspace";
import type { RoleActionExecutor } from "../model/roleViews";

export interface TeamWarRoomProps {
  model: WorkbenchModel;
  teamRunId?: string;
  workId?: string;
  missionId?: string;
  waveId?: string;
  onSelectionChange: (selection: Partial<SelectionState>) => void;
  apiUrl?: string;
  projectBindingId?: string | null;
  executionSpaceId?: string | null;
  onAction?: RoleActionExecutor;
}

export function TeamWarRoom({model,teamRunId,onSelectionChange,apiUrl="",projectBindingId="",executionSpaceId="",onAction=async()=>false}:TeamWarRoomProps){
  const teamId=(model.snapshot.team_runs??[]).find(run=>run.id===teamRunId)?.agent_team_id??teamRunId;
  if(!teamId)return <div className="p-8 text-sm text-muted-foreground">Select an Agent Team.</div>;
  return <TeamWorkspace apiUrl={apiUrl} project={projectBindingId??""} space={executionSpaceId??""} teamId={teamId} teamRunId={teamRunId} selection={{surface:"team",teamId}} onAction={onAction} onSelectionChange={onSelectionChange}/>;
}
