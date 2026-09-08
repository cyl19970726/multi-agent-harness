import { describe, expect, it } from "vitest";
import type { MessageSummary } from "./roleViews";
import { hasPendingDelivery } from "./messageDelivery";
const message=(status:string,recipient_identity_id:string|null="me")=>({deliveries:[{status,recipient_identity_id}]}) as MessageSummary;
describe("recipient-scoped pending delivery",()=>{
  it.each(["queued","routed","claimed"])("includes %s for the exact recipient",status=>expect(hasPendingDelivery(message(status),"me")).toBe(true));
  it.each(["provider_received","acknowledged","failed","expired","invalidated","delivered"])("does not equate %s with unread",status=>expect(hasPendingDelivery(message(status),"me")).toBe(false));
  it("does not count another recipient or an absent recipient",()=>{
    expect(hasPendingDelivery(message("queued","other"),"me")).toBe(false);
    expect(hasPendingDelivery(message("queued",null),"me")).toBe(false);
  });
});
