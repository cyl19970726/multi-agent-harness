import { useState } from "react";
import { Button } from "@/components/ui/button";
import { ordinaryReplyContext } from "../../../model/messageComposerContext";
import type { AgentWorkspaceData, AllowedAction, MessageSummary, RoleActionExecutor } from "../../../model/roleViews";
import { AgentMessageCommandComposer } from "./AgentMessageCommandComposer";

export function AgentMemberMessageComposer({data,actions,selectedMessage,actionsCurrent,onAction,onCompleted}:{
  data:AgentWorkspaceData;actions:AllowedAction[];selectedMessage?:MessageSummary;
  actionsCurrent:boolean;onAction:RoleActionExecutor;onCompleted:()=>void;
}){
  const author={id:data.selected_agent.agent_member_ref.id,label:data.selected_agent.display_name};
  const [recipientId,setRecipientId]=useState(data.team.host_agent_id);
  const [mode,setMode]=useState("send_message");
  const [replyId,setReplyId]=useState<string|null>(null);
  const action=actions.find(item=>item.kind===mode&&item.target_ref.kind==="team_run");
  const selectedReply=ordinaryReplyContext(selectedMessage,author.id);
  const replyMessage=data.messages.find(message=>message.message_id===replyId);
  const reply=mode==="reply_message"?ordinaryReplyContext(replyMessage,author.id):null;
  const targetId=mode==="request_decision"?data.team.host_agent_id:mode==="reply_message"?reply?.recipientId:recipientId;
  const recipient=data.roster.find(item=>item.agent_member_ref.id===targetId&&item.agent_member_ref.id!==author.id);
  const hasAction=(kind:string)=>actions.some(item=>item.kind===kind&&item.target_ref.kind==="team_run");
  return <section aria-label="Member message actions">
    <div className="flex flex-wrap items-center gap-2 px-4 pt-3">
      {hasAction("send_message")&&<Button size="sm" variant="secondary" aria-pressed={mode==="send_message"} onClick={()=>setMode("send_message")}>New message</Button>}
      {hasAction("request_decision")&&<Button size="sm" variant="secondary" aria-pressed={mode==="request_decision"} onClick={()=>setMode("request_decision")}>Ask Host</Button>}
      {hasAction("reply_message")&&<Button size="sm" variant="secondary" aria-pressed={mode==="reply_message"} disabled={!selectedReply} title={selectedReply?undefined:"Select an incoming ordinary message first"} onClick={()=>{setReplyId(selectedReply!.messageId);setMode("reply_message");}}>Reply to selected message</Button>}
      {mode==="send_message"&&<label className="flex min-w-0 items-center gap-2 text-xs">To<select className="min-w-0 max-w-52 rounded border border-border bg-background px-2 py-1" aria-label="Message recipient" value={recipientId} onChange={event=>setRecipientId(event.target.value)}>{data.roster.filter(item=>item.agent_member_ref.id!==author.id).map(item=><option key={item.agent_member_ref.id} value={item.agent_member_ref.id}>{item.display_name}</option>)}</select></label>}
    </div>
    {mode==="reply_message"&&reply&&<p className="px-4 pt-2 text-xs text-muted-foreground">Replying to: “{replyMessage?.body.slice(0,160) || reply?.messageId}”. Its Work context is preserved.</p>}
    {action&&recipient&&(mode!=="reply_message"||reply)?<AgentMessageCommandComposer
      key={`${data.team.latest_run_id}:${author.id}:${recipient.agent_member_ref.id}:${mode}:${reply?.messageId??""}`}
      action={action} author={author} recipient={{id:recipient.agent_member_ref.id,label:recipient.display_name}}
      replyContext={reply??undefined} allowResponseRequired works={data.works} teamId={data.team.team_id}
      teamRunId={data.team.latest_run_id??undefined} actionsCurrent={actionsCurrent} onAction={onAction} onCompleted={onCompleted}
    />:<p className="px-4 py-3 text-xs text-muted-foreground">{mode==="reply_message"?"The reply target is unavailable in this authenticated view. Select an incoming message or start a new message.":"No authorized message route is available."}</p>}
  </section>;
}
