import { Database, FlaskConical, Plug, Settings2, Wrench } from "lucide-react";
import type { ReactNode } from "react";

import canonicalFixture from "../../../../docs/design/company-os-v1/fixtures/company-os-trademark-v1.json";
import type { SelectionState, SurfaceId } from "@/app/selection";
import type { WorkbenchModel } from "@/model/readModel";
import { resolveCompanyOsData, type ResolvedCompanyOsData } from "./sourceTruth";

import {
  BasicDocumentPage,
  CompanyHome,
  DocumentHealthReview,
  DocsWorkspace,
  StructuredDocumentView,
  adaptCompanyOsDocsProjection,
} from "./docs";
import {
  ApprovalFocus,
  FinancePage,
  GovernanceProposalFocus,
  HumanMemberFocus,
  OrganizationPage,
  StandingAgentFocus,
  WorkItemFocus,
  adaptTrademarkOperationsProjection,
} from "./operations";
import { WorkOperatingPage } from "./work/WorkOperatingPage";

type CompanyOsPage =
  | "home"
  | "docs-workspace"
  | "document-health"
  | "document-focus"
  | "workboard"
  | "work-item-focus"
  | "finance"
  | "agents-organization"
  | "standing-agent-focus"
  | "governance-proposal"
  | "approval-focus"
  | "business-module-focus"
  | "human-member-focus";

declare global {
  interface Window {
    /** Deterministic visual fixture injected by the Company OS capture runner. */
    __COMPANY_OS_FIXTURE__?: unknown;
  }
}

const COMPANY_OS_SURFACES = new Set<SurfaceId>([
  "home",
  "docs",
  "organization",
  "work",
  "approvals",
  "finance",
  "providers",
  "plugins",
  "settings",
]);

export function isCompanyOsSurface(surface: SurfaceId): boolean {
  return COMPANY_OS_SURFACES.has(surface);
}

function companyOsProjection(model: WorkbenchModel): unknown {
  const snapshot = model.snapshot as unknown as Record<string, unknown>;
  return snapshot.company_os;
}

export function resolveCompanyOsRouteData(model: WorkbenchModel): ResolvedCompanyOsData {
  const injected = typeof window === "undefined" ? undefined : window.__COMPANY_OS_FIXTURE__;
  return resolveCompanyOsData({
    injected,
    snapshotProjection: companyOsProjection(model),
    fallback: canonicalFixture,
  });
}

function fixtureId(fixture: unknown): string {
  return fixture && typeof fixture === "object" && !Array.isArray(fixture) && typeof (fixture as Record<string, unknown>).fixture_id === "string"
    ? (fixture as Record<string, unknown>).fixture_id as string
    : "company-os-trademark-v1";
}

function selectedPage(selection: SelectionState): CompanyOsPage | undefined {
  switch (selection.surface) {
    case "home":
      return "home";
    case "docs":
      if (selection.docsHealth) return "document-health";
      if (selection.moduleId) return "business-module-focus";
      if (selection.documentId) return "document-focus";
      return "docs-workspace";
    case "work":
      return selection.workItemId ? "work-item-focus" : "workboard";
    case "finance":
      return "finance";
    case "organization":
      if (selection.proposalId) return "governance-proposal";
      if (selection.standingAgentId) return "standing-agent-focus";
      if (selection.personId) return "human-member-focus";
      return "agents-organization";
    case "approvals":
      return "approval-focus";
    default:
      return undefined;
  }
}

function DataTruthBanner({ resolved }: { resolved: ResolvedCompanyOsData }) {
  if (resolved.mode === "store-live" && resolved.source) {
    return (
      <div
        className="flex h-8 shrink-0 items-center gap-2 border-b border-status-good/25 bg-status-good/5 px-4 text-[11px] text-muted-foreground"
        data-company-os-data-mode="store-live"
        role="status"
      >
        <Database className="size-3.5 text-status-good" aria-hidden />
        <span className="font-medium text-foreground">Live · Store-backed Company OS</span>
        <span className="hidden sm:inline">{resolved.source.project_id} · {resolved.source.revision}</span>
      </div>
    );
  }

  const copy = resolved.mode === "prototype-fixture"
    ? "Prototype · fixed fixture fallback"
    : resolved.mode === "capture-fixture"
      ? "Prototype · deterministic capture fixture"
      : "Prototype · unverified snapshot projection";
  return (
    <div
      className="flex h-8 shrink-0 items-center gap-2 border-b border-border bg-status-warn/5 px-4 text-[11px] text-muted-foreground"
      data-company-os-data-mode={resolved.mode}
      role="status"
    >
      <FlaskConical className="size-3.5 text-status-warn" aria-hidden />
      <span className="font-medium text-foreground">{copy}</span>
      <span className="hidden sm:inline">This surface is not claiming live Company OS persistence.</span>
    </div>
  );
}

function CompanyOsRouteRoot({
  page,
  resolved,
  children,
}: {
  page: CompanyOsPage;
  resolved: ResolvedCompanyOsData;
  children: ReactNode;
}) {
  const isLive = resolved.mode === "store-live";
  const isFixture = resolved.mode === "capture-fixture" || resolved.mode === "prototype-fixture";
  return (
    <div
      className="flex h-full min-h-0 min-w-0 flex-1 flex-col"
      data-company-os-page={page}
      data-company-os-fixture={isFixture ? fixtureId(resolved.value) : undefined}
      data-company-os-ready="true"
      data-company-os-prototype={isLive ? "false" : "true"}
      data-company-os-data-mode={resolved.mode}
    >
      <DataTruthBanner resolved={resolved} />
      <div className="min-h-0 min-w-0 flex-1">{children}</div>
    </div>
  );
}

function PlatformPlaceholder({ surface }: { surface: "providers" | "plugins" | "settings" }) {
  const details = {
    providers: { icon: Wrench, title: "Providers", body: "Provider runtimes remain an execution capability. They do not define company membership or responsibility." },
    plugins: { icon: Plug, title: "Plugins", body: "Plugins extend governed capabilities after their object and action contracts are stable." },
    settings: { icon: Settings2, title: "Settings", body: "Company, policy, access, and execution settings will be separated by authority boundary." },
  }[surface];
  const Icon = details.icon;
  return (
    <main className="h-full overflow-auto bg-background p-5 sm:p-8">
      <div className="mx-auto max-w-3xl rounded-lg border border-border bg-card p-6">
        <Icon className="size-5 text-primary" aria-hidden />
        <h1 className="mt-4 text-2xl font-semibold tracking-tight">{details.title}</h1>
        <p className="mt-2 max-w-xl text-sm leading-6 text-muted-foreground">{details.body}</p>
        <p className="mt-6 rounded-md border border-dashed border-border p-3 text-xs text-muted-foreground">
          Platform shell only · no live settings are represented in the Company OS fixture.
        </p>
      </div>
    </main>
  );
}

/**
 * Routes Company OS product pages independently from execution surfaces. The
 * shared adapters accept either an authority-verified Store projection or an
 * explicitly labelled prototype fixture. Presentation remains read-only until
 * a governed browser Action transport is connected separately.
 */
export function CompanyOsRouter({ model, selection, actionsEnabled = false, onAction, onSelectionChange }: { model: WorkbenchModel; selection: SelectionState; actionsEnabled?: boolean; onAction?: (path: string, body?: unknown, options?: { headers?: Readonly<Record<string, string>> }) => Promise<boolean>; onSelectionChange?: (selection: Partial<SelectionState>) => void }) {
  if (selection.surface === "providers" || selection.surface === "plugins" || selection.surface === "settings") {
    return <PlatformPlaceholder surface={selection.surface} />;
  }

  const page = selectedPage(selection);
  if (!page) return null;
  const resolved = resolveCompanyOsRouteData(model);
  const docs = adaptCompanyOsDocsProjection(resolved.value, {
    documentId: selection.documentId,
    moduleId: selection.moduleId,
  });
  const operations = adaptTrademarkOperationsProjection(resolved.value, { workItemId: selection.workItemId });

  let content: ReactNode;
  switch (page) {
    case "home": content = <CompanyHome data={docs.home} />; break;
    case "docs-workspace": content = <DocsWorkspace workspace={docs.workspace} />; break;
    case "document-health": content = <DocumentHealthReview health={docs.health} actionEnabled={actionsEnabled && resolved.mode === "store-live"} onCreateCorrectiveWork={onAction ? (command, capabilityToken) => onAction("/v1/company-os/actions/dispatch", command, { headers: { "X-Harness-Company-OS-Token": capabilityToken } }) : undefined} onRepairRelation={onAction ? (command, capabilityToken) => onAction("/v1/company-os/actions/dispatch", command, { headers: { "X-Harness-Company-OS-Token": capabilityToken } }) : undefined} />; break;
    case "document-focus": content = <BasicDocumentPage document={docs.document} actionEnabled={actionsEnabled && resolved.mode === "store-live"} onDocsAction={onAction ? (command, capabilityToken) => onAction("/v1/company-os/actions/dispatch", command, { headers: { "X-Harness-Company-OS-Token": capabilityToken } }) : undefined} />; break;
    case "workboard": content = <WorkOperatingPage source={resolved.value} />; break;
    case "work-item-focus": content = <WorkItemFocus data={operations} actionEnabled={actionsEnabled && resolved.mode === "store-live"} onTransition={onAction ? (command, capabilityToken) => onAction("/v1/company-os/actions/dispatch", command, { headers: { "X-Harness-Company-OS-Token": capabilityToken } }) : undefined} />; break;
    case "finance": content = <FinancePage data={operations} />; break;
    case "agents-organization": content = <OrganizationPage data={operations} />; break;
    case "standing-agent-focus": content = <StandingAgentFocus data={operations} actorId={selection.standingAgentId} onSelectionChange={onSelectionChange} />; break;
    case "governance-proposal": content = <GovernanceProposalFocus data={operations} />; break;
    case "approval-focus": content = <ApprovalFocus data={operations} actionEnabled={actionsEnabled && resolved.mode === "store-live"} onDecision={onAction ? (command, capabilityToken) => onAction("/v1/company-os/actions/dispatch", command, { headers: { "X-Harness-Company-OS-Token": capabilityToken } }) : undefined} />; break;
    case "business-module-focus": content = <StructuredDocumentView view={docs.moduleView} actionEnabled={actionsEnabled && resolved.mode === "store-live"} onDocsAction={onAction ? (command, capabilityToken) => onAction("/v1/company-os/actions/dispatch", command, { headers: { "X-Harness-Company-OS-Token": capabilityToken } }) : undefined} />; break;
    case "human-member-focus": content = <HumanMemberFocus data={operations} />; break;
  }

  return <CompanyOsRouteRoot page={page} resolved={resolved}>{content}</CompanyOsRouteRoot>;
}
