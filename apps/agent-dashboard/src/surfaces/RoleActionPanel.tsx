import { useEffect, useMemo, useState } from "react";
import { Play, ShieldAlert } from "lucide-react";
import { Button } from "@/components/ui/button";
import type { AllowedAction, RoleActionExecutor } from "../model/roleViews";
import { prepareRoleAction, roleActionRoute } from "../model/roleViews";

const CRITICAL = new Set(["accept_work", "cancel_work", "reconcile_delivery"]);
const EXECUTABLE = new Set([
  "create_work", "assign_work", "rebind_work", "release_work", "accept_work",
  "cancel_work", "claim_work", "start_work", "block_work", "unblock_work", "submit_work",
  "reconcile_delivery",
]);

const FIELD_SPECS: Record<string, Array<{ name: string; label: string; multiline?: boolean }>> = {
  create_work: [
    { name: "work_id", label: "Work ID" },
    { name: "title", label: "Title" },
    { name: "context_markdown", label: "Context", multiline: true },
    { name: "completion_criteria_markdown", label: "Completion criteria", multiline: true },
  ],
  assign_work: [{ name: "member_run_id", label: "MemberRun ID" }],
  rebind_work: [{ name: "member_run_id", label: "Replacement MemberRun ID" }],
  cancel_work: [{ name: "reason", label: "Cancellation reason", multiline: true }],
  block_work: [{ name: "reason", label: "Blocker", multiline: true }],
  unblock_work: [{ name: "resolution", label: "Resolution evidence", multiline: true }],
  submit_work: [
    { name: "result_summary", label: "Result summary", multiline: true },
    { name: "base_revision", label: "Base revision" },
    { name: "candidate_revision", label: "Candidate commit SHA" },
    { name: "artifact_refs", label: "Artifact refs (comma-separated)" },
    { name: "check_refs", label: "Check/evidence refs (comma-separated)" },
  ],
  reconcile_delivery: [{ name: "evidence_ref", label: "Recovery evidence ref" }],
};

export function RoleActionPanel({
  actions,
  onAction,
  context,
  actionsCurrent = true,
  onCompleted,
}: {
  actions: AllowedAction[];
  onAction: RoleActionExecutor;
  context: { teamId?: string; teamRunId?: string; nodeId?: string };
  actionsCurrent?: boolean;
  onCompleted?: () => void;
}) {
  const [selected, setSelected] = useState<AllowedAction | null>(null);
  const [fields, setFields] = useState<Record<string, string>>({});
  const [confirm, setConfirm] = useState(false);
  const [status, setStatus] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const route = useMemo(() => selected ? roleActionRoute(selected, context) : null, [selected, context]);

  useEffect(() => {
    if (!actionsCurrent) {
      setSelected(null);
      setStatus("Projection is stale; actions remain disabled until an authoritative refetch completes.");
    }
  }, [actionsCurrent]);

  const choose = (action: AllowedAction) => {
    setSelected(action);
    setFields({});
    setConfirm(false);
    setStatus(null);
  };
  const execute = async () => {
    if (!selected || !actionsCurrent) return;
    const prepared = prepareRoleAction(selected, context, fields, confirm);
    if ("error" in prepared) {
      setStatus(prepared.error);
      return;
    }
    setBusy(true);
    const result = await onAction(prepared.path, prepared.body, { headers: prepared.headers });
    setBusy(false);
    if (result.ok) {
      setStatus(`Completed ${selected.kind}. Refetching canonical RoleView.`);
      onCompleted?.();
      return;
    }
    const error = result.error;
    setStatus(error
      ? `${error.code}: ${error.message}${error.resource_id ? ` (${error.resource_kind ?? "resource"} ${error.resource_id}${error.current_version != null ? ` v${error.current_version}` : ""})` : ""}`
      : "Canonical service rejected the action.");
  };

  return <section className="rounded-xl border border-border p-4" aria-labelledby="role-actions-title">
    <div className="flex items-center justify-between gap-3">
      <div><h2 id="role-actions-title" className="font-medium">Authorized actions</h2><p className="text-xs text-muted-foreground">Closed semantic intent; identity, authority, CAS and idempotency are transport-bound.</p></div>
      <ShieldAlert className="size-4 text-primary" />
    </div>
    {actions.length ? <div className="mt-3 flex flex-wrap gap-2">{actions.map((action, index) => {
      const unsupported = !EXECUTABLE.has(action.kind);
      const unresolved = !roleActionRoute(action, context);
      const missingVersion = !Number.isSafeInteger(action.required_version);
      const disabled = !actionsCurrent || Boolean(action.disabled_reason) || unsupported || unresolved || missingVersion;
      const reason = !actionsCurrent ? "Awaiting authoritative RoleView refetch" : action.disabled_reason ?? (unsupported ? "Semantic adapter unavailable" : unresolved ? "Route context unavailable" : missingVersion ? "Exact CAS unavailable" : undefined);
      return <Button key={`${action.kind}:${action.target_ref.kind}:${action.target_ref.id}:${index}`} size="sm" variant={selected === action ? "default" : "secondary"} disabled={disabled} title={reason} onClick={() => choose(action)}>{action.kind.replace(/_/g, " ")}</Button>;
    })}</div> : <p className="mt-3 text-xs text-muted-foreground">No actions are authorized for this identity and state.</p>}
    {selected && <div className="mt-4 space-y-3 rounded-lg bg-muted/35 p-3">
      <div className="text-xs"><b>{selected.kind}</b> → <code className="break-all">{route ?? "unavailable"}</code></div>
      {(FIELD_SPECS[selected.kind] ?? []).map((field) => <label key={field.name} className="block text-xs font-medium">{field.label}{field.multiline
        ? <textarea className="mt-1 min-h-20 w-full rounded-md border border-border bg-background p-2 text-xs" value={fields[field.name] ?? ""} onChange={(event) => setFields((current) => ({ ...current, [field.name]: event.target.value }))} />
        : <input className="mt-1 w-full rounded-md border border-border bg-background p-2 text-xs" value={fields[field.name] ?? ""} onChange={(event) => setFields((current) => ({ ...current, [field.name]: event.target.value }))} />}</label>)}
      {CRITICAL.has(selected.kind) && <label className="flex items-center gap-2 text-xs"><input type="checkbox" checked={confirm} onChange={(event) => setConfirm(event.target.checked)} />I confirm this critical durable action.</label>}
      <Button size="sm" disabled={busy || !actionsCurrent || !route} onClick={execute}><Play className="mr-2 size-3" />{busy ? "Executing…" : "Execute action"}</Button>
      {status && <p role="status" className="text-xs text-muted-foreground">{status}</p>}
    </div>}
  </section>;
}
