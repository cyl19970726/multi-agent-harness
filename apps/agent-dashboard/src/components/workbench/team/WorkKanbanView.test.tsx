import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import type { WorkSummary } from "../../../model/roleViews";
import { WorkKanbanView } from "./WorkKanbanView";

function renderWork(resolution: string | null, phase = "closed") {
  const work = {
    work_id: "work-one", title: "Deliver result", phase, resolution,
    condition: "normal", priority: "high", work_revision: 5,
    prerequisite_work_ids: [], successor_work_ids: [],
    readiness: { state: "not_claimable" },
  } as unknown as WorkSummary;
  return renderToStaticMarkup(<WorkKanbanView works={[work]} onSelectWork={() => undefined}/>);
}

describe("closed Work outcomes", () => {
  it.each([
    ["accepted", "Accepted", "text-status-good"],
    ["cancelled", "Cancelled", "text-muted-foreground"],
    ["failed", "Failed", "text-destructive"],
  ])("shows canonical %s without replacing priority or readiness", (resolution, label, tone) => {
    const markup = renderWork(resolution);
    expect(markup).toContain(`data-work-resolution="${resolution}"`);
    expect(markup).toContain(`>${label}</span>`);
    expect(markup).toMatch(new RegExp(`data-work-resolution="${resolution}" class="[^"]*${tone}`));
    expect(markup).toContain(">high</span>");
    expect(markup).toContain("Not claimable");
    if (resolution !== "accepted") expect(markup).not.toContain("text-status-good");
  });

  it("does not infer success from closed phase when resolution is absent or unknown", () => {
    expect(renderWork(null)).toContain("Resolution not recorded");
    expect(renderWork("future_outcome")).toContain("Resolution: future_outcome");
    expect(renderWork(null)).not.toContain(">Accepted</span>");
  });

  it("does not project a closed outcome onto an active card", () => {
    expect(renderWork(null, "active")).not.toContain("data-work-resolution");
  });
});
