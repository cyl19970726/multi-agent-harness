import type { MessageSummary } from "./roleViews";
/** Coordination delivery progress, never a read receipt or provider execution result. */
export function hasPendingDelivery(message:MessageSummary, recipientId:string):boolean {
  return message.deliveries.some(delivery => delivery.recipient_identity_id === recipientId && ["queued", "routed", "claimed"].includes(delivery.status));
}
