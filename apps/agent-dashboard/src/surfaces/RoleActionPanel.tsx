import { useEffect, useMemo, useState } from "react";
import { Play, ShieldAlert } from "lucide-react";
import { Button } from "@/components/ui/button";
import type { AllowedAction, RoleActionExecutor } from "../model/roleViews";
import { prepareRoleAction, roleActionRoute } from "../model/roleViews";

const CRITICAL = new Set(["accept_work", "cancel_work", "reconcile_delivery", "reconcile_message_delivery", "close_member_run", "retire_member_run", "cleanup_workspace", "waive_gate", "revoke_waiver", "start_daemon", "stop_daemon"]);
const EXECUTABLE = new Set([
  "create_work", "assign_work", "rebind_work", "release_work", "accept_work",
  "cancel_work", "claim_work", "start_work", "block_work", "unblock_work", "submit_work",
  "reconcile_delivery",
  "request_changes", "revise_work", "send_message", "reply_message", "request_decision",
  "close_member_run", "reopen_member_run", "retire_member_run", "resume_native_session",
  "provision_workspace", "attach_workspace", "archive_workspace", "cleanup_workspace",
  "write_report", "write_finding", "write_failure", "request_gate_evaluation", "evaluate_gate", "waive_gate", "revoke_waiver",
  "reconcile_message_delivery", "start_daemon", "stop_daemon", "admit_provider", "diagnose",
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
  reconcile_message_delivery: [{ name: "evidence_ref", label: "Recovery evidence ref" },{name:"outcome",label:"Outcome (acknowledged or retry_safe_failure)"}],
  request_changes: [{name:"reason",label:"Requested changes",multiline:true}],
  revise_work: [{name:"result_summary",label:"Revised result",multiline:true},{name:"candidate_revision",label:"Candidate commit SHA"},{name:"artifact_refs",label:"Artifact refs (comma-separated)"},{name:"check_refs",label:"Check refs (comma-separated)"}],
  send_message: [{name:"recipient_ids",label:"Recipient AgentMember IDs (comma-separated)"},{name:"body",label:"Message",multiline:true}],
  reply_message: [{name:"recipient_ids",label:"Recipient AgentMember IDs"},{name:"body",label:"Reply",multiline:true},{name:"correlation_id",label:"Correlation ID"},{name:"causation_id",label:"Message being replied to"}],
  request_decision: [{name:"body",label:"Decision requested",multiline:true}],
  provision_workspace: [{name:"project_binding_id",label:"Project binding ID"},{name:"canonical_root",label:"Canonical workspace path"},{name:"work_id",label:"Work ID (optional)"}],
  write_report: [{name:"summary",label:"Progress report",multiline:true},{name:"evidence_refs",label:"Evidence refs"}],
  write_finding: [{name:"summary",label:"Finding summary"},{name:"detail_markdown",label:"Finding detail",multiline:true},{name:"evidence_refs",label:"Evidence refs"}],
  write_failure: [{name:"observed_failure",label:"Observed failure",multiline:true},{name:"impact",label:"Impact",multiline:true},{name:"primary_cause",label:"Primary cause"},{name:"recommended_host_decision",label:"Recommended Host decision",multiline:true},{name:"evidence_refs",label:"Evidence refs"}],
  request_gate_evaluation: [{name:"gate_type",label:"Gate type"},{name:"gate_contract_version",label:"Gate contract version"},{name:"evaluator_id",label:"Evaluator actor ID"},{name:"evaluator_version",label:"Evaluator version"}],
  evaluate_gate: [{name:"summary",label:"Evaluation summary",multiline:true},{name:"evidence_refs",label:"Evidence refs"}],
  waive_gate: [{name:"reason",label:"Waiver reason",multiline:true},{name:"evidence_refs",label:"Evidence refs"}],
  admit_provider: [{name:"provider",label:"Installed provider (for example, codex)"},{name:"execution_mode",label:"Execution mode (for example, codex_app_server)"}],
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
    if (selected.kind === "diagnose") {
      setStatus("Diagnostics are read-only. Refetching the authoritative OperatorView.");
      onCompleted?.();
      return;
    }
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
