import { BriefcaseBusiness, Database, FlaskConical, Network, Plug, Settings2, Wrench } from "lucide-react";
import type { ReactNode } from "react";

import type { SelectionState } from "@/app/selection";
import type { WorkbenchModel } from "@/model/readModel";
import { resolveCompanyOsRouteData } from "./routeMeta";
import type { ResolvedCompanyOsData } from "./sourceTruth";

import {
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
import { CustomPageHost } from "./page-packages/CustomPageHost";
import { AgentTeamOrganization } from "@/surfaces/AgentTeamOrganization";
import { TeamWorks } from "@/surfaces/TeamWorks";

type CompanyOsPage =
  | "home"
  | "docs-workspace"
  | "document-health"
  | "custom-page"
  | "workboard"
  | "team-works"
  | "work-item-focus"
  | "finance"
  | "agents-organization"
  | "agent-team-organization"
  | "standing-agent-focus"
  | "governance-proposal"
  | "approval-focus"
  | "business-module-focus"
  | "human-member-focus";

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
      if (selection.customPageId) return "custom-page";
      if (selection.moduleId) return "business-module-focus";
      return "docs-workspace";
    case "work":
      if (selection.workView === "team-works") return "team-works";
      return selection.workItemId ? "work-item-focus" : "workboard";
    case "finance":
      return "finance";
    case "organization":
      if (selection.proposalId) return "governance-proposal";
      if (selection.standingAgentId) return "standing-agent-focus";
      if (selection.personId) return "human-member-focus";
      if (selection.orgView === "agent-teams") return "agent-team-organization";
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

function StoreLiveLoadingBanner() {
  return (
    <div
      className="flex h-8 shrink-0 items-center gap-2 border-b border-status-good/25 bg-status-good/5 px-4 text-[11px] text-muted-foreground"
      data-company-os-data-mode="store-live-loading"
      role="status"
    >
      <Database className="size-3.5 animate-pulse text-status-good" aria-hidden />
      <span className="font-medium text-foreground">Loading · Store-backed Company OS</span>
      <span className="hidden sm:inline">Waiting for the live Company Store projection; prototype fixture data is suppressed.</span>
    </div>
  );
}

function StoreLiveLoadingPage({ page }: { page: CompanyOsPage }) {
  return (
    <div
      className="flex h-full min-h-0 min-w-0 flex-1 flex-col"
      data-company-os-page={page}
      data-company-os-ready="loading"
      data-company-os-prototype="false"
      data-company-os-data-mode="store-live-loading"
    >
      <StoreLiveLoadingBanner />
      <main className="flex h-full min-h-0 items-center justify-center bg-background p-6">
        <section className="w-full max-w-xl rounded-xl border border-border bg-card p-6 shadow-sm">
          <div className="inline-flex size-10 items-center justify-center rounded-full bg-status-good/10 text-status-good">
            <Database className="size-5 animate-pulse" aria-hidden />
          </div>
          <h1 className="mt-4 text-xl font-semibold tracking-tight">Loading live Company OS Docs</h1>
          <p className="mt-2 text-sm leading-6 text-muted-foreground">
            The dashboard is connecting to the selected Company Store. Static Brand or Trademark fixture data is intentionally hidden so the page cannot be mistaken for current company truth.
          </p>
        </section>
      </main>
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
      <div className="h-full min-h-0 min-w-0 flex-1 overflow-hidden">{children}</div>
    </div>
  );
}

function ExecutionSnapshotRoot({ sourceLabel, children }: { sourceLabel: string; children: ReactNode }) {
  return (
    <div className="flex h-full min-h-0 min-w-0 flex-1 flex-col" data-company-os-data-mode="execution-snapshot">
      <div className="flex h-8 shrink-0 items-center gap-2 border-b border-primary/20 bg-primary/[0.035] px-4 text-[11px] text-muted-foreground" role="status">
        <Database className="size-3.5 text-primary" aria-hidden />
        <span className="font-medium text-foreground">Execution snapshot</span>
        <span className="hidden sm:inline">{sourceLabel} · AgentTeam and Team Work truth remain separate from the Company Store.</span>
      </div>
      <div className="h-full min-h-0 min-w-0 flex-1 overflow-hidden">{children}</div>
    </div>
  );
}

function KernelViewTabs({ surface, selection, onSelectionChange }: { surface: "organization" | "work"; selection: SelectionState; onSelectionChange?: (selection: Partial<SelectionState>) => void }) {
  const organization = surface === "organization";
  const options = organization
    ? [
        { id: "org-units", label: "Company OrgUnits", icon: Database },
        { id: "agent-teams", label: "Agent Teams", icon: Network },
      ]
    : [
        { id: "company-work", label: "Company WorkItems", icon: Database },
        { id: "team-works", label: "Team Works", icon: BriefcaseBusiness },
      ];
  const active = organization ? (selection.orgView ?? "org-units") : (selection.workView ?? "company-work");
  return (
    <nav aria-label={`${organization ? "Organization" : "Work"} data kernel`} className="flex h-11 shrink-0 items-center gap-1 overflow-x-auto border-b border-border bg-card px-3 sm:px-5">
      {options.map((option) => {
        const Icon = option.icon;
        return <button key={option.id} type="button" aria-current={active === option.id ? "page" : undefined} onClick={() => onSelectionChange?.(organization
          ? { surface: "organization", orgView: option.id === "agent-teams" ? "agent-teams" : undefined, orgTeamId: undefined, orgExpanded: undefined, standingAgentId: undefined, personId: undefined, proposalId: undefined }
          : { surface: "work", workView: option.id === "team-works" ? "team-works" : undefined, workItemId: undefined, teamWorkId: undefined, workTeamId: undefined, workHostId: undefined, workMemberId: undefined, workStatus: undefined, workSource: undefined, workDemand: undefined })} className={`inline-flex min-h-9 shrink-0 items-center gap-2 rounded-lg px-3 text-xs font-medium ${active === option.id ? "bg-primary/10 text-primary" : "text-muted-foreground hover:bg-accent hover:text-foreground"}`}><Icon className="size-3.5" />{option.label}</button>;
      })}
    </nav>
  );
}

function KernelViewFrame({ surface, selection, onSelectionChange, children }: { surface: "organization" | "work"; selection: SelectionState; onSelectionChange?: (selection: Partial<SelectionState>) => void; children: ReactNode }) {
  return <div className="flex h-full min-h-0 flex-col"><KernelViewTabs surface={surface} selection={selection} onSelectionChange={onSelectionChange} /><div className="min-h-0 flex-1 overflow-hidden">{children}</div></div>;
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
export function CompanyOsRouter({ model, selection, actionsEnabled = false, livePending = false, snapshotLoading = false, sourceLabel = "snapshot", onAction, onSelectionChange }: { model: WorkbenchModel; selection: SelectionState; actionsEnabled?: boolean; livePending?: boolean; snapshotLoading?: boolean; sourceLabel?: string; onAction?: (path: string, body?: unknown, options?: { headers?: Readonly<Record<string, string>> }) => Promise<boolean>; onSelectionChange?: (selection: Partial<SelectionState>) => void }) {
  if (selection.surface === "providers" || selection.surface === "plugins" || selection.surface === "settings") {
    return <PlatformPlaceholder surface={selection.surface} />;
  }

  const page = selectedPage(selection);
  if (!page) return null;
  if (page === "agent-team-organization") {
    return <ExecutionSnapshotRoot sourceLabel={sourceLabel}><KernelViewFrame surface="organization" selection={selection} onSelectionChange={onSelectionChange}><AgentTeamOrganization snapshot={model.snapshot} selection={selection} loading={snapshotLoading} onSelectionChange={onSelectionChange} /></KernelViewFrame></ExecutionSnapshotRoot>;
  }
  if (page === "team-works") {
    return <ExecutionSnapshotRoot sourceLabel={sourceLabel}><KernelViewFrame surface="work" selection={selection} onSelectionChange={onSelectionChange}><TeamWorks snapshot={model.snapshot} selection={selection} loading={snapshotLoading} onSelectionChange={onSelectionChange} /></KernelViewFrame></ExecutionSnapshotRoot>;
  }
  const resolved = resolveCompanyOsRouteData(model);
  if (livePending && resolved.mode === "prototype-fixture") {
    return <StoreLiveLoadingPage page={page} />;
  }
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
    case "custom-page": content = <CustomPageHost pageId={selection.customPageId} source={resolved.value} />; break;
    case "workboard": content = <WorkOperatingPage source={resolved.value} />; break;
    case "work-item-focus": content = <WorkItemFocus data={operations} actionEnabled={actionsEnabled && resolved.mode === "store-live"} onTransition={onAction ? (command, capabilityToken) => onAction("/v1/company-os/actions/dispatch", command, { headers: { "X-Harness-Company-OS-Token": capabilityToken } }) : undefined} />; break;
    case "finance": content = <FinancePage data={operations} />; break;
    case "agents-organization": content = <OrganizationPage data={operations} onSelectionChange={onSelectionChange} />; break;
    case "standing-agent-focus": content = <StandingAgentFocus data={operations} actorId={selection.standingAgentId} onSelectionChange={onSelectionChange} />; break;
    case "governance-proposal": content = <GovernanceProposalFocus data={operations} />; break;
    case "approval-focus": content = <ApprovalFocus data={operations} actionEnabled={actionsEnabled && resolved.mode === "store-live"} onDecision={onAction ? (command, capabilityToken) => onAction("/v1/company-os/actions/dispatch", command, { headers: { "X-Harness-Company-OS-Token": capabilityToken } }) : undefined} />; break;
    case "business-module-focus": content = <StructuredDocumentView view={docs.moduleView} actionEnabled={actionsEnabled && resolved.mode === "store-live"} onDocsAction={onAction ? (command, capabilityToken) => onAction("/v1/company-os/actions/dispatch", command, { headers: { "X-Harness-Company-OS-Token": capabilityToken } }) : undefined} />; break;
    case "human-member-focus": content = <HumanMemberFocus data={operations} />; break;
  }

  if (page === "agents-organization") content = <KernelViewFrame surface="organization" selection={selection} onSelectionChange={onSelectionChange}>{content}</KernelViewFrame>;
  if (page === "workboard") content = <KernelViewFrame surface="work" selection={selection} onSelectionChange={onSelectionChange}>{content}</KernelViewFrame>;
  return <CompanyOsRouteRoot page={page} resolved={resolved}>{content}</CompanyOsRouteRoot>;
}
