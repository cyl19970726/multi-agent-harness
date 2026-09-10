import { describe, expect, it } from "vitest";
import type { MessageSummary } from "./roleViews";
import { messageDraftStorageKey, ordinaryReplyContext } from "./messageComposerContext";

const message={message_id:"mail",kind:"message",correlation_id:"thread",work_id:"work",sender:{kind:"agent_member",id:"host"},recipients:[{kind:"agent_member",id:"member"}],response_intent:"informational"} as MessageSummary;
describe("authenticated message composer context",()=>{
  it("preserves the exact lineage and Work even for an informational message",()=>{
    expect(ordinaryReplyContext(message,"member")).toEqual({messageId:"mail",correlationId:"thread",recipientId:"host",workId:"work"});
  });
  it.each(["host","outsider"])("does not treat outgoing or unaddressed messages as incoming for %s",actor=>{
    expect(ordinaryReplyContext(message,actor)).toBeNull();
  });
  it("keeps provider questions out of ordinary replies",()=>{
    expect(ordinaryReplyContext({...message,kind:"provider_interaction_request"},"member")).toBeNull();
  });
  it("refuses unknown lineage",()=>{
    expect(ordinaryReplyContext({...message,correlation_id:""},"member")).toBeNull();
  });
  it("isolates drafts between authors and reply chains",()=>{
    const key=messageDraftStorageKey("run","member","host","reply_message","mail");
    expect(key).not.toBe(messageDraftStorageKey("run","peer","host","reply_message","mail"));
    expect(key).not.toBe(messageDraftStorageKey("run","member","host","reply_message","other"));
    expect(key).not.toBe(messageDraftStorageKey("run","member","host","send_message"));
  });
});
