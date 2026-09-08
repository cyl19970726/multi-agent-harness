import { describe, expect, it } from "vitest";
import { providerDisplayName } from "@/lib/provider";
import conversationSource from "./AgentConversationWorkspace.tsx?raw";
import teamSource from "./TeamWorkspace.tsx?raw";
import { readFileSync } from "node:fs";
const workspaceCss = readFileSync(new URL("./agent-workspace.css", import.meta.url), "utf8");
import directorySource from "./Surfaces.tsx?raw";

describe("workspace authority and navigation copy", () => {
  it("does not route current responsibility or identity to retired Company OS surfaces", () => {
    expect(teamSource).not.toContain("Mission Log owns Host judgment");
    expect(teamSource).toContain("Work records responsibility and acceptance");
    expect(directorySource).not.toContain("managed from Organization");
    expect(directorySource).toContain("Team memberships record which teams they participate in");
  });

  it("uses the current Session provider for the Host native link without changing its exact target", () => {
    expect(conversationSource).toContain('href={currentSession.native_session_open_target.uri}>Continue this exact Host Session in {providerDisplayName(currentSession.provider)}');
    expect(conversationSource).not.toContain("Continue this exact Host Session in Codex Desktop");
    expect(providerDisplayName("claude")).toBe("Claude Code");
    expect(providerDisplayName("kimi")).toBe("Kimi Code");
    expect(providerDisplayName("codex")).toBe("Codex");
    expect(providerDisplayName("future-provider")).toBe("future-provider");
  });

  it("marks the unsupported history navigation disabled and explains the available alternative", () => {
    expect(conversationSource).toContain('disabled aria-label="Agent Session history unavailable"');
    expect(conversationSource).toContain("Session history navigation is not available here. Read the current native Session in the Session tab.");
  });
});

describe("mobile dialog keyboard parity", () => {
  it("uses the existing profile focus behavior for mobile sheets", () => {
    const mobileSheet = conversationSource.slice(conversationSource.indexOf("function MobileSheet"), conversationSource.indexOf("function ProfileSection"));
    expect(mobileSheet).toContain("useDialogFocus(dialogRef,closeRef,onClose)");
    expect(mobileSheet).toContain('ref={dialogRef} tabIndex={-1} role="dialog"');
    expect(mobileSheet).toContain("ref={closeRef}");
    expect(conversationSource).toContain("useDialogFocus(dialogRef,closeRef,onClose,openerRef)");
    expect(conversationSource).toContain('event.key==="Escape"');
    expect(conversationSource).toContain('event.key!=="Tab"');
    expect(conversationSource).toContain("opener?.focus()");
  });
});


describe("wrapping modebar height contract", () => {
  it("keeps child heights independent of the wrapping container", () => {
    const modebar = conversationSource.slice(conversationSource.indexOf('<div data-testid="agent-workspace-modebar"'), conversationSource.indexOf('<Tabs.Content value="session"'));
    expect(modebar).not.toContain("h-full");
    expect(modebar).toContain("agent-workspace-tabs flex h-11 shrink-0");
    expect(modebar).toContain("<RuntimeTruthStrip truth={data.runtime_truth}/>");
  });

  it("limits phone hiding to the auxiliary source note, not runtime facts", () => {
    expect(workspaceCss).toContain(".aw-modebar .aw-native-source-note{");
    expect(workspaceCss).not.toContain(".aw-modebar > div:last-child{");
  });
});
