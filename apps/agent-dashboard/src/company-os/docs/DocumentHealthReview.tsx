import { useRef, useState } from "react";
import { AlertTriangle, CheckCircle2, ClipboardList, FileSearch2, Hammer, ShieldCheck } from "lucide-react";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";
import { ArtField, EditorialTitle, ObjectEmblem } from "../visuals";

import { RelationChips } from "./RelationChips";
import { buildDocsHealthCorrectiveWorkCommand, buildDocsHealthRelationRepairCommand } from "./healthAction";
import type {
  CompanyOsCorrectiveWorkCommand,
  CompanyOsDocumentHealthData,
  CompanyOsHealthFinding,
  CompanyOsLink,
  CompanyOsRelationRepairCommand,
} from "./types";

const severityTone: Record<CompanyOsHealthFinding["severity"], "bad" | "warn" | "good" | "info"> = {
  critical: "bad",
  warning: "warn",
  info: "info",
  good: "good",
};

function severityClass(severity: CompanyOsHealthFinding["severity"]) {
  if (severity === "critical") return "border-status-danger/35 bg-status-danger/[0.06]";
  if (severity === "warning") return "border-status-warn/35 bg-status-warn/[0.07]";
  if (severity === "good") return "border-status-good/35 bg-status-good/[0.07]";
  return "border-border bg-card/70";
}

function findingLinks(finding: CompanyOsHealthFinding): CompanyOsLink[] {
  return [finding.subject, finding.related, ...(finding.affected ?? [])].filter((link): link is CompanyOsLink => Boolean(link?.id));
}

const unavailableAction = "Connect a Store-live project and provide a session capability before dispatching governed Docs Health actions.";

export function DocumentHealthReview({
  health,
  actionEnabled = false,
  onCreateCorrectiveWork,
  onRepairRelation,
}: {
  health: CompanyOsDocumentHealthData;
  actionEnabled?: boolean;
  onCreateCorrectiveWork?: (command: CompanyOsCorrectiveWorkCommand, capabilityToken: string) => Promise<boolean>;
  onRepairRelation?: (command: CompanyOsRelationRepairCommand, capabilityToken: string) => Promise<boolean>;
}) {
  const selected = health.findings.find((finding) => finding.id === health.selectedFindingId)
    ?? health.findings.find((finding) => finding.relationRepairContext)
    ?? health.findings.find((finding) => finding.correctiveWorkContext)
    ?? health.findings[0];
  const isPassing = health.status === "pass";
  const [capabilityToken, setCapabilityToken] = useState("");
  const [correctiveNote, setCorrectiveNote] = useState("");
  const [submitting, setSubmitting] = useState(false);
  const [feedback, setFeedback] = useState<string | null>(null);
  const intents = useRef<Record<string, { id: string; createdAt: string }>>({});
  const relationIntents = useRef<Record<string, { id: string; createdAt: string }>>({});
  const canCreateCorrectiveWork = Boolean(actionEnabled && onCreateCorrectiveWork && selected?.correctiveWorkContext);
  const canRepairRelation = Boolean(actionEnabled && onRepairRelation && selected?.relationRepairContext);
  const correctiveReady = canCreateCorrectiveWork && Boolean(capabilityToken.trim()) && Boolean(correctiveNote.trim()) && !submitting;
  const relationReady = canRepairRelation && Boolean(capabilityToken.trim()) && Boolean(correctiveNote.trim()) && !submitting;

  async function createCorrectiveWork() {
    if (!selected || !correctiveReady || !onCreateCorrectiveWork) return;
    const intent = intents.current[selected.id] ?? {
      id: `action-browser-docs-health-${crypto.randomUUID()}`,
      createdAt: new Date().toISOString(),
    };
    intents.current[selected.id] = intent;
    setSubmitting(true);
    setFeedback(null);
    try {
      const command = buildDocsHealthCorrectiveWorkCommand({
        finding: selected,
        note: correctiveNote,
        commandId: intent.id,
        createdAt: intent.createdAt,
      });
      const accepted = await onCreateCorrectiveWork(command, capabilityToken.trim());
      if (accepted) {
        setCapabilityToken("");
        setCorrectiveNote("");
      }
      setFeedback(accepted ? "Corrective WorkItem created in Store truth." : "Corrective WorkItem was not created. Review the action error and retry with the same intent.");
    } catch (error) {
      setFeedback(error instanceof Error ? error.message : String(error));
    } finally {
      setSubmitting(false);
    }
  }

  async function repairRelation() {
    if (!selected || !relationReady || !onRepairRelation) return;
    const intent = relationIntents.current[selected.id] ?? {
      id: `action-browser-docs-relation-${crypto.randomUUID()}`,
      createdAt: new Date().toISOString(),
    };
    relationIntents.current[selected.id] = intent;
    setSubmitting(true);
    setFeedback(null);
    try {
      const command = buildDocsHealthRelationRepairCommand({
        finding: selected,
        note: correctiveNote,
        commandId: intent.id,
        createdAt: intent.createdAt,
      });
      const accepted = await onRepairRelation(command, capabilityToken.trim());
      if (accepted) {
        setCapabilityToken("");
        setCorrectiveNote("");
      }
      setFeedback(accepted ? "Relation repair recorded in Store truth." : "Relation repair was not recorded. Review the action error and retry with the same intent.");
    } catch (error) {
      setFeedback(error instanceof Error ? error.message : String(error));
    } finally {
      setSubmitting(false);
    }
  }

  const correctiveUnavailableReason = !actionEnabled
    ? unavailableAction
    : !selected?.correctiveWorkContext
      ? "The selected finding does not expose a complete work_item.append contract."
      : !capabilityToken.trim() || !correctiveNote.trim()
        ? "Enter the session capability and a durable corrective note."
        : undefined;
  const relationUnavailableReason = !actionEnabled
    ? unavailableAction
    : !selected?.relationRepairContext
      ? "The selected finding does not expose a complete relation.append contract."
      : !capabilityToken.trim() || !correctiveNote.trim()
        ? "Enter the session capability and a durable Docs action note."
        : undefined;
  const anyGovernedAction = canCreateCorrectiveWork || canRepairRelation;
  const primaryUnavailableReason = canRepairRelation
    ? relationUnavailableReason
    : canCreateCorrectiveWork
      ? correctiveUnavailableReason
      : correctiveUnavailableReason ?? relationUnavailableReason;

  return (
    <main
      data-company-os-page="document-health"
      data-company-os-fixture={health.fixtureId}
      data-company-os-ready="true"
      className="company-workbench h-full overflow-auto bg-background"
    >
      <ArtField />
      <div className="relative mx-auto grid min-h-full max-w-[1480px] lg:grid-cols-[260px_minmax(0,1fr)_320px]">
        <aside className="hidden border-r border-border bg-card/55 p-4 backdrop-blur-sm lg:block" aria-label="Docs health navigation">
          <div className="rounded-xl border border-primary/20 bg-primary/[0.06] p-3">
            <div className="flex items-center gap-2">
              <ObjectEmblem kind="docs" className="size-8 rounded-lg" />
              <div>
                <p className="text-sm font-semibold">Docs Health</p>
                <p className="text-[11px] text-muted-foreground">Structure review</p>
              </div>
            </div>
          </div>
          <nav className="mt-5 space-y-1 text-xs">
            <a href="?surface=docs" className="flex items-center justify-between rounded-md px-2 py-2 hover:bg-accent">
              Workspace
              <span className="text-muted-foreground">{health.counts.documents}</span>
            </a>
            <a href="?surface=docs&health=structure" className="flex items-center justify-between rounded-md bg-primary/10 px-2 py-2 text-primary">
              Structure health
              <span>{health.counts.findings}</span>
            </a>
          </nav>
          <div className="mt-6 rounded-lg border border-dashed border-border p-3 text-xs leading-5 text-muted-foreground">
            Health review is a read-only projection. It can propose governed Actions, but it must not delete or mutate documents by itself.
          </div>
        </aside>

        <section className="min-w-0 px-6 py-7 sm:px-9">
          <header className="flex flex-wrap items-start justify-between gap-4">
            <div>
              <p className="text-[11px] text-muted-foreground">Company / Docs / Governance</p>
              <EditorialTitle className="mt-7">{health.title}</EditorialTitle>
              <p className="mt-3 max-w-2xl text-sm leading-6 text-muted-foreground">
                {health.description ?? "Review whether company memory has durable owners, typed records, relations, and module roots."}
              </p>
            </div>
            <Badge tone={isPassing ? "good" : health.counts.critical ? "bad" : "warn"}>
              {isPassing ? "Passing" : `${health.counts.findings} finding${health.counts.findings === 1 ? "" : "s"}`}
            </Badge>
          </header>

          <section className="mt-7 grid gap-3 sm:grid-cols-2 xl:grid-cols-4" aria-label="Docs health counts">
            {[
              ["Documents", health.counts.documents],
              ["Typed records", health.counts.typedRecords],
              ["Relations", health.counts.relations],
              ["Warnings", health.counts.warning],
            ].map(([label, value]) => (
              <div key={label} className="rounded-xl border border-border bg-card/75 p-4">
                <p className="text-[11px] uppercase tracking-wider text-muted-foreground">{label}</p>
                <p className="mt-2 text-3xl font-semibold tracking-tight">{value}</p>
              </div>
            ))}
          </section>

          <section className="mt-8 grid gap-5 xl:grid-cols-[0.95fr_1.05fr]">
            <div>
              <div className="flex items-center gap-2">
                <FileSearch2 className="size-4 text-primary" />
                <h2 className="company-editorial-title text-2xl">Findings</h2>
              </div>
              <div className="mt-3 space-y-2">
                {health.findings.length ? health.findings.map((finding) => (
                  <a
                    key={finding.id}
                    href={`?surface=docs&health=structure&finding=${encodeURIComponent(finding.id)}`}
                    data-docs-health-finding={finding.id}
                    data-company-os-ref={finding.subject?.id}
                    className={cn("block rounded-xl border p-4 hover:bg-accent/45", severityClass(finding.severity))}
                  >
                    <div className="flex items-start justify-between gap-3">
                      <div className="min-w-0">
                        <p className="text-sm font-semibold">{finding.title}</p>
                        <p className="mt-1 line-clamp-2 text-xs leading-5 text-muted-foreground">{finding.detail}</p>
                      </div>
                      <Badge tone={severityTone[finding.severity]}>{finding.severity}</Badge>
                    </div>
                  </a>
                )) : (
                  <div data-docs-health-finding="structure-healthy" className="rounded-xl border border-status-good/35 bg-status-good/[0.07] p-4">
                    <div className="flex items-center gap-2">
                      <CheckCircle2 className="size-4 text-status-good" />
                      <p className="text-sm font-semibold">Structure healthy</p>
                    </div>
                    <p className="mt-2 text-xs leading-5 text-muted-foreground">No orphan documents, missing typed-record sources, duplicate titles, or missing module roots are visible in this projection.</p>
                  </div>
                )}
              </div>
            </div>

            <div className="rounded-2xl border border-border bg-card/80 p-5 shadow-sm">
              {selected ? (
                <>
                  <div className="flex items-start justify-between gap-3">
                    <div>
                      <p className="text-[11px] uppercase tracking-wider text-muted-foreground">Selected finding</p>
                      <h2 className="mt-2 text-xl font-semibold tracking-tight">{selected.title}</h2>
                    </div>
                    <Badge tone={severityTone[selected.severity]}>{selected.kind}</Badge>
                  </div>
                  <p className="mt-3 text-sm leading-6 text-muted-foreground">{selected.detail}</p>
                  <div className="mt-5">
                    <p className="text-[11px] font-semibold uppercase tracking-wider text-muted-foreground">Affected truth</p>
                    <RelationChips links={findingLinks(selected)} className="mt-2" emptyLabel="No affected durable object supplied." />
                  </div>
                  <div className="mt-5 rounded-xl border border-border bg-background/70 p-4">
                    <div className="flex gap-2">
                      <Hammer className="mt-0.5 size-4 shrink-0 text-primary" />
                      <div>
                        <p className="text-sm font-semibold">Recommended governed action</p>
                        <p className="mt-1 text-xs leading-5 text-muted-foreground">{selected.recommendedAction}</p>
                      </div>
                    </div>
                  </div>
                  <div className="mt-5 flex flex-wrap gap-2">
                    <Button
                      disabled={!correctiveReady}
                      title={correctiveUnavailableReason}
                      onClick={() => void createCorrectiveWork()}
                      data-company-os-action-state={canCreateCorrectiveWork ? "available" : "unavailable"}
                    >
                      <ClipboardList />
                      {submitting ? "Creating…" : selected.correctiveWorkLabel ?? "Create corrective WorkItem"}
                    </Button>
                    <Button
                      disabled={!relationReady}
                      variant="outline"
                      title={relationUnavailableReason}
                      onClick={() => void repairRelation()}
                      data-docs-health-direct-action-state={canRepairRelation ? "available" : "unavailable"}
                    >
                      <ShieldCheck />
                      {submitting && canRepairRelation ? "Linking…" : selected.directActionLabel ?? "Direct Docs action"}
                    </Button>
                  </div>
                  <div className="mt-4 grid gap-2 sm:grid-cols-2">
                    <label className="text-xs font-medium text-muted-foreground">
                      Session capability
                      <input
                        data-docs-health-action-token
                        type="password"
                        autoComplete="off"
                        value={capabilityToken}
                        onChange={(event) => setCapabilityToken(event.target.value)}
                        disabled={!anyGovernedAction}
                        placeholder="Not stored"
                        className="mt-1 h-9 w-full rounded-md border border-input bg-background px-3 text-sm text-foreground disabled:bg-muted"
                      />
                    </label>
                    <label className="text-xs font-medium text-muted-foreground">
                      Corrective note
                      <input
                        data-docs-health-corrective-note
                        value={correctiveNote}
                        onChange={(event) => setCorrectiveNote(event.target.value)}
                        disabled={!anyGovernedAction}
                        placeholder="Required for audit"
                        className="mt-1 h-9 w-full rounded-md border border-input bg-background px-3 text-sm text-foreground disabled:bg-muted"
                      />
                    </label>
                  </div>
                  <p className="mt-3 text-xs leading-5 text-muted-foreground">
                    {feedback ?? primaryUnavailableReason ?? "The server validates source Document, module scope, actor permission, policy and idempotency before recording Docs truth."}
                  </p>
                </>
              ) : (
                <div className="flex min-h-64 flex-col items-center justify-center text-center">
                  <CheckCircle2 className="size-8 text-status-good" />
                  <h2 className="mt-3 text-xl font-semibold tracking-tight">No corrective work needed</h2>
                  <p className="mt-2 max-w-sm text-sm leading-6 text-muted-foreground">Docs Health will still keep the CLI and browser review page as acceptance evidence.</p>
                </div>
              )}
            </div>
          </section>
        </section>

        <aside className="hidden border-l border-border bg-card/55 p-5 backdrop-blur-sm lg:block" aria-label="Docs health policy">
          <section className="rounded-xl border border-border bg-card/75 p-4">
            <div className="flex items-center gap-2">
              <AlertTriangle className="size-4 text-status-warn" />
              <h2 className="company-editorial-title text-xl">Policy boundary</h2>
            </div>
            <p className="mt-3 text-xs leading-5 text-muted-foreground">
              No deletion without governed action. Health can flag stale or duplicate records; cleanup needs an explicit Docs Action or corrective WorkItem.
            </p>
          </section>
          <section className="mt-6 space-y-2" aria-label="Governed cleanup queue" data-docs-cleanup-queue="true">
            <p className="text-[11px] font-semibold uppercase tracking-wider text-muted-foreground">Cleanup queue</p>
            <p className="text-xs leading-5 text-muted-foreground">Rename, split, merge, archive, and migration are high-judgment operations. Health routes them to WorkItems; it does not execute them directly.</p>
            {health.cleanupQueue?.length ? (
              <div className="space-y-2">
                {health.cleanupQueue.map((item) => (
                  <a
                    key={item.id}
                    href={`?surface=docs&health=structure&finding=${encodeURIComponent(item.findingId)}`}
                    data-docs-cleanup-operation={item.operation}
                    data-company-os-ref={item.subject?.id}
                    className={cn("block rounded-lg border bg-background/70 p-3 text-xs hover:bg-accent/45", item.disabledReason ? "border-dashed border-border" : "border-primary/20")}
                  >
                    <div className="flex items-center justify-between gap-2">
                      <span className="font-medium text-foreground">{item.label}</span>
                      <Badge tone={item.disabledReason ? "info" : "warn"}>{item.route === "corrective_work_item" ? "WorkItem" : "gated"}</Badge>
                    </div>
                    <p className="mt-1 leading-5 text-muted-foreground">{item.detail}</p>
                    {item.disabledReason && <p className="mt-2 leading-4 text-muted-foreground">{item.disabledReason}</p>}
                  </a>
                ))}
              </div>
            ) : <div className="rounded-lg border border-dashed border-border p-3 text-xs leading-5 text-muted-foreground">No high-judgment cleanup candidates are visible in this projection.</div>}
          </section>
          {health.governanceAgent && (
            <section className="mt-6 space-y-2">
              <p className="text-[11px] font-semibold uppercase tracking-wider text-muted-foreground">Governance agent</p>
              <RelationChips links={[health.governanceAgent]} />
            </section>
          )}
          <section className="mt-6 space-y-2">
            <p className="text-[11px] font-semibold uppercase tracking-wider text-muted-foreground">CLI / Skill commands</p>
            {health.actionHints?.map((hint) => (
              <div key={hint.id} className="rounded-lg border border-border bg-background/70 p-3">
                <div className="flex items-center justify-between gap-2">
                  <p className="text-xs font-medium">{hint.label}</p>
                  <Badge tone={hint.disabledReason ? "info" : hint.tone === "warning" ? "warn" : "good"}>
                    {hint.disabledReason ? "gated" : "ready"}
                  </Badge>
                </div>
                <code className="mt-2 block break-words rounded-md bg-muted px-2 py-1.5 text-[11px] text-muted-foreground">{hint.command}</code>
                {hint.disabledReason && <p className="mt-2 text-[11px] leading-4 text-muted-foreground">{hint.disabledReason}</p>}
              </div>
            ))}
          </section>
          <section className="mt-6 space-y-2">
            <p className="text-[11px] font-semibold uppercase tracking-wider text-muted-foreground">Structure refs</p>
            <RelationChips links={health.structureLinks} emptyLabel="No structure refs supplied." />
          </section>
        </aside>
      </div>
    </main>
  );
}
