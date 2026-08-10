import {useMemo,useState} from "react";
import {Play,ShieldAlert} from "lucide-react";
import {Button} from "@/components/ui/button";
import type {AllowedAction,RoleActionExecutor} from "../model/roleViews";
import {roleActionRoute} from "../model/roleViews";

const CRITICAL=new Set(["accept_work","cancel_work","retire_member_run","cleanup_workspace","waive_gate","revoke_waiver"]);
function initialPayload(action:AllowedAction){
  const value:Record<string,unknown>={};
  if(action.required_version!==null)value.expected_version=action.required_version;
  if(["request_changes","cancel_work","block_work"].includes(action.kind))value.reason="";
  if(action.kind==="unblock_work")value.resolution="";
  return JSON.stringify(value,null,2);
}

export function RoleActionPanel({actions,onAction,context,onCompleted}:{actions:AllowedAction[];onAction:RoleActionExecutor;context:{teamId?:string;teamRunId?:string;nodeId?:string};onCompleted?:()=>void}){
  const[selected,setSelected]=useState<AllowedAction|null>(null),[payload,setPayload]=useState("{}"),[confirm,setConfirm]=useState(false),[status,setStatus]=useState<string|null>(null),[busy,setBusy]=useState(false);
  const route=useMemo(()=>selected?roleActionRoute(selected,context):null,[selected,context]);
  const choose=(action:AllowedAction)=>{setSelected(action);setPayload(initialPayload(action));setConfirm(false);setStatus(null)};
  const execute=async()=>{if(!selected||!route)return;let body:unknown;try{body=JSON.parse(payload)}catch{setStatus("Payload must be valid JSON.");return}if(CRITICAL.has(selected.kind)&&!confirm){setStatus("Confirm this critical action before execution.");return}setBusy(true);const key=crypto.randomUUID();const ok=await onAction(route,body,{headers:{"Idempotency-Key":key,...(selected.required_version!==null?{"If-Match":String(selected.required_version)}:{})}});setBusy(false);setStatus(ok?`Completed ${selected.kind}. RoleView will refresh from canonical state.`:"Canonical service rejected the action; see the error banner.");if(ok)onCompleted?.()};
  return <section className="rounded-xl border border-border p-4" aria-labelledby="role-actions-title"><div className="flex items-center justify-between gap-3"><div><h2 id="role-actions-title" className="font-medium">Authorized actions</h2><p className="text-xs text-muted-foreground">Server-resolved identity, canonical route, CAS version and one-shot idempotency key.</p></div><ShieldAlert className="size-4 text-primary"/></div>{actions.length?<div className="mt-3 flex flex-wrap gap-2">{actions.map((action,index)=>{const unresolved=!roleActionRoute(action,context);const disabled=Boolean(action.disabled_reason)||unresolved;return <Button key={`${action.kind}:${action.target_ref.kind}:${action.target_ref.id}:${index}`} size="sm" variant={selected===action?"default":"secondary"} disabled={disabled} title={action.disabled_reason??(unresolved?"Canonical route context unavailable":undefined)} onClick={()=>choose(action)}>{action.kind.replace(/_/g," ")}</Button>})}</div>:<p className="mt-3 text-xs text-muted-foreground">No actions are authorized for this identity and state.</p>}{selected&&<div className="mt-4 space-y-3 rounded-lg bg-muted/35 p-3"><div className="text-xs"><b>{selected.kind}</b> → <code className="break-all">{route??"unavailable"}</code></div><label className="block text-xs font-medium" htmlFor="role-action-payload">Canonical command payload</label><textarea id="role-action-payload" className="min-h-32 w-full rounded-md border border-border bg-background p-2 font-mono text-xs" value={payload} onChange={event=>setPayload(event.target.value)}/>{CRITICAL.has(selected.kind)&&<label className="flex items-center gap-2 text-xs"><input type="checkbox" checked={confirm} onChange={event=>setConfirm(event.target.checked)}/>I confirm this critical action and its durable effects.</label>}<Button size="sm" disabled={busy||!route} onClick={execute}><Play className="mr-2 size-3"/>{busy?"Executing…":"Execute canonical action"}</Button>{status&&<p role="status" className="text-xs text-muted-foreground">{status}</p>}</div>}</section>
}
