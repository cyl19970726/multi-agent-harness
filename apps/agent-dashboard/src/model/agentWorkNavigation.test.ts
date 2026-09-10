import { describe, expect, it } from "vitest";
import type { WorkSummary } from "./roleViews";
import { linkedWorkLens, reconcileWorkLens, resolveWorkspaceWork } from "./agentWorkNavigation";

const current = {work_id:"current",phase:"active",owner_actor_ref:{id:"member"}} as WorkSummary;
const historical = {work_id:"history",phase:"closed",owner_actor_ref:{id:"member"}} as WorkSummary;

describe("Agent Workspace Work links",()=>{
  it("keeps Eligible when selecting an eligible peer Work",()=>{
    expect(reconcileWorkLens("eligible",{...current,eligible_member_ids:["peer"]},"peer")).toBe("eligible");
  });
  it("keeps Review when selecting owned review Work",()=>{
    expect(reconcileWorkLens("review",{...current,phase:"review"},"member")).toBe("review");
  });
  it("changes a filter that cannot show the linked historical Work",()=>{
    expect(reconcileWorkLens("current",historical,"member")).toBe("closed");
  });
  it("resolves history before current responsibility",()=>{
    expect(resolveWorkspaceWork([current,historical],"history","current")).toBe(historical);
  });
  it("does not replace an unavailable explicit link with current Work",()=>{
    expect(resolveWorkspaceWork([current],"outside","current")).toBeUndefined();
  });
  it("uses current responsibility only when there is no explicit link",()=>{
    expect(resolveWorkspaceWork([current],undefined,"current")).toBe(current);
  });
  it("reveals closed Work rather than hiding it in Current",()=>{
    expect(linkedWorkLens(historical,"member")).toBe("closed");
  });
  it("reveals a shared Work in its phase even when another member owns it",()=>{
    expect(linkedWorkLens(current,"peer")).toBe("active");
  });
  it("keeps current ownership and the no-link default",()=>{
    expect(linkedWorkLens(current,"member")).toBe("current");
    expect(linkedWorkLens(undefined,"member")).toBe("current");
  });
});
