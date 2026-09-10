import type { MessageSummary } from "./roleViews";

// Ordinary replies retain the incoming lineage and Work context. Provider
// interaction questions have a separate answer action, never this composer.
export function ordinaryReplyContext(message:MessageSummary|undefined,authorId:string){
  if(!message||message.kind!=="message"||!message.correlation_id
    ||message.sender.kind!=="agent_member"||message.sender.id===authorId
    ||!message.recipients.some(recipient=>recipient.kind==="agent_member"&&recipient.id===authorId))return null;
  return {messageId:message.message_id,correlationId:message.correlation_id,recipientId:message.sender.id,workId:message.work_id};
}

export function messageDraftStorageKey(teamRunId:string|undefined,authorId:string,recipientId:string,kind:string,replyId?:string){
  return `agent-workspace-composer:${JSON.stringify([teamRunId??"team",authorId,recipientId,kind,replyId??null])}`;
}
