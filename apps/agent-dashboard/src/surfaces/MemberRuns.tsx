/** Retired MemberRun route compatibility shell; MemberWorkbench is authority. */
import type { WorkbenchModel } from "../model/readModel";
import type { SelectionState } from "../app/selection";
import { MemberWorkbench } from "./MemberWorkbench";
import type { RoleActionExecutor } from "../model/roleViews";

export interface MemberRunFocusProps {
  model: WorkbenchModel;
  memberRunId: string;
  onSelectionChange: (selection: Partial<SelectionState>) => void;
  apiUrl?: string;
  projectBindingId?: string | null;
  executionSpaceId?: string | null;
  onAction?: RoleActionExecutor;
}

export function MemberRunFocus({memberRunId,apiUrl="",projectBindingId="",executionSpaceId="",onAction=async()=>({ok:false,error:{code:"RETIRED_ROUTE",message:"Retired route",status:410}})}:MemberRunFocusProps){
  return <MemberWorkbench apiUrl={apiUrl} project={projectBindingId??""} space={executionSpaceId??""} memberRunId={memberRunId} onAction={onAction} actionsCurrent={false}/>;
}
