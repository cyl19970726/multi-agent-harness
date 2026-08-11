import { useEffect, useMemo, useState } from "react";
import { MessageSquareReply, SendHorizontal } from "lucide-react";

import { Button } from "@/components/ui/button";
import { prepareRoleAction, type AllowedAction, type MemberCapacitySummary, type MessageSummary, type RoleActionExecutor, type WorkSummary } from "../../../model/roleViews";

const MESSAGE_ACTIONS = new Set(["send_message", "reply_message", "request_decision"]);

export function TeamMessageComposer({ actions, members, works, replyTo, teamId, teamRunId, actionsCurrent, onAction, onClearReply, onCompleted }: {
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
}) {
  const available = actions.filter((action) => MESSAGE_ACTIONS.has(action.kind));
  const action = useMemo(() => available.find((candidate) => candidate.kind === (replyTo ? "reply_message" : "send_message")) ?? available[0], [available, replyTo]);
  const [recipientIds,setRecipientIds] = useState("");
  const [workId,setWorkId] = useState("");
  const [body,setBody] = useState("");
  const [responseRequired,setResponseRequired] = useState(false);
  const [busy,setBusy] = useState(false);
  const [status,setStatus] = useState<string|null>(null);
  useEffect(() => {
    if (!replyTo) return;
    setRecipientIds(replyTo.sender.id);
    setWorkId(replyTo.work_id ?? "");
    setResponseRequired(false);
    setStatus(null);
  }, [replyTo?.message_id]);
  if (!available.length) return null;
  const execute = async () => {
    if (!action || !actionsCurrent) return;
    const prepared = prepareRoleAction(action,{teamId,teamRunId},{recipient_ids:recipientIds,body,work_id:workId,response_required:String(responseRequired),correlation_id:replyTo?.correlation_id ?? "",causation_id:replyTo?.message_id ?? ""},false);
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
  return <section aria-labelledby="team-composer-title" className="rounded-xl border border-border bg-card p-3"><div className="flex items-center justify-between gap-3"><div><h2 id="team-composer-title" className="text-sm font-semibold">{replyTo ? "Correlated reply" : "Team message"}</h2><p className="text-[10px] text-muted-foreground">Sender identity and authority are selected by the authenticated server.</p></div>{replyTo && <Button size="sm" variant="secondary" onClick={onClearReply}>Cancel reply</Button>}</div>{replyTo && <p className="mt-2 truncate rounded-md bg-muted/35 px-2 py-1.5 text-[10px] text-muted-foreground">Replying to {replyTo.sender.id} · {replyTo.message_id}</p>}<div className="mt-3 grid gap-2 sm:grid-cols-2"><label className="text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">Recipient<select value={recipientIds} onChange={(event) => setRecipientIds(event.target.value)} className="mt-1 h-10 w-full rounded-md border border-border bg-background px-2 text-xs font-normal normal-case tracking-normal"><option value="">Select member</option>{members.map((member) => <option key={member.agent_member_ref.id} value={member.agent_member_ref.id}>{member.display_name}</option>)}</select></label><label className="text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">Related Work<select value={workId} onChange={(event) => setWorkId(event.target.value)} className="mt-1 h-10 w-full rounded-md border border-border bg-background px-2 text-xs font-normal normal-case tracking-normal"><option value="">No Work link</option>{works.map((work) => <option key={work.work_id} value={work.work_id}>{work.title || work.work_id}</option>)}</select></label></div><label className="mt-2 block text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">Message<textarea value={body} onChange={(event) => setBody(event.target.value)} className="mt-1 min-h-20 w-full resize-y rounded-md border border-border bg-background p-2.5 text-sm font-normal normal-case tracking-normal" placeholder="Write a factual coordination message…"/></label><div className="mt-2 flex flex-wrap items-center gap-3"><label className="flex min-h-10 items-center gap-2 text-xs"><input type="checkbox" checked={responseRequired} onChange={(event) => setResponseRequired(event.target.checked)}/>Response required</label><Button size="sm" className="ml-auto min-h-10" disabled={!actionsCurrent || busy || !recipientIds || !body.trim() || Boolean(action?.disabled_reason)} title={!actionsCurrent ? "Awaiting a current authoritative view" : action?.disabled_reason ?? undefined} onClick={execute}>{replyTo ? <MessageSquareReply className="size-3.5"/> : <SendHorizontal className="size-3.5"/>}{busy ? "Sending…" : replyTo ? "Send reply" : "Send message"}</Button></div>{status && <p role="status" className="mt-2 text-xs text-muted-foreground">{status}</p>}</section>;
}
