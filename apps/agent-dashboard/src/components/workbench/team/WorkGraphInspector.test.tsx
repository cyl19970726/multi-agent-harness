import { createRef } from "react";
import { act, create } from "react-test-renderer";
import { expect, it, vi } from "vitest";
import type { MemberCapacitySummary, WorkSummary } from "../../../model/roleViews";
import { WorkGraphInspector } from "./WorkGraphInspector";
const work={work_id:"work-one",title:"Done",phase:"closed",condition:"normal",resolution:"accepted",owner_actor_ref:{kind:"agent_member",id:"owner"},work_revision:5,prerequisite_work_ids:[],artifact_refs:[],check_refs:[],gate_summary:{passed:0,required:0}} as unknown as WorkSummary;
const owner={agent_member_ref:{kind:"agent_member",id:"owner"},display_name:"Reviewer"} as MemberCapacitySummary;
function render(candidate=work,members=[owner]){
  const open=vi.fn();
  const renderer=create(<WorkGraphInspector work={candidate} allWorks={[candidate]} teamId="team" actionsCurrent={false} onAction={vi.fn()} onCompleted={vi.fn()} onClose={vi.fn()} onNavigate={vi.fn()} onOpenMember={vi.fn()} onOpenAgent={open} members={members} closeRef={createRef()}/>);
  return {renderer,open,links:renderer.root.findAll(node=>node.type==="button"&&node.children.some(child=>typeof child==="string"&&child.includes("workspace")))};
}
it("opens the verified durable owner with no current runtime or write authority",()=>{
  const {renderer,open,links}=render();
  expect(links).toHaveLength(1);
  act(()=>links[0].props.onClick());
  expect(open).toHaveBeenCalledWith("owner");
  renderer.unmount();
});
it("does not invent a member link for an absent owner or a matching human ID",()=>{
  for(const candidate of [render(work,[]),render({...work,owner_actor_ref:{kind:"human",id:"owner"}})]){
    expect(candidate.links).toHaveLength(0);
    candidate.renderer.unmount();
  }
});
