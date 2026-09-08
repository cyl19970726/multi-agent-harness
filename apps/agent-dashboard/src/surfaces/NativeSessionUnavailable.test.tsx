import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { NativeSessionUnavailable } from "./NativeSessionUnavailable";
import source from "./AgentConversationWorkspace.tsx?raw";

describe("native history read failures", () => {
  it("explains exact binding unavailability without claiming history was lost", () => {
    const markup = renderToStaticMarkup(<NativeSessionUnavailable reason="exact_session_unavailable"/>);
    expect(markup).toContain("cannot currently locate the exact native Session");
    expect(markup).toContain("does not establish whether its saved history exists");
    expect(markup).toContain("<code>exact_session_unavailable</code>");
  });
  it("separates coordinator messages from unavailable native history", () => {
    const markup = renderToStaticMarkup(<NativeSessionUnavailable reason="node_daemon_read_unavailable"/>);
    expect(markup).toContain("Coordination messages below are not the full execution transcript");
    expect(source.indexOf("<NativeSessionUnavailable reason=")).toBeLessThan(source.indexOf("    {rows.length"));
  });
  it("retains unknown codes and diagnostic detail without guessing a lifecycle state", () => {
    const markup = renderToStaticMarkup(<NativeSessionUnavailable reason="future_reason" detail="read rejected"/>);
    expect(markup).toContain("<code>future_reason</code>");
    expect(markup).toContain("read rejected");
    expect(markup).not.toContain("execution failed");
  });
});
