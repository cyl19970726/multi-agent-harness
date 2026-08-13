import { useEffect, useMemo, useState, type KeyboardEvent, type ReactNode } from "react";
import { BriefcaseBusiness, CornerDownLeft, MessageSquareText, Paperclip, SendHorizontal } from "lucide-react";

import { Button } from "@/components/ui/button";
import {
  prepareRoleAction,
  type AllowedAction,
  type RoleActionExecutor,
  type WorkSummary,
} from "../../../model/roleViews";

export function AgentMessageCommandComposer({
  action,
  actionControl,
  recipient,
  recipients,
  works,
  linkedWorkId,
  teamId,
  teamRunId,
  actionsCurrent,
  onAction,
  onCompleted,
}: {
  action: AllowedAction;
  actionControl: ReactNode;
  recipient?: {id:string;label:string};
  recipients:Array<{id:string;label:string}>;
  works:WorkSummary[];
  linkedWorkId?:string;
  teamId:string;
  teamRunId?:string;
  actionsCurrent:boolean;
  onAction:RoleActionExecutor;
  onCompleted:()=>void;
}) {
  const initialRecipient=recipient?.id??recipients[0]?.id??"";
  const [recipientId,setRecipientId]=useState(initialRecipient);
  const [workId,setWorkId]=useState(linkedWorkId??"");
  const [body,setBody]=useState("");
  const [responseRequired,setResponseRequired]=useState(false);
  const [busy,setBusy]=useState(false);
  const [status,setStatus]=useState<string|null>(null);
  useEffect(()=>{if(recipient)setRecipientId(recipient.id);},[recipient?.id]);
  useEffect(()=>{if(linkedWorkId!==undefined)setWorkId(linkedWorkId);},[linkedWorkId]);
  const recipientLabel=useMemo(()=>recipient?.label??recipients.find(item=>item.id===recipientId)?.label??"Select Agent",[recipient,recipientId,recipients]);
  const execute=async()=>{
    if(!actionsCurrent||busy)return;
    const resolvedTeamRunId=action.target_ref.kind==="team_run"?action.target_ref.id:teamRunId;
    const prepared=prepareRoleAction(action,{teamId,teamRunId:resolvedTeamRunId},{recipient_ids:recipientId,body,work_id:workId,response_required:String(responseRequired)},false);
    if("error" in prepared){setStatus(prepared.error);return;}
    setBusy(true);setStatus(null);
    const result=await onAction(prepared.path,prepared.body,{headers:prepared.headers});
    setBusy(false);
    if(!result.ok){setStatus(result.error?`${result.error.code}: ${result.error.message}`:"Canonical service rejected the message.");return;}
    setBody("");setStatus("Message recorded. Refreshing this Agent Workspace.");onCompleted();
  };
  const onMessageKeyDown=(event:KeyboardEvent<HTMLTextAreaElement>)=>{
    if(event.key!=="Enter"||event.shiftKey||event.nativeEvent.isComposing)return;
    event.preventDefault();
    if(recipientId&&body.trim()&&!busy&&!action.disabled_reason)void execute();
  };
  return <section className="aw-command-composer" aria-label="Agent command composer">
    <div className="aw-command-route" aria-label={`Command route: ${recipientLabel}`}>
      <div className="aw-command-action">{actionControl}</div>
      <label className="aw-command-target"><MessageSquareText aria-hidden="true"/><span>to</span>{recipient?<strong title={recipient.label}>{recipient.label}</strong>:<select aria-label="Recipient Agent" value={recipientId} onChange={event=>setRecipientId(event.target.value)} required><option value="">Choose an Agent</option>{recipients.map(item=><option key={item.id} value={item.id}>{item.label}</option>)}</select>}</label>
      <label className="aw-command-work"><BriefcaseBusiness aria-hidden="true"/><span>about</span><select aria-label="Related Work" value={workId} onChange={event=>setWorkId(event.target.value)}><option value="">No related Work</option>{works.map(work=><option key={work.work_id} value={work.work_id}>{work.title||work.work_id}</option>)}</select></label>
      <label className="aw-command-response"><input type="checkbox" checked={responseRequired} onChange={event=>setResponseRequired(event.target.checked)}/><span>Response requested</span></label>
    </div>
    <div className="aw-command-input">
      <textarea aria-label="Message" value={body} onChange={event=>setBody(event.target.value)} onKeyDown={onMessageKeyDown} placeholder={`Write to ${recipientLabel}…`} rows={2}/>
      <div className="aw-command-input__footer"><span className="aw-command-attachment" aria-label="Attachments are recorded through canonical evidence actions"><Paperclip aria-hidden="true"/>Evidence or file</span><span className="aw-command-submit-hint"><CornerDownLeft aria-hidden="true"/>Enter sends · Shift + Enter adds a line</span><Button size="sm" disabled={!actionsCurrent||busy||!recipientId||!body.trim()||Boolean(action.disabled_reason)} title={action.disabled_reason??undefined} onClick={execute}><SendHorizontal aria-hidden="true"/>{busy?"Sending…":"Send"}</Button></div>
    </div>
    <footer><span>Authenticated Harness Message · separate from current-turn control</span>{action.disabled_reason&&<span>{action.disabled_reason}</span>}</footer>
    {status&&<p className="aw-command-status" role="status">{status}</p>}
  </section>;
}
