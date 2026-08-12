import { useEffect, useState } from "react";
import {
  Activity,
  BriefcaseBusiness,
  FolderGit2,
  Inbox,
  RadioTower,
  UserRound,
} from "lucide-react";

import { Avatar } from "@/components/workbench/Avatar";
import {
  fetchRoleView,
  type MemberWorkbenchData,
  type RoleActionExecutor,
  type RoleView,
} from "../model/roleViews";
import {
  AttentionStrip,
  ViewProvenance,
  ViewState,
  WorkTable,
} from "./RoleViewPrimitives";
import { RoleActionPanel } from "./RoleActionPanel";

export function MemberWorkbench({
  apiUrl,
  space,
  project,
  memberRunId,
  onAction,
  actionsCurrent,
}: {
  apiUrl: string;
  space: string;
  project: string;
  memberRunId: string;
  onAction: RoleActionExecutor;
  actionsCurrent: boolean;
}) {
  const [view, setView] = useState<RoleView<MemberWorkbenchData> | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [refresh, setRefresh] = useState(0);
  const retry = () => setRefresh((value) => value + 1);
  useEffect(() => {
    let live = true;
    setLoading(true);
    fetchRoleView<MemberWorkbenchData>(
      apiUrl,
      `/v1/views/member-workbench/${encodeURIComponent(memberRunId)}`,
      { space, project },
    )
      .then((value) => {
        if (live) {
          setView(value);
          setError(null);
        }
      })
      .catch((reason) => {
        if (live) setError(String(reason));
      })
      .finally(() => {
        if (live) setLoading(false);
      });
    return () => {
      live = false;
    };
  }, [apiUrl, space, project, memberRunId, refresh]);
  const teamRunId = view?.data.member_run.team_run_id;
  const teamId = view
    ? [...view.data.my_works, ...view.data.eligible_ready_pool].find(
        (work) => work.team_id,
      )?.team_id
    : undefined;
  return (
    <main
      className="agent-team-surface h-full flex-1 overflow-y-auto p-4 sm:p-6"
      data-testid="member-home"
    >
      <div className="mx-auto max-w-[1400px] space-y-6">
        <h1 className="sr-only">Member Workbench</h1>
        <ViewState
          loading={loading}
          error={error}
          identityLabel={`Member home · ${memberRunId}`}
          onRetry={retry}
        >
          {view && (
            <>
              <header className="border-b border-border pb-6">
                <div className="flex flex-wrap justify-between gap-4">
                  <p className="company-editorial-title text-xl">
                    Agent Member Home
                  </p>
                  <ViewProvenance view={view} />
                </div>
                <div className="mt-6 grid items-center gap-6 lg:grid-cols-[10rem_minmax(16rem,.9fr)_minmax(0,1.6fr)]">
                  <div className="flex justify-center lg:justify-start">
                    <div className="member-home-portrait">
                      <Avatar
                        name={view.data.agent_member.id}
                        identity={`${view.data.agent_member.id} ${view.data.agent_member.role}`}
                        size="xl"
                        tone={
                          view.data.member_run.runtime_status === "active"
                            ? "running"
                            : "idle"
                        }
                      />
                    </div>
                  </div>
                  <div className="min-w-0">
                    <div className="agent-team-eyebrow flex items-center gap-2">
                      <UserRound className="size-3.5" />
                      Durable AgentMember
                    </div>
                    <h1 className="company-editorial-title mt-2 break-words text-3xl">
                      {view.data.agent_member.id}
                    </h1>
                    <p className="mt-2 text-sm font-medium">
                      {view.data.agent_member.role}
                    </p>
                    <p className="mt-1 text-xs text-muted-foreground">
                      Organization {view.data.agent_member.organization_status}{" "}
                      · exact-self RoleView
                    </p>
                  </div>
                  <dl className="grid gap-x-6 gap-y-4 border-t border-border pt-4 sm:grid-cols-2 lg:border-l lg:border-t-0 lg:pl-6 lg:pt-0">
                    <HeroFact
                      icon={Activity}
                      label="Current MemberRun"
                      value={view.data.member_run.id}
                      detail={`${view.data.member_run.runtime_status} · generation ${view.data.member_run.runtime_generation}`}
                    />
                    <HeroFact
                      icon={RadioTower}
                      label="Native session"
                      value={view.data.native_session_health}
                      detail={view.data.member_run.coordination_status}
                    />
                    <HeroFact
                      icon={FolderGit2}
                      label="Workspace"
                      value={view.data.workspace_binding?.id ?? "not bound"}
                      detail={`${view.data.runtime_fabric.work_execution_bindings.length} execution binding records`}
                    />
                    <HeroFact
                      icon={BriefcaseBusiness}
                      label="Responsibility"
                      value={`${view.data.my_works.length} Work`}
                      detail={`${view.data.eligible_ready_pool.length} eligible ready`}
                    />
                  </dl>
                </div>
              </header>
              <AttentionStrip view={view} />
              <div className="grid gap-8 xl:grid-cols-[minmax(0,1.25fr)_minmax(22rem,.75fr)]">
                <div className="space-y-7">
                  <section className="border-y border-border py-4">
                    <SectionTitle
                      icon={BriefcaseBusiness}
                      title="My Work"
                      detail="Exact responsibility bound to this AgentMember and current execution context."
                    />
                    <WorkTable items={view.data.my_works} />
                  </section>
                  <section>
                    <SectionTitle
                      icon={BriefcaseBusiness}
                      title="Eligible ready pool"
                      detail="Claimable Work projected by server authority; eligibility is not ownership."
                    />
                    <WorkTable items={view.data.eligible_ready_pool} />
                  </section>
                </div>
                <aside className="space-y-6">
                  <section className="border-y border-border py-4">
                    <SectionTitle
                      icon={Inbox}
                      title="Delivery and review"
                      detail="Separate coordination facts; no transcript mirroring."
                    />
                    <dl className="mt-4 grid grid-cols-2 gap-4">
                      <CompactFact
                        label="Unread messages"
                        value={view.data.unread_messages.length}
                      />
                      <CompactFact
                        label="Queued deliveries"
                        value={view.data.queued_deliveries.length}
                      />
                      <CompactFact
                        label="Gate requirements"
                        value={view.data.gate_requirements.length}
                      />
                      <CompactFact
                        label="Pending interactions"
                        value={view.data.pending_provider_interactions.length}
                      />
                    </dl>
                  </section>
                  <section className="border-y border-border py-4">
                    <SectionTitle
                      icon={Activity}
                      title="Execution evidence"
                      detail="Provider outcomes remain source-linked and distinct."
                    />
                    <dl className="mt-4 grid grid-cols-3 gap-3">
                      <CompactFact
                        label="Reports"
                        value={view.data.report_history.length}
                      />
                      <CompactFact
                        label="Findings"
                        value={view.data.finding_history.length}
                      />
                      <CompactFact
                        label="Failures"
                        value={view.data.failure_history.length}
                      />
                    </dl>
                  </section>
                  <section>
                    <SectionTitle
                      icon={UserRound}
                      title="Authorized actions"
                      detail="Exact server-projected controls for this MemberRun."
                    />
                    {view.allowed_actions.length ? (
                      <div className="mt-3 [&>div]:rounded-none [&>div]:border-x-0 [&>div]:shadow-none">
                        <RoleActionPanel
                          actions={view.allowed_actions}
                          onAction={onAction}
                          actionsCurrent={
                            actionsCurrent && view.freshness === "current"
                          }
                          context={{ teamId, teamRunId }}
                          onCompleted={retry}
                        />
                      </div>
                    ) : (
                      <p className="mt-3 border-l-2 border-border pl-3 text-xs leading-5 text-muted-foreground">
                        No action is authorized for this exact identity and state.
                      </p>
                    )}
                  </section>
                </aside>
              </div>
            </>
          )}
        </ViewState>
      </div>
    </main>
  );
}

function HeroFact({
  icon: Icon,
  label,
  value,
  detail,
}: {
  icon: typeof Activity;
  label: string;
  value: string;
  detail: string;
}) {
  return (
    <div className="min-w-0">
      <dt className="flex items-center gap-1.5 text-[9px] font-semibold uppercase tracking-[.1em] text-muted-foreground">
        <Icon className="size-3.5" />
        {label}
      </dt>
      <dd className="mt-1 break-words text-sm font-semibold">{value}</dd>
      <p className="mt-1 text-[10px] text-muted-foreground">{detail}</p>
    </div>
  );
}
function SectionTitle({
  icon: Icon,
  title,
  detail,
}: {
  icon: typeof Activity;
  title: string;
  detail: string;
}) {
  return (
    <div className="mb-3">
      <div className="flex items-center gap-2">
        <Icon className="size-4 text-primary" />
        <h2 className="company-editorial-title text-lg">{title}</h2>
      </div>
      <p className="mt-1 text-xs text-muted-foreground">{detail}</p>
    </div>
  );
}
function CompactFact({ label, value }: { label: string; value: number }) {
  return (
    <div>
      <dt className="text-[9px] uppercase tracking-[.08em] text-muted-foreground">
        {label}
      </dt>
      <dd className="mt-1 text-xl font-semibold tabular-nums">{value}</dd>
    </div>
  );
}
