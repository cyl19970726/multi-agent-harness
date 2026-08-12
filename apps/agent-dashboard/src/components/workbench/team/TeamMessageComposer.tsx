import { useEffect, useMemo, useState } from "react";
import { MessageSquareReply, SendHorizontal } from "lucide-react";

import { Button } from "@/components/ui/button";
import { prepareRoleAction, type AllowedAction, type MemberCapacitySummary, type MessageSummary, type RoleActionExecutor, type WorkSummary } from "../../../model/roleViews";

const MESSAGE_ACTIONS = new Set(["send_message", "reply_message", "request_decision"]);

export function TeamMessageComposer({ actions, members, works, replyTo, teamId, teamRunId, actionsCurrent, onAction, onClearReply, onCompleted, collapsibleOnMobile=false, fixedRecipient, variant="panel" }: {
  actions:AllowedAction[];
  members:MemberCapacitySummary[];
  works:WorkSummary[];
  replyTo:MessageSummary|null;
  teamId:string;
  teamRunId?:string;
  actionsCurrent:boolean;
  onAction:RoleActionExecutor;
  onClearReply:()=>void;
  onCompleted:()=>void;
  collapsibleOnMobile?:boolean;
  fixedRecipient?:{id:string;label:string};
  variant?:"panel"|"conversation";
}) {
  const available = actions.filter((action) => MESSAGE_ACTIONS.has(action.kind));
  const action = useMemo(() => available.find((candidate) => candidate.kind === (replyTo ? "reply_message" : "send_message")) ?? available[0], [available, replyTo]);
  const [recipientIds,setRecipientIds] = useState("");
  const [workId,setWorkId] = useState("");
  const [body,setBody] = useState("");
  const [responseRequired,setResponseRequired] = useState(false);
  const [busy,setBusy] = useState(false);
  const [status,setStatus] = useState<string|null>(null);
  const [mobileOpen,setMobileOpen] = useState(false);
  const [conversationDetailsOpen,setConversationDetailsOpen] = useState(false);
  useEffect(() => {
    if (!replyTo) return;
    setRecipientIds(replyTo.sender.id);
    setWorkId(replyTo.work_id ?? "");
    setResponseRequired(false);
    setStatus(null);
  }, [replyTo?.message_id]);
  useEffect(() => {
    if (fixedRecipient && !replyTo) setRecipientIds(fixedRecipient.id);
  },[fixedRecipient?.id,replyTo?.message_id]);
  if (!available.length) return null;
  const execute = async () => {
    if (!action || !actionsCurrent) return;
    const resolvedTeamRunId = action.target_ref.kind === "team_run" ? action.target_ref.id : teamRunId;
    const prepared = prepareRoleAction(action,{teamId,teamRunId:resolvedTeamRunId},{recipient_ids:recipientIds,body,work_id:workId,response_required:String(responseRequired),correlation_id:replyTo?.correlation_id ?? "",causation_id:replyTo?.message_id ?? ""},false);
    if ("error" in prepared) { setStatus(prepared.error); return; }
    setBusy(true);
    const result = await onAction(prepared.path,prepared.body,{headers:prepared.headers});
    setBusy(false);
    if (!result.ok) { setStatus(result.error ? `${result.error.code}: ${result.error.message}` : "Canonical service rejected the message."); return; }
    setStatus("Message recorded. Refetching canonical HostConsole.");
    setBody("");
    onClearReply();
    onCompleted();
  };
  if (variant === "conversation") return <section aria-labelledby="team-composer-title" className="border-t border-border bg-background/95 px-2 py-2 sm:px-3 sm:py-3"><div className="agent-team-composer mx-auto max-w-4xl rounded-xl p-2.5"><button type="button" className="mb-1 flex min-h-7 w-full items-center justify-between px-1 text-[10px] font-semibold text-muted-foreground sm:hidden" aria-expanded={conversationDetailsOpen} onClick={() => setConversationDetailsOpen((open) => !open)}><span>To {fixedRecipient?.label ?? "Team"}</span><span>{conversationDetailsOpen ? "Hide details" : "Message details"}</span></button><div className={`${conversationDetailsOpen ? "grid" : "hidden sm:grid"} min-w-0 gap-2 border-b border-border px-1 pb-2 sm:grid-cols-[auto_minmax(12rem,1fr)_auto] sm:items-center`}><div className="flex min-w-0 items-center gap-2 rounded-md bg-secondary px-2.5 py-1.5"><span className="text-[9px] font-semibold uppercase tracking-wider text-muted-foreground">To</span><span className="truncate text-xs font-semibold">{fixedRecipient?.label ?? "Team"}</span></div><label className="grid min-w-0 grid-cols-[auto_minmax(0,1fr)] items-center gap-2 text-[9px] font-semibold uppercase tracking-wider text-muted-foreground"><span className="shrink-0">Related Work</span><select value={workId} onChange={(event) => setWorkId(event.target.value)} className="h-9 min-w-0 w-full rounded-md border border-border bg-card px-2 text-xs font-normal normal-case tracking-normal"><option value="">No Work link</option>{works.map((work) => <option key={work.work_id} value={work.work_id}>{work.title || work.work_id}</option>)}</select></label><div className="flex min-w-0 flex-wrap items-center justify-between gap-2 sm:justify-start"><label className="flex min-h-9 items-center gap-1.5 whitespace-nowrap text-[10px] text-muted-foreground"><input type="checkbox" checked={responseRequired} onChange={(event) => setResponseRequired(event.target.checked)}/>Response required</label>{replyTo && <Button size="sm" variant="secondary" onClick={onClearReply}>Cancel reply</Button>}</div></div>{replyTo && <p className="mt-2 truncate rounded-md bg-primary/[0.05] px-2 py-1 text-[9px] text-muted-foreground">Correlated reply to {replyTo.sender.id} · {replyTo.message_id}</p>}<div className="mt-2 flex min-w-0 items-end gap-2"><textarea aria-label="Message" value={body} onFocus={() => setConversationDetailsOpen(true)} onChange={(event) => setBody(event.target.value)} className="min-h-11 min-w-0 flex-1 resize-y border-0 bg-transparent p-2 text-sm outline-none placeholder:text-muted-foreground/70 sm:min-h-14" rows={1} placeholder={`Write a message${fixedRecipient ? ` to ${fixedRecipient.label}` : ""}…`}/><Button size="sm" className="min-h-11 shrink-0 px-3 sm:px-4" disabled={!actionsCurrent || busy || !recipientIds || !body.trim() || Boolean(action?.disabled_reason)} title={!actionsCurrent ? "Awaiting a current authoritative view" : action?.disabled_reason ?? undefined} onClick={execute}><SendHorizontal className="size-3.5"/>{busy ? "Sending…" : replyTo ? "Reply" : "Send"}</Button></div><div className="flex items-center justify-between border-t border-border px-1 pt-2"><p className="text-[9px] text-muted-foreground">Authenticated ordinary Message · never current-turn Steer</p><p className="hidden text-[9px] text-muted-foreground sm:block">Shift + Enter for a new line</p></div>{status && <p role="status" className="mt-1 text-[10px] text-muted-foreground">{status}</p>}</div></section>;
  return <>{collapsibleOnMobile && !mobileOpen && <Button className="w-full sm:hidden" size="sm" variant="secondary" onClick={() => setMobileOpen(true)}><MessageSquareReply className="size-3.5"/>Compose team message</Button>}<section aria-labelledby="team-composer-title" className={`${collapsibleOnMobile && !mobileOpen ? "hidden sm:block " : ""}rounded-xl border border-border bg-card p-3`}><div className="flex items-center justify-between gap-3"><div><h2 id="team-composer-title" className="text-sm font-semibold">{replyTo ? "Correlated reply" : "Team message"}</h2><p className="text-[10px] text-muted-foreground">Sender identity and authority are selected by the authenticated server.</p></div>{replyTo && <Button size="sm" variant="secondary" onClick={onClearReply}>Cancel reply</Button>}</div>{replyTo && <p className="mt-2 truncate rounded-md bg-muted/35 px-2 py-1.5 text-[10px] text-muted-foreground">Replying to {replyTo.sender.id} · {replyTo.message_id}</p>}<div className="mt-2 grid gap-2 sm:grid-cols-2">{fixedRecipient ? <div className="rounded-md border border-border bg-muted/20 px-3 py-2"><p className="text-[9px] font-semibold uppercase tracking-wider text-muted-foreground">To</p><p className="mt-0.5 truncate text-xs font-medium">{fixedRecipient.label}</p></div> : <label className="text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">Recipient<select value={recipientIds} onChange={(event) => setRecipientIds(event.target.value)} className="mt-1 h-10 w-full rounded-md border border-border bg-background px-2 text-xs font-normal normal-case tracking-normal"><option value="">Select member</option>{members.map((member) => <option key={member.agent_member_ref.id} value={member.agent_member_ref.id}>{member.display_name}</option>)}</select></label>}<label className="text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">Related Work<select value={workId} onChange={(event) => setWorkId(event.target.value)} className="mt-1 h-10 w-full rounded-md border border-border bg-background px-2 text-xs font-normal normal-case tracking-normal"><option value="">No Work link</option>{works.map((work) => <option key={work.work_id} value={work.work_id}>{work.title || work.work_id}</option>)}</select></label></div><label className="mt-2 block text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">Message<textarea value={body} onChange={(event) => setBody(event.target.value)} className="mt-1 min-h-20 w-full resize-y rounded-md border border-border bg-background p-2.5 text-sm font-normal normal-case tracking-normal" placeholder="Write a factual coordination message…"/></label><div className="mt-2 flex flex-wrap items-center gap-3"><label className="flex min-h-10 items-center gap-2 text-xs"><input type="checkbox" checked={responseRequired} onChange={(event) => setResponseRequired(event.target.checked)}/>Response required</label><span className="text-[10px] text-muted-foreground">Ordinary messages queue to the recipient; they never steer an active turn.</span><Button size="sm" className="ml-auto min-h-10" disabled={!actionsCurrent || busy || !recipientIds || !body.trim() || Boolean(action?.disabled_reason)} title={!actionsCurrent ? "Awaiting a current authoritative view" : action?.disabled_reason ?? undefined} onClick={execute}>{replyTo ? <MessageSquareReply className="size-3.5"/> : <SendHorizontal className="size-3.5"/>}{busy ? "Sending…" : replyTo ? "Send reply" : "Send message"}</Button></div>{status && <p role="status" className="mt-2 text-xs text-muted-foreground">{status}</p>}</section></>;
}
