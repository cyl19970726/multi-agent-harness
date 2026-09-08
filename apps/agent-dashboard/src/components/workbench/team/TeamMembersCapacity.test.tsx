import { act, create } from "react-test-renderer";
import { describe, expect, it, vi } from "vitest";
import type { MemberCapacitySummary } from "../../../model/roleViews";
import { TeamMembersCapacity } from "./TeamMembersCapacity";
const member={agent_member_ref:{kind:"agent_member",id:"archived-member"},display_name:"Historical reviewer",current_member_run_ref:null,role:"reviewer",organization_status:"active",capacity:"unavailable"} as unknown as MemberCapacitySummary;
it("opens a historical durable member without granting runtime addressability", () => {
  const onOpenAgent=vi.fn(), onOpenMember=vi.fn();
  const renderer=create(<TeamMembersCapacity members={[member]} onOpenMember={onOpenMember} onOpenAgent={onOpenAgent}/>);
  const buttons=renderer.root.findAllByType("button");
  expect(buttons).toHaveLength(2);
  for(const button of buttons){expect(button.props.disabled).toBe(false);act(()=>button.props.onClick());}
  expect(onOpenAgent).toHaveBeenCalledWith("archived-member");
  expect(onOpenMember).not.toHaveBeenCalled();
  renderer.unmount();
});
it("preserves the historical MemberRun callback when durable navigation is not supplied", () => {
  const onOpenMember=vi.fn();
  const renderer=create(<TeamMembersCapacity members={[{...member,current_member_run_ref:"run-1"}]} onOpenMember={onOpenMember}/>);
  act(()=>renderer.root.findAllByType("button")[0].props.onClick());
  expect(onOpenMember).toHaveBeenCalledWith("run-1");
  renderer.unmount();
});
