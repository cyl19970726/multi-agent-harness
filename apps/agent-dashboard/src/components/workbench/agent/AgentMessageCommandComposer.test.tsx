import { act, create, type ReactTestRenderer } from "react-test-renderer";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { AllowedAction, RoleActionExecutor } from "../../../model/roleViews";
import { AgentMessageCommandComposer } from "./AgentMessageCommandComposer";

const action={kind:"send_message",target_ref:{kind:"team_run",id:"run"},required_version:3,disabled_reason:null} as AllowedAction;
let renderer:ReactTestRenderer;
beforeEach(()=>{
  const values=new Map<string,string>();
  vi.stubGlobal("window",{localStorage:{getItem:(key:string)=>values.get(key)??null,setItem:(key:string,value:string)=>values.set(key,value),removeItem:(key:string)=>values.delete(key)},requestAnimationFrame:(fn:()=>void)=>fn()});
});
afterEach(()=>{if(renderer)act(()=>renderer.unmount());vi.unstubAllGlobals();});
function render(onAction:RoleActionExecutor,extra:Partial<React.ComponentProps<typeof AgentMessageCommandComposer>>={}){
  act(()=>{renderer=create(<AgentMessageCommandComposer action={action} author={{id:"member",label:"Member"}} recipient={{id:"host",label:"Host"}} works={[]} teamId="team" actionsCurrent onAction={onAction} onCompleted={()=>{}} {...extra}/>);});
}
const draft=()=>renderer.root.findByType("textarea");
const send=()=>renderer.root.findByProps({"aria-label":"Send message"});
describe("ordinary message delivery intent",()=>{
  it("protects a newer remounted draft from a delayed successful receipt",async()=>{
    let resolve!: (value:{ok:true})=>void;
    const onAction=vi.fn(()=>new Promise<{ok:true}>(done=>{resolve=done;}));
    render(onAction,{allowResponseRequired:true,linkedWorkId:"work"});
    act(()=>draft().props.onChange({target:{value:"Submitted draft"}}));
    let pending!:Promise<void>;
    act(()=>{pending=send().props.onClick();});
    expect(draft().props.disabled).toBe(true);
    expect(renderer.root.findByProps({"aria-label":"Clear related Work"}).props.disabled).toBe(true);
    expect(renderer.root.findByProps({"aria-label":"Open slash commands"}).props.disabled).toBe(true);
    expect(renderer.root.findByType("input").props.disabled).toBe(true);
    act(()=>renderer.unmount());
    render(onAction);
    act(()=>draft().props.onChange({target:{value:"New draft after returning"}}));
    await act(async()=>{resolve({ok:true});await pending;});
    expect(draft().props.value).toBe("New draft after returning");
    act(()=>renderer.unmount());render(onAction);
    expect(draft().props.value).toBe("New draft after returning");
  });
  it("retains a failed draft and retry key, clearing only after a canonical receipt",async()=>{
    const onAction=vi.fn().mockResolvedValueOnce({ok:false,error:{code:"VERSION_CONFLICT",message:"stale"}}).mockResolvedValueOnce({ok:true});
    render(onAction);
    act(()=>draft().props.onChange({target:{value:"Keep this draft"}}));
    await act(async()=>send().props.onClick());
    expect(draft().props.value).toBe("Keep this draft");
    await act(async()=>send().props.onClick());
    expect(onAction.mock.calls[0][2].headers["Idempotency-Key"]).toBe(onAction.mock.calls[1][2].headers["Idempotency-Key"]);
    expect(draft().props.value).toBe("");
  });
  it("keeps the exact reply lineage and Work even outside the visible Work list",async()=>{
    const onAction=vi.fn().mockResolvedValue({ok:true});
    render(onAction,{action:{...action,kind:"reply_message"},replyContext:{messageId:"incoming",correlationId:"conversation",workId:"historical-work"}});
    act(()=>draft().props.onChange({target:{value:"Reply"}}));
    await act(async()=>send().props.onClick());
    expect(onAction.mock.calls[0][1]).toMatchObject({action:"reply_message",recipient_ids:["host"],correlation_id:"conversation",causation_id:"incoming",work_id:"historical-work"});
    expect(renderer.root.findAllByProps({"aria-label":"Clear related Work"})).toHaveLength(0);
  });
  it("keeps a transport-error draft and permits a later retry",async()=>{
    const onAction=vi.fn().mockRejectedValue(new Error("offline"));
    render(onAction);act(()=>draft().props.onChange({target:{value:"Offline draft"}}));
    await act(async()=>send().props.onClick());
    expect(draft().props.value).toBe("Offline draft");
    expect(send().props.disabled).toBe(false);
  });
  it("does not dispatch an unavailable action",async()=>{
    const onAction=vi.fn();render(onAction,{action:{...action,disabled_reason:"No current daemon"}});
    act(()=>draft().props.onChange({target:{value:"Not authorized"}}));
    await act(async()=>send().props.onClick());
    expect(onAction).not.toHaveBeenCalled();
  });
});
