import { useEffect, useMemo, useRef, useState, type KeyboardEvent } from "react";
import { BriefcaseBusiness, CornerDownLeft, MessageSquareText, SendHorizontal, Slash, X } from "lucide-react";

import { Button } from "@/components/ui/button";
import { messageDraftStorageKey } from "../../../model/messageComposerContext";
import {
  prepareRoleAction,
  type AllowedAction,
  type RoleActionExecutor,
  type WorkSummary,
} from "../../../model/roleViews";

type ComposerDraft={body:string;workId:string;idempotencyKey:string;responseRequired:boolean};

function newIdempotencyKey(){return crypto.randomUUID()}
function readDraft(key:string):ComposerDraft{
  try{
    const value=JSON.parse(window.localStorage.getItem(key)??"null") as Partial<ComposerDraft>|null;
    return {body:typeof value?.body==="string"?value.body:"",workId:typeof value?.workId==="string"?value.workId:"",idempotencyKey:typeof value?.idempotencyKey==="string"?value.idempotencyKey:newIdempotencyKey(),responseRequired:value?.responseRequired===true};
  }catch{return {body:"",workId:"",idempotencyKey:newIdempotencyKey(),responseRequired:false};}
}

export function AgentMessageCommandComposer({
  action,
  author,
  recipient,
  replyContext,
  allowResponseRequired=false,
  works,
  linkedWorkId,
  teamId,
  teamRunId,
  actionsCurrent,
  onAction,
  onCompleted,
}: {
  action: AllowedAction;
  author:{id:string;label:string};
  recipient:{id:string;label:string};
  replyContext?:{messageId:string;correlationId:string;workId:string|null};
  allowResponseRequired?:boolean;
  works:WorkSummary[];
  linkedWorkId?:string;
  teamId:string;
  teamRunId?:string;
  actionsCurrent:boolean;
  onAction:RoleActionExecutor;
  onCompleted:()=>void;
}) {
  const resolvedTeamRunId=action.target_ref.kind==="team_run"?action.target_ref.id:teamRunId;
  const storageKey=messageDraftStorageKey(resolvedTeamRunId,author.id,recipient.id,action.kind,replyContext?.messageId);
  const initialDraft=useMemo(()=>readDraft(storageKey),[storageKey]);
  const [body,setBody]=useState(initialDraft.body);
  const [workId,setWorkId]=useState(linkedWorkId??initialDraft.workId);
  const [responseRequired,setResponseRequired]=useState(initialDraft.responseRequired);
  const idempotencyKey=useRef(initialDraft.idempotencyKey);
  const [busy,setBusy]=useState(false);
  const [status,setStatus]=useState<string|null>(null);
  const [activeOption,setActiveOption]=useState(0);
  const inputRef=useRef<HTMLTextAreaElement>(null);
  useEffect(()=>{
    if(!body&&!workId){window.localStorage.removeItem(storageKey);return;}
    window.localStorage.setItem(storageKey,JSON.stringify({body,workId,responseRequired,idempotencyKey:idempotencyKey.current} satisfies ComposerDraft));
  },[body,storageKey,workId,responseRequired]);
  useEffect(()=>{if(linkedWorkId!==undefined)setWorkId(linkedWorkId);},[linkedWorkId]);
  const effectiveWorkId=replyContext?replyContext.workId??"":workId;
  const linkedWork=useMemo(()=>works.find(work=>work.work_id===effectiveWorkId),[works,effectiveWorkId]);
  const slashMatch=replyContext?null:body.match(/^\/([^\s]*)(?:\s+(.*))?$/s);
  const paletteOpen=Boolean(slashMatch);
  const commandQuery=(slashMatch?.[1]??"").toLowerCase();
  const workPalette=commandQuery==="work";
  const workQuery=(slashMatch?.[2]??"").trim().toLowerCase();
  const filteredWorks=useMemo(()=>works.filter(work=>!workQuery||`${work.title} ${work.work_id}`.toLowerCase().includes(workQuery)).slice(0,8),[workQuery,works]);
  const options=workPalette?filteredWorks:commandQuery===""||"work".startsWith(commandQuery)?[null]:[];
  useEffect(()=>setActiveOption(0),[body]);
  const reviseDraft=(next:{body?:string;workId?:string})=>{
    idempotencyKey.current=newIdempotencyKey();
    if(next.body!==undefined)setBody(next.body);
    if(next.workId!==undefined)setWorkId(next.workId);
    setStatus(null);
  };
  const chooseOption=(index:number)=>{
    const option=options[index];
    if(!workPalette){reviseDraft({body:"/work "});window.requestAnimationFrame(()=>inputRef.current?.focus());return;}
    if(option){reviseDraft({body:"",workId:option.work_id});window.requestAnimationFrame(()=>inputRef.current?.focus());}
  };
  const execute=async()=>{
    if(!actionsCurrent||busy||paletteOpen||action.disabled_reason)return;
    if(action.kind==="reply_message"&&!replyContext){setStatus("Select an incoming message to reply.");return;}
    const prepared=prepareRoleAction(action,{teamId,teamRunId:resolvedTeamRunId},{recipient_ids:recipient.id,body,work_id:effectiveWorkId,response_required:String(allowResponseRequired&&responseRequired),correlation_id:replyContext?.correlationId??"",causation_id:replyContext?.messageId??""},false);
    if("error" in prepared){setStatus(prepared.error);return;}
    const submittedKey=idempotencyKey.current;
    setBusy(true);setStatus(null);
    let result;
    try{result=await onAction(prepared.path,prepared.body,{headers:{...prepared.headers,"Idempotency-Key":submittedKey}});}
    catch(reason){setBusy(false);setStatus(`Message was not confirmed: ${String(reason)}`);return;}
    setBusy(false);
    if(!result.ok){setStatus(result.error?`${result.error.code}: ${result.error.message}`:"Canonical service rejected the message.");return;}
    // A remounted composer may already own a newer draft under this route.
    if(readDraft(storageKey).idempotencyKey===submittedKey)window.localStorage.removeItem(storageKey);
    if(idempotencyKey.current===submittedKey){
      idempotencyKey.current=newIdempotencyKey();
      setBody("");setWorkId("");setResponseRequired(false);
    }
    setStatus("Message recorded. Refreshing this Agent Workspace.");onCompleted();
  };
  const onMessageKeyDown=(event:KeyboardEvent<HTMLTextAreaElement>)=>{
    if(event.key==="Escape"&&paletteOpen){event.preventDefault();reviseDraft({body:""});return;}
    if(paletteOpen&&event.key==="ArrowDown"){event.preventDefault();setActiveOption(value=>Math.min(value+1,Math.max(0,options.length-1)));return;}
    if(paletteOpen&&event.key==="ArrowUp"){event.preventDefault();setActiveOption(value=>Math.max(0,value-1));return;}
    if(event.key!=="Enter"||event.shiftKey||event.nativeEvent.isComposing)return;
    event.preventDefault();
    if(paletteOpen){if(options.length)chooseOption(activeOption);return;}
    if(body.trim()&&!busy&&!action.disabled_reason)void execute();
  };
  return <section className="aw-command-composer" aria-label={`Message ${recipient.label}`}>
    <div className="aw-command-route" aria-label={`Message route: ${author.label} to ${recipient.label}`}>
      <span className="aw-command-target"><MessageSquareText aria-hidden="true"/><span>{author.label} →</span><span className="aw-route-chip"><span title={recipient.label}>{recipient.label}</span></span></span>
      <span className="aw-command-work"><BriefcaseBusiness aria-hidden="true"/><span>Work context</span>{effectiveWorkId?<span className="aw-route-chip aw-route-chip--removable"><span title={linkedWork?.title||effectiveWorkId}>{linkedWork?.title||effectiveWorkId}</span>{!replyContext&&<button type="button" aria-label="Clear related Work" disabled={busy} onClick={()=>reviseDraft({workId:""})}><X aria-hidden="true"/></button>}</span>:<span>None</span>}</span>
    </div>
    <div className="aw-command-input">
      <textarea disabled={busy} ref={inputRef} aria-label="Message" value={body} onChange={event=>reviseDraft({body:event.target.value})} onKeyDown={onMessageKeyDown} placeholder={`Message ${recipient.label}…`} rows={2} aria-controls={paletteOpen?"agent-composer-slash-menu":undefined} aria-expanded={paletteOpen||undefined}/>
      {paletteOpen&&<div id="agent-composer-slash-menu" className="aw-slash-menu" role="listbox" aria-label={workPalette?"Select related Work":"Slash commands"}>
        {options.length?options.map((option,index)=><button key={option?.work_id??"work-command"} type="button" disabled={busy} role="option" aria-selected={index===activeOption} data-active={index===activeOption||undefined} onMouseEnter={()=>setActiveOption(index)} onClick={()=>chooseOption(index)}><BriefcaseBusiness aria-hidden="true"/><span><strong>{option?option.title||option.work_id:"/work"}</strong><small>{option?`${option.phase} · rev ${option.work_revision}`:"Link one visible Work as Message context"}</small></span></button>):<p>No visible Work matches “{workQuery}”.</p>}
      </div>}
      <div className="aw-command-input__footer">{!replyContext&&<button type="button" className="aw-slash-trigger" aria-label="Open slash commands" disabled={busy} onClick={()=>{reviseDraft({body:"/"});window.requestAnimationFrame(()=>inputRef.current?.focus());}}><Slash aria-hidden="true"/>Commands</button>}<span className="aw-command-submit-hint"><CornerDownLeft aria-hidden="true"/>Enter sends · Shift + Enter adds a line</span><Button size="icon" className="aw-command-send" aria-label="Send message" disabled={!actionsCurrent||busy||!body.trim()||paletteOpen||Boolean(action.disabled_reason)} title={action.disabled_reason??undefined} onClick={execute}><SendHorizontal aria-hidden="true"/><span className="sr-only">Send</span></Button></div>
    </div>
    <footer><span>Authenticated ordinary Message · immutable after send · Work is context only</span>{action.disabled_reason&&<span>{action.disabled_reason}</span>}</footer>
    {allowResponseRequired&&action.kind!=="request_decision"&&<label className="flex items-center gap-2 px-4 pb-2 text-xs text-muted-foreground"><input type="checkbox" disabled={busy} checked={responseRequired} onChange={event=>{idempotencyKey.current=newIdempotencyKey();setResponseRequired(event.target.checked);setStatus(null);}}/>Request a response</label>}
    {status&&<p className="aw-command-status" role="status">{status}</p>}
  </section>;
}
