import { useEffect, useState, type ReactNode } from "react";
import {
  ArrowLeft,
  ArrowRight,
  Bot,
  CheckCircle2,
  ChevronRight,
  Clock3,
  ExternalLink,
  FileCheck2,
  FileText,
  GitBranch,
  Link2,
  MessageSquare,
  RotateCcw,
  Send,
  Square,
  ShieldAlert,
  ShieldCheck,
  Sparkles,
  Users,
  Wrench,
} from "lucide-react";

import { fetchNativeMemberActivity } from "@/api";

import {
  closeTeamMember,
  interruptTeamMember,
  reopenTeamMember,
  resolvePendingInteraction,
  resumeTeamMember,
  sendTeamMessage,
  steerTeamMember,
  type ActionDescriptor,
} from "@/api/actions";
import { Avatar } from "@/components/workbench/Avatar";
import { TextArea } from "@/components/workbench/OperatorForms";
import type { WorkbenchActivityItem } from "@/components/workbench/activity/ActivityStream";
import { MemberHistoryNarrative } from "@/components/workbench/member/MemberHistoryNarrative";
import { Markdown } from "@/components/workbench/Markdown";
import { ContextModule, ContextRail } from "@/components/workbench/context/ContextRail";
import { TeamRunCompact } from "@/components/workbench/entities/TeamRunControls";
import { FocusShell } from "@/components/workbench/layout/FocusShell";
import { EmptyState, MonoId, StatusDot, type StatusTone } from "@/components/workbench/atoms";
import { memberTone } from "@/components/workbench/tones";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";
import { liveSteerCapability, memberModelLabel, providerStackLine } from "@/lib/provider";
import { selectMemberRunContext, type MemberRunContext, type StableTeamActivity } from "@/model/teamSelectors";
import type { WorkbenchModel } from "@/model/readModel";
import type {
  NativeActivityItem,
  NativeActivityProjection,
  ProviderControlValue,
  TeamMemberCloseRequest,
  TeamMessage,
  Wave,
  Work,
  TeamMessageResponseIntent,
} from "@/types";
import type { SelectionState } from "@/app/selection";

const ACTIONS_DISABLED_HINT = "Connect a live source to message this member";

function claudeDesktopSessionUri(member: MemberRunContext["member"]): string | undefined {
  const session = member.native_session;
  if (
    member.provider !== "claude"
    || session?.provider !== "claude"
    || session.execution_mode !== "claude_agent_sdk"
    || !session.native_session_id
  ) return undefined;
  return `claude://resume?session=${encodeURIComponent(session.native_session_id)}`;
}

export interface MemberRunFocusProps {
  model: WorkbenchModel;
  memberRunId?: string;
  /** Optional Mission/Wave navigation context; it does not own this MemberRun. */
  missionId?: string;
  waveId?: string;
  onSelectionChange: (selection: Partial<SelectionState>) => void;
  /** True only when the dashboard is connected to a writable live source. */
  actionsEnabled?: boolean;
  /** Posts a harness action and refreshes the dashboard snapshot. */
  onAction?: (path: string, body?: unknown) => void;
  /** Live Harness API used for on-demand provider-native activity reads. */
  apiUrl?: string;
  projectBindingId?: string;
  executionSpaceId?: string;
  isLoading?: boolean;
}

/**
 * Focused working surface for one MemberRun in an AgentTeamRun attempt.
 *
 * The stream is deliberately the primary surface: durable messages, explicit
 * actions, and team events share one chronological language. Provider thinking
 * may appear only as a current transient preview; it is never added to the
 * durable activity selector or used as evidence.
 */
export function MemberRunFocus({
  model,
  memberRunId,
  missionId,
  waveId,
  onSelectionChange,
  actionsEnabled = false,
  onAction,
  apiUrl,
  projectBindingId,
  executionSpaceId,
  isLoading = false,
}: MemberRunFocusProps) {
  const [now, setNow] = useState(() => Date.now());
  const [draft, setDraft] = useState("");
  const [composerMode, setComposerMode] = useState<"message" | "steer">("message");
  const [responseIntent, setResponseIntent] = useState<TeamMessageResponseIntent>("response_required");
  // Member Focus is an audit/work surface. Open on the complete native-backed
  // chronology; Key activity is an optional focus lens, never the default
  // substitute for the member's history.
  const [showFullActivity, setShowFullActivity] = useState(true);
  const [nativeActivity, setNativeActivity] = useState<NativeActivityProjection>();
  const [nativeActivityState, setNativeActivityState] = useState<"idle" | "loading" | "ready" | "unavailable">("idle");

  useEffect(() => {
    const timer = window.setInterval(() => setNow(Date.now()), 1_000);
    return () => window.clearInterval(timer);
  }, []);

  const context = selectMemberRunContext(model.snapshot, memberRunId);
  const companyProjection = model.snapshot.company_os as {
    actors?: Array<{
      id?: string;
      display_name?: string;
      record?: { execution_agent_member_ref?: string | null };
    }>;
    standing_assignment_conflicts?: Array<{ agent_member_id?: string }>;
  } | undefined;
  const organizationLinkConflict = Boolean(
    context?.member.agent_member_id
    && companyProjection?.standing_assignment_conflicts?.some(
      (conflict) => conflict.agent_member_id === context.member.agent_member_id,
    ),
  );
  const organizationActor = context?.member.agent_member_id && !organizationLinkConflict
    ? companyProjection?.actors?.find((actor) =>
      actor.record?.execution_agent_member_ref === context.member.agent_member_id)
    : undefined;
  const closeRequest = model.snapshot.team_member_close_requests?.find(
    (request) => request.member_run_id === memberRunId,
  );
  const steerCapability = context ? liveSteerCapability(context.member) : { allowed: false, reason: undefined };
  const canLiveSteer = steerCapability.allowed;

  // Native provider activity is live while the member runs, so poll it every
  // 5s in that window; otherwise one read per session change is enough and the
  // interval is torn down. SSE keeps covering the durable rows in both cases.
  const memberStatus = context?.member.status;
  useEffect(() => {
    setNativeActivity(undefined);
    setNativeActivityState(apiUrl && memberRunId ? "loading" : "idle");
    if (!apiUrl || !memberRunId) return;
    let cancelled = false;
    let inFlight = false;
    const load = () => {
      if (inFlight) return;
      inFlight = true;
      fetchNativeMemberActivity(apiUrl, memberRunId, projectBindingId, executionSpaceId)
        .then((projection) => {
          if (!cancelled) {
            setNativeActivity(projection);
            setNativeActivityState("ready");
          }
        })
        .catch(() => {
          if (!cancelled) setNativeActivityState("unavailable");
        })
        .finally(() => {
          inFlight = false;
        });
    };
    load();
    const timer = memberStatus === "running" ? window.setInterval(load, 5_000) : null;
    return () => {
      cancelled = true;
      if (timer !== null) window.clearInterval(timer);
    };
  }, [
    apiUrl,
    memberRunId,
    projectBindingId,
    executionSpaceId,
    memberStatus,
    context?.member.native_session?.native_session_id,
  ]);

  useEffect(() => {
    if (composerMode === "steer" && !canLiveSteer) {
      setComposerMode("message");
    }
  }, [canLiveSteer, composerMode]);

  if (!context) {
    if (isLoading) {
      return <div className="grid min-h-0 flex-1 place-items-center bg-background text-sm text-muted-foreground">Loading member history…</div>;
    }
    return <MemberRunNotFound memberRunId={memberRunId} onSelectionChange={onSelectionChange} />;
  }

  const finished = isFinishedMember(context.member.status);
  const coordinationStatus = context.member.coordination_status ?? "active";
  const coordinationOpen = coordinationStatus === "active";
  const livePreview = isCurrentPreview(context.liveActivity?.expires_at, now)
    ? context.liveActivity
    : undefined;
  const currentWork = context.currentWork;
  const pendingInteraction = context.interactions.find(
    (interaction) => interaction.member_run_id === context.member.id && interaction.status === "pending",
  );
  const activityItems = toActivityItems(context, livePreview?.preview, nativeActivity?.items);
  const shownActivity = showFullActivity
    ? activityItems
    : projectKeyActivity(activityItems);
  const evidence = collectEvidence(context, model);
  const navigationMission = context.mission ?? model.snapshot.missions?.find((item) => item.id === missionId);
  const navigationWave = context.wave ?? model.snapshot.waves?.find(
    (item) =>
      item.id === waveId &&
      (!navigationMission || item.mission_id === navigationMission.id),
  );
  const stableTeam = model.snapshot.teams?.find((item) => item.id === context.run.agent_team_id);

  const goBackToTeam = () =>
    onSelectionChange({
      surface: "team",
      teamId: context.run.id,
      memberRunId: undefined,
      missionId: navigationMission?.id,
      waveId: navigationWave?.id,
    });

  const dispatchMessage = () => {
    const body = draft.trim();
    if (!body || !actionsEnabled || finished || !coordinationOpen) return;
    if (composerMode === "steer" && !canLiveSteer) return;
    const descriptor = composerMode === "steer"
      ? steerTeamMember(context.run.id, context.member.id, body)
      : sendTeamMessage(context.run.id, {
        fromMemberId: "host",
        senderKind: "operator",
        senderId: "operator",
        senderName: "Operator",
        toMemberIds: [context.member.id],
        kind: "message",
        body,
        workId: currentWork?.id,
        responseIntent,
        originWaveId: navigationWave?.id,
      });
    dispatch(onAction, descriptor);
    setDraft("");
  };

  // The 152px hero and its 44px side padding are a desktop composition: at
  // 390px they left roughly 300px to hold a 130px portrait, the identity block
  // and four controls, so the controls overlapped the title. Both relax below
  // `sm` only; the desktop and tablet composition is unchanged.
  return (
    <FocusShell
      className="member-focus-theme min-h-0 bg-[#fdfcf9]"
      headerClassName="bg-[#fdfcf9] px-4 py-3 sm:h-[152px] sm:px-11 sm:py-0"
      composerClassName="bg-background px-3 py-2 shadow-[0_-12px_30px_-28px_rgba(15,23,42,0.55)] sm:py-2.5"
      responsiveContextVariant="sheet"
      mainLabel="Member work history"
      header={
        <MemberHeroHeader
          context={context}
          closeRequest={closeRequest}
          actionsEnabled={actionsEnabled}
          onAction={onAction}
          onBack={goBackToTeam}
        />
      }
      context={
        <MemberContextRail
          context={context}
          navigationWave={navigationWave}
          navigationMissionId={navigationMission?.id}
          teamName={stableTeam?.name}
          evidence={evidence}
          sessionStatus={context.member.native_session?.availability}
          organizationActor={organizationActor}
          organizationLinkConflict={organizationLinkConflict}
          onSelectionChange={onSelectionChange}
        />
      }
      composer={
        <MemberComposer
          value={draft}
          mode={composerMode}
          responseIntent={responseIntent}
          disabled={!actionsEnabled || finished || !coordinationOpen}
          disabledReason={!coordinationOpen
            ? coordinationStatus === "retired"
              ? "This member is retired; its history is permanently read-only."
              : "This member is closed. Reopen it to resume messaging and its native session."
            : finished
              ? "This member runtime is finished; close and reopen it to resume the same native session."
              : ACTIONS_DISABLED_HINT}
          supportsLiveSteer={canLiveSteer}
          steerUnavailableReason={steerCapability.reason}
          deliveryHint={composerMode === "steer"
            ? "Injects only this explicit Steer into the active Codex turn."
            : `${responseIntent === "response_required" ? "Requests a reply in" : "Adds context to"} the member's next provider round${currentWork ? ` and links Work ${currentWork.id}` : ""}.`}
          onChange={setDraft}
          onModeChange={(mode) => setComposerMode(mode as "message" | "steer")}
          onResponseIntentChange={(intent) => setResponseIntent(intent as TeamMessageResponseIntent)}
          onSend={dispatchMessage}
        />
      }
    >
      <div className="mx-auto flex w-full max-w-[1080px] flex-col px-5 py-2 sm:px-8">
        {pendingInteraction && (
          <section className="mb-2 rounded-xl border border-status-warn/30 bg-status-warn/[0.055] px-3.5 py-3 shadow-[0_12px_30px_-26px_rgba(217,119,6,0.7)]">
            <div className="flex flex-wrap items-start justify-between gap-3">
              <div className="min-w-0 flex-1">
                <div className="flex flex-wrap items-center gap-2">
                  <ShieldAlert className="size-4 text-status-warn" />
                  <p className="text-[12px] font-semibold text-foreground">{pendingInteraction.title}</p>
                  <Badge tone="warn">{pendingInteraction.route} decision</Badge>
                </div>
                <p className="mt-1.5 text-[12px] leading-relaxed text-muted-foreground">{pendingInteraction.prompt}</p>
              </div>
              <div className="flex max-w-sm flex-wrap justify-end gap-1.5">
                {pendingInteraction.options.map((option) => (
                  <Button
                    key={option.id}
                    size="sm"
                    variant={option.intent?.startsWith("reject") ? "secondary" : "default"}
                    disabled={!actionsEnabled || pendingInteraction.route === "policy"}
                    onClick={() => dispatch(onAction, resolvePendingInteraction(
                      context.run.id,
                      pendingInteraction.id,
                      option.id,
                      pendingInteraction.route === "human" ? "operator" : "host",
                    ))}
                  >
                    {option.label}
                  </Button>
                ))}
                {pendingInteraction.route === "policy" && (
                  <span className="self-center text-[10px] text-muted-foreground">Awaiting governed policy decision</span>
                )}
              </div>
            </div>
          </section>
        )}
        <MemberGoalPanel
          context={context}
          snapshotWorks={model.snapshot.works ?? []}
          onOpenWork={(work) => onSelectionChange({ surface: "team", teamId: work.team_run_id, memberRunId: undefined, teamWorkId: work.id })}
        />
        <section className="min-h-[18rem] overflow-hidden bg-background" data-native-activity-state={nativeActivityState}>
          <header className="flex min-h-[58px] flex-wrap items-center justify-between gap-x-3 gap-y-1.5 border-b border-border/70 py-2 sm:h-[58px] sm:flex-nowrap sm:py-0">
            <h2 className="text-[17px] font-semibold tracking-[-0.025em] text-foreground sm:text-[20px]">Work history</h2>
            <div className="flex min-w-0 items-center gap-2">
              <span className="truncate rounded-lg border border-[#e5dfd9] bg-[#fffefa] px-3 py-2 text-[11px] font-medium text-foreground">Complete history · {activityItems.length}</span>
              <button type="button" aria-pressed={!showFullActivity} onClick={() => setShowFullActivity((value) => !value)} className="min-h-11 shrink-0 rounded-lg border border-[#e5dfd9] bg-[#fffefa] px-3 py-2 text-[11px] font-medium text-muted-foreground transition-colors hover:border-[#f08068] hover:text-foreground sm:min-h-0">
                {showFullActivity ? "Focus" : "Return to complete"}
              </button>
            </div>
          </header>
          <MemberHistoryNarrative
            items={shownActivity}
            memberName={context.member.name ?? context.member.id}
            memberRole={context.member.role ?? "Team member"}
            memberStatus={context.member.status}
            empty={
              <EmptyState
                icon={Clock3}
                title="No durable activity yet"
                description="Messages, explicit actions, and observable team events will appear here."
              />
            }
          />
        </section>
      </div>
    </FocusShell>
  );
}

function MemberHeroHeader({
  context,
  closeRequest,
  actionsEnabled,
  onAction,
  onBack,
}: {
  context: MemberRunContext;
  closeRequest?: TeamMemberCloseRequest;
  actionsEnabled: boolean;
  onAction?: MemberRunFocusProps["onAction"];
  onBack: () => void;
}) {
  const name = context.member.name ?? context.member.id;
  const desktopUri = claudeDesktopSessionUri(context.member);
  return (
    <header className="flex h-full min-w-0 flex-col gap-2 sm:flex-row sm:items-center sm:justify-between sm:gap-6">
      <div className="flex min-w-0 items-center gap-3 sm:items-end sm:gap-9 sm:self-stretch">
        {/* Compact identity below `sm`. The sculpted arch portrait is a
            desktop composition and keeps its exact markup at `sm` and up. */}
        <span className="grid size-11 shrink-0 place-items-center sm:hidden">
          <Avatar name={name} identity={context.member.role ?? context.member.id} tone={memberTone(context.member.status)} />
        </span>
        <div className="relative hidden h-full w-[130px] shrink-0 items-end justify-center overflow-hidden sm:flex">
          <span className="absolute inset-x-1 bottom-0 h-[118px] overflow-hidden rounded-t-[64px] border border-b-0 border-[#eadfd7] bg-[linear-gradient(180deg,#fff8f3,#f6ede6)] shadow-[0_22px_44px_-34px_rgba(91,57,36,.7)] [&>span]:size-[116px] [&>span]:rounded-none [&>span]:border-0 [&>span]:ring-0">
            <Avatar name={name} identity={context.member.role ?? context.member.id} tone={memberTone(context.member.status)} size="xl" />
          </span>
        </div>
        <div className="min-w-0 self-center sm:pb-1">
          <h1 className="truncate text-[19px] font-semibold tracking-[-0.035em] text-foreground sm:text-[29px]">{name}</h1>
          <p className="mt-0.5 text-[12px] text-muted-foreground max-sm:truncate sm:mt-1">{context.member.role ?? "Team member"}</p>
          {/* The nowrap/shrink-0 scroll behaviour is mobile-only: applying it at
              every width forced this row to wrap on desktop. */}
          <div className="mt-1.5 flex items-center gap-2 overflow-x-auto text-[11px] max-sm:[&>*]:shrink-0 max-sm:[&>*]:whitespace-nowrap sm:mt-4 sm:flex-wrap sm:gap-3 sm:overflow-visible">
            <span className="inline-flex items-center gap-1.5 font-medium text-status-good"><StatusDot tone={memberStatusTone(context.member.status)} /> {context.member.status ?? "unknown"}</span>
            <span className="h-4 w-px bg-border" />
            <span className="text-muted-foreground">Coordination</span>
            <span className="text-foreground">{context.member.coordination_status ?? "active"} · gen {context.member.runtime_generation ?? 1}</span>
            <span className="h-4 w-px bg-border" />
            <span className="text-muted-foreground">Provider</span>
            <span className="text-foreground">{providerStackLine(context.member.provider, context.member.provider_profile?.execution_mode ?? context.member.native_session?.execution_mode, memberModelLabel(context.member))}</span>
          </div>
        </div>
      </div>
      <div className="flex shrink-0 items-center gap-2 overflow-x-auto pb-0.5 [&>*]:shrink-0 sm:overflow-visible sm:pb-0">
        {closeRequest?.status === "pending" && (
          <Badge tone="warn" title={closeRequest.reason}>Close pending</Badge>
        )}
        {desktopUri && (
          <Button asChild size="sm" variant="outline" className="min-h-11 sm:min-h-0">
            <a
              href={desktopUri}
              title="Import this provider-native session into Claude Desktop. Observe only while Harness drives the member."
            >
              <ExternalLink className="size-3.5" /> Open in Claude Desktop
            </a>
          </Button>
        )}
        {context.member.status === "running" && (
          <Button
            size="sm"
            variant="outline"
            className="min-h-11 sm:min-h-0"
            disabled={!actionsEnabled || !context.member.provider_profile?.supports_cancel}
            title={context.member.provider_profile?.supports_cancel
              ? "Interrupt the active provider turn"
              : interruptUnavailableReason(context.member)}
            onClick={() => dispatch(onAction, interruptTeamMember(context.run.id, context.member.id))}
          >
            <Square className="size-3 fill-current" /> Interrupt
          </Button>
        )}
        {(context.member.coordination_status ?? "active") === "active" && (
          <Button
            size="sm"
            variant="outline"
            className="min-h-11 sm:min-h-0"
            disabled={!actionsEnabled || closeRequest?.status === "pending"}
            onClick={() => dispatch(onAction, closeTeamMember(context.run.id, context.member.id))}
          >
            <Square className="size-3" /> Close
          </Button>
        )}
        {context.member.coordination_status === "closed" && (
          <Button
            size="sm"
            variant="outline"
            className="min-h-11 sm:min-h-0"
            disabled={!actionsEnabled || !["stopped", "completed", "failed"].includes(context.member.status ?? "")}
            title={["stopped", "completed", "failed"].includes(context.member.status ?? "")
              ? context.member.native_session?.supports_resume
                ? "Resume the recorded provider-native session under a fresh Supervisor"
                : "Start a new adapter process and resume this member's existing native session"
              : "Wait until the closing runtime reaches a terminal status"}
            onClick={() => dispatch(
              onAction,
              context.member.native_session?.supports_resume
                ? resumeTeamMember(context.run.id, context.member.id)
                : reopenTeamMember(context.run.id, context.member.id),
            )}
          >
            <RotateCcw className="size-3.5" /> {context.member.native_session?.supports_resume ? "Resume session" : "Reopen"}
          </Button>
        )}
        <Button size="sm" variant="outline" className="min-h-11 sm:min-h-0" onClick={onBack}><ArrowLeft className="size-3.5" /> Back to team</Button>
      </div>
    </header>
  );
}

function interruptUnavailableReason(member: MemberRunContext["member"]): string {
  const profile = member.provider_profile;
  if (!profile) return "Interrupt unavailable: provider capabilities were not captured.";
  const version = profile.provider_version ?? "unknown version";
  return `Interrupt unavailable: ${profile.provider} ${version} in ${profile.execution_mode} does not support provider-native cancellation.`;
}

function MemberRunNotFound({
  memberRunId,
  onSelectionChange,
}: Pick<MemberRunFocusProps, "memberRunId" | "onSelectionChange">) {
  return (
    <div className="mx-auto flex min-h-0 w-full max-w-3xl flex-1 flex-col px-5 py-6">
      <Button
        variant="ghost"
        size="sm"
        className="mb-4 w-fit"
        onClick={() => onSelectionChange({ surface: "team", memberRunId: undefined })}
      >
        <ArrowLeft className="size-3.5" /> Agent teams
      </Button>
      <EmptyState
        icon={Users}
        title="Member run not found"
        description={
          memberRunId
            ? `Member run ${memberRunId} is not available in this project snapshot.`
            : "Choose a member from an Agent Team attempt to open its focus view."
        }
      />
    </div>
  );
}

function Breadcrumb({
  context,
  onSelectionChange,
  onBack,
}: {
  context: MemberRunContext;
  onSelectionChange: MemberRunFocusProps["onSelectionChange"];
  onBack: () => void;
}) {
  return (
    <nav aria-label="Member run path" className="flex min-w-0 items-center gap-1">
      <button type="button" onClick={onBack} className="shrink-0 hover:text-foreground">
        Agent Team
      </button>
      {context.mission && (
        <>
          <ChevronRight className="size-3 shrink-0" />
          <button
            type="button"
            onClick={() => onSelectionChange({ surface: "missions", missionId: context.mission?.id })}
            className="max-w-32 truncate hover:text-foreground"
          >
            {context.mission.title}
          </button>
        </>
      )}
      {context.wave && (
        <>
          <ChevronRight className="size-3 shrink-0" />
          <button
            type="button"
            onClick={() => onSelectionChange({ surface: "missions", missionId: context.wave?.mission_id, waveId: context.wave?.id })}
            className="max-w-32 truncate hover:text-foreground"
          >
            Wave {context.wave.index}
          </button>
        </>
      )}
      <ChevronRight className="size-3 shrink-0" />
      <button type="button" onClick={onBack} className="max-w-32 truncate hover:text-foreground">
        {context.run.objective ?? "Team attempt"}
      </button>
    </nav>
  );
}

function MemberGoalPanel({ context, snapshotWorks, onOpenWork }: { context: MemberRunContext; snapshotWorks: Work[]; onOpenWork: (work: Work) => void }) {
  const work = context.currentWork;
  const queuedOwnedWorks = context.queuedOwnedWorks.filter((candidate) => candidate.id !== work?.id);
  const eligibleReadyWorks = context.eligibleReadyWorks.filter((candidate) => candidate.id !== work?.id);
  const latestSteer = latestSteerSummary(context);
  const nextAction = memberWorkNextAction(context);
  const creatorIds = new Set([context.member.id, context.member.agent_member_id].filter((id): id is string => Boolean(id)));
  const createdWorks = snapshotWorks.filter((candidate) =>
    ["member_run", "agent_member"].includes(candidate.created_by_actor.kind)
    && creatorIds.has(candidate.created_by_actor.id),
  );
  const parentIds = new Set([
    work?.id,
    ...context.queuedOwnedWorks.map((candidate) => candidate.id),
    ...createdWorks.map((candidate) => candidate.id),
  ].filter((id): id is string => Boolean(id)));
  const childWorks = snapshotWorks.filter((candidate) => Boolean(candidate.parent_work_id && parentIds.has(candidate.parent_work_id)));
  return (
    <section aria-label="Current Work (Member Goal)" className="mb-2 rounded-xl border border-primary/20 bg-[linear-gradient(135deg,hsl(var(--primary)/.055),hsl(var(--background))_52%)] px-4 py-3 shadow-[0_14px_34px_-30px_rgba(15,23,42,.55)]">
      {/* The fact grid holds a 15rem minimum, so on a phone the flex-1 summary
          beside it collapsed to roughly 60px and clipped its own text. Below
          `sm` the two stack instead of competing for one row. */}
      <div className="flex flex-col items-stretch gap-3 sm:flex-row sm:flex-wrap sm:items-start sm:justify-between">
        <div className="min-w-0 flex-1" data-goal-summary="true">
          <div className="flex flex-wrap items-center gap-2">
            <ShieldCheck className="size-4 text-primary" />
            <p className="text-[10px] font-semibold uppercase tracking-[0.14em] text-primary">Current Work · Member Goal</p>
            <Badge tone={workStatusTone(work?.status)}>{work?.status ?? "unassigned"}</Badge>
          </div>
          <p className="mt-2 text-sm font-semibold text-foreground">{work?.title ?? "No Work currently owned"}</p>
          <div className="mt-1 line-clamp-3 text-[12px] leading-relaxed text-muted-foreground">
            {work?.context_markdown ? <Markdown source={work.context_markdown} compact /> : "This member has no durable Work ownership yet."}
          </div>
        </div>
        <div className="grid gap-1.5 text-[10px] sm:min-w-[15rem] sm:max-w-[22rem]">
          <GoalFact label="Completion" value={work?.completion_criteria_markdown || "Not declared"} />
          <GoalFact label="Owned paths" value={context.member.owned_paths?.join(", ") || "No path ownership recorded"} mono />
          <GoalFact label="Latest steer" value={latestSteer ?? "No durable steer recorded"} />
          <GoalFact label="Work ID" value={work?.id ?? "Not assigned"} mono />
        </div>
      </div>
      <div className="mt-3 grid gap-2 border-t border-border/60 pt-3 sm:grid-cols-2">
        <MemberWorkQueue
          label="Owned queue"
          description="Assigned to this member and ready after the current Work."
          works={queuedOwnedWorks}
          empty="No additional owned Work is queued."
          onOpen={onOpenWork}
        />
        <MemberWorkQueue
          label="Eligible ready pool"
          description="Unowned team Work this member may claim from its own runtime."
          works={eligibleReadyWorks}
          empty="No unowned ready Work is eligible for this member."
          onOpen={onOpenWork}
        />
      </div>
      <div className="mt-2 grid gap-2 border-t border-border/60 pt-3 sm:grid-cols-2" data-member-work-lineage="true">
        <MemberWorkQueue
          label="Created Work"
          description="Rows whose created_by_actor explicitly identifies this MemberRun or AgentMember."
          works={createdWorks}
          empty="No created Work"
          onOpen={onOpenWork}
        />
        <MemberWorkQueue
          label="Child Work"
          description="Direct children linked through parent_work_id from this member's Work."
          works={childWorks}
          empty="No child Work"
          onOpen={onOpenWork}
        />
      </div>
      <div className="mt-2 flex items-start gap-2 rounded-lg border border-primary/15 bg-primary/[0.035] px-3 py-2 text-[10px] leading-relaxed text-muted-foreground" aria-label="Member next Work action">
        <ArrowRight className="mt-0.5 size-3 shrink-0 text-primary" />
        <p><span className="font-semibold text-foreground">Next:</span> {nextAction}</p>
      </div>
    </section>
  );
}

function memberWorkNextAction(context: MemberRunContext): string {
  const work = context.currentWork;
  if (work?.status === "open") {
    return "Start this owned Work from the member's native runtime. This operator view does not impersonate the member.";
  }
  if (work?.status === "in_progress") {
    return "Continue the current Work in the provider-native session, then submit its result and evidence for Host review.";
  }
  if (work?.status === "blocked") {
    return "Wait for the Host to resolve and resume this Work; keep the blocker conversation linked to this Work.";
  }
  if (work?.status === "review") {
    return "Host review is pending. Changes requested return this same Work and ownership to the member.";
  }
  if (context.eligibleReadyWorks.length > 0) {
    return "No Work is currently owned. The member may claim an eligible ready Work from its own runtime.";
  }
  return "No owned or eligible Work is ready. The member remains available for Host assignment or team coordination.";
}

function MemberWorkQueue({
  label,
  description,
  works,
  empty,
  onOpen,
}: {
  label: string;
  description: string;
  works: Work[];
  empty: string;
  onOpen?: (work: Work) => void;
}) {
  return (
    <div className="rounded-lg border border-border/60 bg-background/75 px-3 py-2.5">
      <div className="flex items-center justify-between gap-2">
        <p className="text-[10px] font-semibold uppercase tracking-[0.12em] text-foreground">{label}</p>
        <Badge tone={works.length ? "info" : "muted"}>{works.length}</Badge>
      </div>
      <p className="mt-0.5 text-[10px] leading-relaxed text-muted-foreground">{description}</p>
      {works.length ? (
        <ul className="mt-2 space-y-1.5">
          {works.slice(0, 3).map((work) => (
            <li key={work.id} className="rounded-md border border-border/50 bg-card">
              <button type="button" disabled={!onOpen} onClick={() => onOpen?.(work)} className="flex w-full min-w-0 items-start justify-between gap-2 px-2 py-1.5 text-left disabled:cursor-default">
              <div className="min-w-0">
                <p className="truncate text-[11px] font-medium text-foreground" title={work.title}>{work.title}</p>
                <p className="mt-0.5 truncate font-mono text-[9px] text-muted-foreground" title={work.id}>{work.id}</p>
              </div>
              <Badge tone={workStatusTone(work.status)}>{work.status}</Badge>
              </button>
            </li>
          ))}
          {works.length > 3 && <li className="text-[10px] text-muted-foreground">+{works.length - 3} more on the Team Works board</li>}
        </ul>
      ) : <p className="mt-2 text-[10px] text-muted-foreground">{empty}</p>}
    </div>
  );
}

function GoalFact({ label, value, mono = false }: { label: string; value: string; mono?: boolean }) {
  return (
    <div className="grid grid-cols-[4.5rem_minmax(0,1fr)] gap-2 rounded-md border border-border/60 bg-background/80 px-2 py-1.5">
      <span className="font-semibold uppercase tracking-wider text-muted-foreground">{label}</span>
      <span className={cn("line-clamp-2 text-foreground", mono && "font-mono text-[9px]")}>{value}</span>
    </div>
  );
}

function MemberContextRail({
  context,
  navigationWave,
  navigationMissionId,
  teamName,
  evidence,
  sessionStatus,
  organizationActor,
  organizationLinkConflict,
  onSelectionChange,
}: {
  context: MemberRunContext;
  navigationWave?: Wave;
  navigationMissionId?: string;
  teamName?: string;
  evidence: EvidenceItem[];
  sessionStatus?: string;
  organizationActor?: { id?: string; display_name?: string };
  organizationLinkConflict: boolean;
  onSelectionChange: MemberRunFocusProps["onSelectionChange"];
}) {
  const work = context.currentWork;
  const activeMembers = context.members.filter((member) => member.status === "running").length;
  const gateTone = waveGateTone(navigationWave?.gate_status);
  const peerMembers = context.members.filter((member) => member.id !== context.member.id);
  const hostThread = context.messagesForMember.filter((message) =>
    message.from_member_id === "host" || (message.to_member_ids ?? []).includes("host"),
  );
  const peerThread = context.messagesForMember.filter((message) =>
    message.from_member_id !== "host"
    && !(message.to_member_ids ?? []).includes("host")
  );
  const latestSteer = latestSteerSummary(context);

  return (
    <ContextRail label="Member context" hideHeader className="bg-[#fbfaf7]" contentClassName="flex flex-col gap-4 space-y-0 p-5">
      <ContextModule
        title={navigationWave ? `Wave ${navigationWave.index} · ${navigationWave.title}` : "No Host-plan Wave selected"}
        icon={<GitBranch className="size-3.5" />}
        tone={gateTone}
        className="order-2 rounded-xl bg-card shadow-[0_14px_34px_-32px_rgba(15,23,42,.65)]"
        action={
          navigationWave ? (
            <RailOpenButton
              label="Open wave"
              onClick={() => onSelectionChange({ surface: "missions", missionId: navigationWave.mission_id, waveId: navigationWave.id })}
            />
          ) : undefined
        }
      >
        {navigationWave ? (
          <div className="space-y-2 text-[12px]">
            <p className="line-clamp-3 leading-relaxed text-foreground">{navigationWave.objective}</p>
            <RailKeyValue label="Revision" value={String(navigationWave.revision ?? 1)} />
            <div className="flex flex-wrap gap-1.5 pt-0.5">
              <Badge tone={gateTone}>decision {navigationWave.gate_status ?? "pending"}</Badge>
              <Badge tone="muted">{context.wave ? "legacy direct executor" : "navigation context"}</Badge>
            </div>
            {!context.wave && <p className="text-[11px] leading-relaxed text-muted-foreground">This MemberRun continues independently; the shared Works board records durable ownership while the Wave remains Host planning context.</p>}
          </div>
        ) : (
          <RailEmpty>Open this member from a Mission to retain the current Host-plan context.</RailEmpty>
        )}
      </ContextModule>

      <ContextModule
        title={teamName ?? "Agent Team"}
        icon={<Users className="size-3.5" />}
        tone={teamStatusTone(context.run.status)}
        className="order-1 rounded-xl bg-card shadow-[0_14px_34px_-32px_rgba(15,23,42,.65)]"
        action={<RailOpenButton label="Open team" onClick={() => onSelectionChange({
          surface: "team",
          teamId: context.run.id,
          memberRunId: undefined,
          missionId: navigationMissionId,
          waveId: navigationWave?.id,
        })} />}
      >
        <TeamRunCompact
          run={context.run}
          members={context.members}
          needsYouCount={context.needsYou.total}
          onOpen={() => onSelectionChange({
            surface: "team",
            teamId: context.run.id,
            memberRunId: undefined,
            missionId: navigationMissionId,
            waveId: navigationWave?.id,
          })}
        />
        <p className="mt-2 text-[11px] text-muted-foreground">
          {activeMembers} active · {context.needsYou.total ? `${context.needsYou.total} needs attention` : "no open signals"}
        </p>
      </ContextModule>

      <ContextModule
        title="Organization identity"
        icon={organizationLinkConflict
          ? <ShieldAlert className="size-3.5" />
          : <Users className="size-3.5" />}
        tone={organizationLinkConflict ? "bad" : organizationActor ? "info" : undefined}
        className="order-1 rounded-xl bg-card shadow-[0_14px_34px_-32px_rgba(15,23,42,.65)]"
        action={organizationActor?.id ? (
          <RailOpenButton
            label="Open profile"
            onClick={() => onSelectionChange({
              surface: "organization",
              standingAgentId: organizationActor.id,
            })}
          />
        ) : undefined}
      >
        {organizationLinkConflict ? (
          <div role="alert" className="space-y-2 text-[12px]">
            <p className="font-medium text-status-bad">Ambiguous Standing Agent link</p>
            <p className="text-[11px] leading-5 text-muted-foreground">
              More than one Standing Agent claims this AgentMember. Harness withholds the Organization identity instead of guessing a profile.
            </p>
          </div>
        ) : organizationActor ? (
          <div className="space-y-2 text-[12px]">
            <RailKeyValue label="Standing Agent" value={organizationActor.display_name ?? organizationActor.id ?? "Linked actor"} />
            <RailKeyValue label="AgentMember" value={context.member.agent_member_id ?? "Not linked"} mono />
            <p className="text-[11px] text-muted-foreground">Explicit Company-owned execution identity link.</p>
          </div>
        ) : (
          <RailEmpty>Ad-hoc execution. No Standing Agent explicitly links this AgentMember.</RailEmpty>
        )}
      </ContextModule>

      <ContextModule
        title="Current Work · Member Goal"
        icon={<ShieldCheck className="size-3.5" />}
        tone={work ? workStatusTone(work.status) : "warn"}
        className="order-2 rounded-xl bg-card shadow-[0_14px_34px_-32px_rgba(15,23,42,.65)]"
      >
        {work ? (
          <div className="space-y-2.5 text-[12px]">
            <p className="font-semibold text-foreground">{work.title}</p>
            {work.context_markdown && <div className="line-clamp-5 text-muted-foreground"><Markdown source={work.context_markdown} compact /></div>}
            <RailKeyValue label="Status" value={work.status} />
            <RailKeyValue label="Priority" value={work.priority} />
            <RailKeyValue label="Updated" value={formatTime(work.updated_at)} />
            <RailKeyValue label="Version" value={String(work.version)} mono />
            <RailKeyValue label="Completion" value={work.completion_criteria_markdown || "Not declared"} />
            <RailKeyValue label="Latest steer" value={latestSteer ?? "No durable steer recorded"} />
            <RailReferenceList label="Artifacts" refs={work.artifact_refs} empty="No Work artifacts attached." />
            <RailReferenceList label="Checks" refs={work.check_refs} empty="No Work checks attached." />
            <div>
              <p className="mb-1 text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">Owned paths</p>
              {context.member.owned_paths?.length ? (
                <div className="flex flex-wrap gap-1">
                  {context.member.owned_paths.map((path) => <Badge key={path} tone="muted">{path}</Badge>)}
                </div>
              ) : <p className="text-muted-foreground">No path ownership recorded.</p>}
            </div>
            <RailKeyValue label="Permissions" value="Not reported by this member run" />
          </div>
        ) : (
          <RailEmpty>No Work is owned by this member. Messages and observed activity do not prove responsibility.</RailEmpty>
        )}
        <div className="mt-2 grid grid-cols-2 gap-2 border-t border-border/60 pt-2">
          <RailKeyValue label="Owned queued" value={String(context.queuedOwnedWorks.length)} />
          <RailKeyValue label="Eligible ready" value={String(context.eligibleReadyWorks.length)} />
        </div>
      </ContextModule>

      <ContextModule
        title="Host & peer threads"
        icon={<MessageSquare className="size-3.5" />}
        tone={hostThread.length || peerThread.length ? "decision" : "idle"}
        collapsible
        defaultOpen
        className="order-4 rounded-xl bg-card shadow-[0_14px_34px_-32px_rgba(15,23,42,.65)]"
      >
        <MessageThreadGroup label="Host" messages={hostThread} context={context} />
        <MessageThreadGroup label="Peers" messages={peerThread} context={context} className="mt-3" />
      </ContextModule>

      <ContextModule
        title="Team peers"
        icon={<Users className="size-3.5" />}
        tone={peerMembers.some((member) => member.status === "running") ? "running" : "idle"}
        className="order-5 rounded-xl bg-card shadow-[0_14px_34px_-32px_rgba(15,23,42,.65)]"
      >
        {peerMembers.length ? (
          <div className="space-y-1">
            {peerMembers.map((peer) => (
              <button
                key={peer.id}
                type="button"
                onClick={() => onSelectionChange({
                  surface: "team",
                  teamId: context.run.id,
                  memberRunId: peer.id,
                  missionId: navigationMissionId,
                  waveId: navigationWave?.id,
                })}
                className="flex w-full items-center justify-between gap-2 rounded-md px-2 py-1.5 text-left hover:bg-accent"
              >
                <span className="min-w-0 truncate text-[11px] font-medium text-foreground">{peer.name ?? peer.id}</span>
                <Badge tone={memberStatusTone(peer.status)}>{peer.status ?? "unknown"}</Badge>
              </button>
            ))}
          </div>
        ) : <RailEmpty>No peer MemberRuns in this TeamRun.</RailEmpty>}
      </ContextModule>

      <ContextModule title="Artifacts & evidence" icon={<FileCheck2 className="size-3.5" />} tone={evidence.length ? "good" : "idle"} className="order-7 rounded-xl bg-card shadow-[0_14px_34px_-32px_rgba(15,23,42,.65)]">
        {evidence.length ? (
          <ul className="space-y-2">
            {evidence.slice(0, 6).map((item) => (
              <li key={item.id} className="rounded-md border border-border/70 bg-card px-2.5 py-2">
                <div className="flex min-w-0 items-center gap-2">
                  <FileText className="size-3.5 shrink-0 text-muted-foreground" />
                  <span className="min-w-0 flex-1 truncate text-[12px] font-medium text-foreground">{item.label}</span>
                </div>
                <p className="mt-1 text-[10px] text-muted-foreground">{item.source}</p>
              </li>
            ))}
          </ul>
        ) : (
          <RailEmpty>No output or evidence references are linked to this member yet.</RailEmpty>
        )}
      </ContextModule>

      <ContextModule title="Runtime" icon={<Wrench className="size-3.5" />} tone={memberStatusTone(context.member.status)} className="order-6 rounded-xl bg-card shadow-[0_14px_34px_-32px_rgba(15,23,42,.65)]">
        <div className="space-y-1.5 text-[12px]">
          <RailKeyValue label="Provider" value={context.member.provider ?? "Not recorded"} />
          <RailKeyValue label="Execution mode" value={context.member.provider_profile?.execution_mode ?? "Not recorded"} />
          <RailKeyValue label="Ordinary mail" value={context.member.provider_profile?.ordinary_message_boundary ?? "unknown"} />
          <RailKeyValue label="Compatibility" value={context.member.provider_profile?.compatibility_status ?? "unknown"} />
          <RailKeyValue label="Model control" value={providerControlSummary(context.member.provider_controls?.model, context.member.model)} />
          <RailKeyValue label="Reasoning control" value={providerControlSummary(context.member.provider_controls?.reasoning_effort)} />
          <RailKeyValue label="Service control" value={providerControlSummary(context.member.provider_controls?.service_tier)} />
          <RailKeyValue label="Native session" value={context.member.native_session?.native_session_id ?? "Unavailable"} mono />
          <RailKeyValue label="Resume" value={context.member.native_session?.supports_resume ? "Supported" : "Not verified"} />
          <RailKeyValue label="Actual cwd" value={context.member.workspace_snapshot?.cwd ?? "Not captured (legacy run)"} mono title="Runs started before workspace capture was introduced did not record their cwd. Reopen the member to capture it." />
          <RailKeyValue label="Git branch" value={context.member.workspace_snapshot?.git_branch ?? "Detached or not captured"} mono />
          <RailKeyValue label="Last activity" value={formatRelative(context.member.last_event_at)} />
          {claudeDesktopSessionUri(context.member) && (
            <p className="rounded-md border border-status-warn/25 bg-status-warn/5 px-2.5 py-2 text-[10px] leading-4 text-muted-foreground">
              Desktop import is an observation surface while Harness drives this Member. Simultaneous SDK and Desktop generation is not verified.
            </p>
          )}
          <details className="pt-1 text-[10px] text-muted-foreground">
            <summary className="cursor-pointer font-medium hover:text-foreground">Advanced runtime facts</summary>
            <div className="mt-2 space-y-1.5 border-l border-border pl-2.5 text-[11px]">
              <RailKeyValue label="Provider version" value={context.member.provider_profile?.provider_version ?? "Not reported"} />
              <RailKeyValue label="Adapter contract" value={context.member.provider_profile?.adapter_contract_version ?? "Not recorded"} />
              {context.member.provider_controls?.model.note && <RailKeyValue label="Model receipt" value={context.member.provider_controls.model.note} />}
              {context.member.provider_controls?.reasoning_effort.note && <RailKeyValue label="Reasoning receipt" value={context.member.provider_controls.reasoning_effort.note} />}
              {context.member.provider_controls?.service_tier.note && <RailKeyValue label="Service receipt" value={context.member.provider_controls.service_tier.note} />}
              <RailKeyValue label="Session status" value={sessionStatus ?? "Not reported"} />
              <RailKeyValue label="Execution root" value={context.run.execution_root ?? "Not recorded"} mono />
              <RailKeyValue label="Worktree" value={context.member.worktree_ref ?? "None"} mono />
              <RailKeyValue label="Git HEAD" value={context.member.workspace_snapshot?.git_head ?? "Not captured"} mono />
              <RailKeyValue label="Instruction roots" value={formatWorkspaceRoots(context.member.workspace_snapshot?.instruction_roots)} mono />
              <RailKeyValue label="Skill roots" value={formatWorkspaceRoots(context.member.workspace_snapshot?.skill_roots)} mono />
            </div>
          </details>
        </div>
      </ContextModule>

      <ContextModule title="Native subagent activity" icon={<Bot className="size-3.5" />} tone={context.delegationsForMember.length ? "decision" : "idle"} collapsible defaultOpen={context.delegationsForMember.length > 0} className="order-8 rounded-xl bg-card">
        {context.delegationsForMember.length ? (
          <ul className="space-y-2">
            {context.delegationsForMember.map((delegation) => (
              <li key={delegation.id} className="rounded-md border border-border/70 bg-card px-2.5 py-2 text-[12px]">
                <div className="flex flex-wrap gap-1.5"><Badge tone={delegation.mode === "provider_native" ? "info" : "decision"}>{delegation.mode === "provider_native" ? "observed" : "orchestrated"}</Badge><Badge tone={teamStatusTone(delegation.status)}>{delegation.status ?? "unknown"}</Badge></div>
                <p className="mt-1.5 text-foreground">{delegation.objective ?? "Delegated work"}</p>
                <p className="mt-1 text-[10px] text-muted-foreground">{delegation.mode === "provider_native" ? "Provider-native: lifecycle is not controlled by the harness." : "Harness-observed delegation."}</p>
              </li>
            ))}
          </ul>
        ) : <RailEmpty>No observed native subagent activity. Subagents remain this Member's internal implementation detail.</RailEmpty>}
      </ContextModule>
    </ContextRail>
  );
}

function MessageThreadGroup({
  label,
  messages,
  context,
  className,
}: {
  label: string;
  messages: TeamMessage[];
  context: MemberRunContext;
  className?: string;
}) {
  const recent = messages.slice(-3).reverse();
  return (
    <div className={className}>
      <div className="mb-1 flex items-center justify-between gap-2 text-[9px] font-semibold uppercase tracking-wider text-muted-foreground">
        <span>{label}</span><span>{messages.length}</span>
      </div>
      {recent.length ? (
        <div className="space-y-1.5">
          {recent.map((message) => {
            const outgoing = message.from_member_id === context.member.id;
            const counterpart = outgoing
              ? (message.to_member_ids ?? []).map((id) => memberName(context, id)).join(", ")
              : memberName(context, message.from_member_id);
            return (
              <div key={message.id} className="rounded-md border border-border/60 bg-background/70 px-2 py-1.5">
                <div className="flex min-w-0 items-center gap-1.5 text-[9px] text-muted-foreground">
                  <Badge tone={messageTone(message.kind)}>{message.kind ?? "message"}</Badge>
                  {message.response_intent && <Badge tone="muted">{message.response_intent === "response_required" ? "reply" : "info"}</Badge>}
                  {message.work_id && <Badge tone="info" title={message.work_id}><ShieldCheck className="size-2.5" /> Work</Badge>}
                  <span className="min-w-0 flex-1 truncate">{outgoing ? `to ${counterpart}` : `from ${counterpart}`}</span>
                  <span>{messageDeliverySummary(message, context.member.id)}</span>
                </div>
                <p className="mt-1 line-clamp-2 text-[10px] leading-relaxed text-foreground">{message.body || "No body"}</p>
              </div>
            );
          })}
        </div>
      ) : <p className="text-[10px] text-muted-foreground">No messages in this thread.</p>}
    </div>
  );
}

function memberName(context: MemberRunContext, id?: string): string {
  if (!id || id === "host") return "Host";
  return context.memberById.get(id)?.name ?? id;
}

function messageDeliverySummary(message: TeamMessage, memberId: string): string {
  const relevant = message.from_member_id === memberId
    ? message.deliveries ?? []
    : (message.deliveries ?? []).filter((delivery) => delivery.member_id === memberId);
  const statuses = [...new Set(relevant.map((delivery) => delivery.status).filter(Boolean))];
  return statuses.length ? statuses.join("/") : "recorded";
}

function latestSteerSummary(context: MemberRunContext): string | undefined {
  const control = [...context.messagesForMember]
    .reverse()
    .find((message) =>
      message.kind === "control"
      && (message.to_member_ids ?? []).includes(context.member.id)
      && /steer/i.test(message.body ?? ""),
    );
  if (control?.body) return control.body;
  const action = [...context.actionsForMember]
    .reverse()
    .find((candidate) => /steer/i.test(`${candidate.action_type ?? ""} ${candidate.title ?? ""}`));
  if (action) return action.summary ?? action.title;
  const event = [...context.eventsForMember]
    .reverse()
    .find((candidate) => /steer/i.test(`${candidate.operation ?? ""} ${candidate.summary ?? ""}`));
  return event?.summary;
}

function formatWorkspaceRoots(roots?: string[]): string {
  return roots?.length ? roots.join(" · ") : "None discovered or not captured";
}

function MemberComposer({
  value,
  mode,
  responseIntent,
  disabled,
  disabledReason,
  deliveryHint,
  supportsLiveSteer,
  steerUnavailableReason,
  onChange,
  onModeChange,
  onResponseIntentChange,
  onSend,
}: {
  value: string;
  mode: "message" | "steer";
  responseIntent: TeamMessageResponseIntent;
  disabled: boolean;
  disabledReason: string;
  deliveryHint: string;
  supportsLiveSteer: boolean;
  steerUnavailableReason?: string;
  onChange: (value: string) => void;
  onModeChange: (value: string) => void;
  onResponseIntentChange: (value: string) => void;
  onSend: () => void;
}) {
  if (disabled) {
    return (
      <div className="mx-auto flex min-h-11 w-full max-w-4xl items-center gap-3 rounded-xl border border-border/70 bg-muted/20 px-3.5 py-2 text-[11px] text-muted-foreground sm:h-10 sm:py-0">
        <MessageSquare className="size-3.5 shrink-0" />
        <span className="min-w-0 flex-1 truncate">{disabledReason}</span>
        <Badge tone="muted">read only</Badge>
      </div>
    );
  }
  return (
    <form
      className="mx-auto flex w-full max-w-4xl flex-col items-stretch gap-2 sm:flex-row sm:items-end"
      onSubmit={(event) => {
        event.preventDefault();
        onSend();
      }}
    >
      <div className="min-w-0 flex-1">
        <label htmlFor="member-run-message" className="sr-only">Message this member</label>
        <TextArea
          id="member-run-message"
          value={value}
          disabled={disabled}
          onChange={(event) => onChange(event.target.value)}
          placeholder={disabled ? disabledReason : "Message this member…"}
          className="min-h-12 resize-none"
          onKeyDown={(event) => {
            if ((event.metaKey || event.ctrlKey) && event.key === "Enter") {
              event.preventDefault();
              onSend();
            }
          }}
        />
        <p className="mt-1 text-[10px] text-muted-foreground">{disabled ? disabledReason : `${deliveryHint} ⌘/Ctrl + Enter to send.`}</p>
      </div>
      <div className="flex items-end gap-2 sm:contents">
      <select
        aria-label="Delivery mode"
        value={mode}
        disabled={disabled}
        onChange={(event) => onModeChange(event.target.value)}
        className="h-11 max-w-28 flex-1 rounded-md border border-border bg-background px-2 text-[11px] sm:h-8 sm:flex-none text-foreground outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:opacity-50"
      >
        <option value="message">Message</option>
        <option value="steer" disabled={!supportsLiveSteer}>
          {supportsLiveSteer ? "Steer" : "Steer unavailable"}
        </option>
      </select>
      {mode === "message" && (
        <select
          aria-label="Response intent"
          value={responseIntent}
          disabled={disabled}
          onChange={(event) => onResponseIntentChange(event.target.value)}
          className="h-11 max-w-32 flex-1 rounded-md border border-border bg-background px-2 text-[11px] sm:h-8 sm:flex-none text-foreground outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:opacity-50"
        >
          <option value="response_required">Needs reply</option>
          <option value="informational">Informational</option>
        </select>
      )}
      {!supportsLiveSteer && steerUnavailableReason && (
        <span className="sr-only" aria-live="polite">{steerUnavailableReason}</span>
      )}
      <Button type="submit" size="icon" className="size-11 sm:size-8" disabled={disabled || !value.trim()} aria-label="Send message">
        <Send className="size-3.5" />
      </Button>
      </div>
    </form>
  );
}

function RailOpenButton({ label, onClick }: { label: string; onClick: () => void }) {
  return <button type="button" onClick={onClick} className="text-[10px] font-medium text-primary hover:underline">{label}</button>;
}

function RailEmpty({ children }: { children: string }) {
  return <p className="text-[12px] leading-relaxed text-muted-foreground">{children}</p>;
}

function RailKeyValue({ label, value, mono = false, title }: { label: string; value: string; mono?: boolean; title?: string }) {
  return <div className="flex min-w-0 items-start justify-between gap-3" title={title}><span className="shrink-0 text-muted-foreground">{label}</span><span className={cn("min-w-0 text-right text-foreground", mono && "truncate font-mono text-[11px]")}>{value}</span></div>;
}

function RailReferenceList({ label, refs, empty }: { label: string; refs?: string[]; empty: string }) {
  return (
    <div>
      <p className="mb-1 text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">{label}</p>
      {refs?.length ? (
        <div className="flex flex-wrap gap-1">
          {refs.map((ref) => <Badge key={ref} tone="muted" title={ref}>{ref}</Badge>)}
        </div>
      ) : <p className="text-[11px] text-muted-foreground">{empty}</p>}
    </div>
  );
}

function providerControlSummary(control?: ProviderControlValue | null, legacyRequested?: string | null): string {
  const requested = control?.requested ?? legacyRequested;
  const effective = control?.effective;
  const status = control?.status ?? (legacyRequested ? "legacy_unverified" : "not_recorded");
  return `${requested ? `requested ${requested}` : "provider default"} → ${effective ?? "not confirmed"} · ${status}`;
}

function toActivityItems(
  context: MemberRunContext,
  transientPreview?: string,
  nativeItems: NativeActivityItem[] = [],
): WorkbenchActivityItem[] {
  const durable = context.activityForMember.map((item) => toActivityItem(item, context));
  const native = nativeItems.map((item, index): WorkbenchActivityItem => ({
    id: `native:${context.member.id}:${index}:${item.occurred_at ?? ""}`,
    kind: item.kind === "message" ? "message" : "action",
    glyph: item.kind === "tool" ? nativeToolGlyph(item.title) : "message",
    title: item.title,
    actor: context.member.name ?? context.member.id,
    timestamp: formatTime(item.occurred_at),
    occurredAt: item.occurred_at,
    tone: item.status === "failed" ? "bad" : item.status === "started" ? "running" : "good",
    prominence: "detail",
    source: "provider-native",
    rawText: item.summary ?? item.title,
    actorLabel: context.member.name ?? context.member.id,
    statusLabel: item.status,
    body: item.kind === "tool" ? nativeToolDetails(item.summary) : readableHistoryBody(item.summary),
  }));
  const joined = [...native, ...durable].sort(compareActivityChronology);
  if (!transientPreview) return joined;
  return [
    {
      id: `live:${context.member.id}`,
      kind: "thinking",
      title: "Current provider preview",
      body: transientPreview,
      actor: context.member.name ?? context.member.id,
      timestamp: "now",
      occurredAt: new Date().toISOString(),
      transient: true,
      source: "live",
      rawText: transientPreview,
      actorLabel: context.member.name ?? context.member.id,
      statusLabel: "live",
      tone: "decision",
      glyph: "runtime",
      prominence: "primary",
    },
    ...joined,
  ];
}

/** Build the optional focus lens without rewriting the complete chronology. */
function projectKeyActivity(items: WorkbenchActivityItem[]): WorkbenchActivityItem[] {
  const visible = items.filter((item) => item.prominence !== "detail");
  const native = items.filter((item) => item.source === "provider-native");
  if (native.length === 0 && visible.length <= 6) return visible;

  const selected = new Set<string>();
  const select = (item: WorkbenchActivityItem | undefined) => item && selected.add(item.id);
  visible.filter((item) => item.transient || item.prominence === "pressure").forEach(select);
  select(visible.find((item) => item.kind === "message"));
  // The compact narrative must prove that the bound provider session is
  // actually visible. Keep its opening response, latest runtime/tool action,
  // and latest message while Full record exposes every native row.
  select(native.find((item) => item.kind === "message"));
  select(findLastItem(native, (item) =>
    item.glyph === "runtime" && typeof item.title === "string" && item.title !== "tool result",
  ));
  select(findLastItem(native, (item) => item.kind === "message"));
  select(findLastItem(visible, (item) => item.kind === "evidence"));
  select(findLastItem(visible, (item) => item.glyph === "handoff"));
  for (let index = visible.length - 1; index >= 0; index -= 1) {
    if (selected.size >= 8) break;
    select(visible[index]);
  }
  return items.filter((item) => selected.has(item.id));
}

function findLastItem(
  items: WorkbenchActivityItem[],
  predicate: (item: WorkbenchActivityItem) => boolean,
): WorkbenchActivityItem | undefined {
  for (let index = items.length - 1; index >= 0; index -= 1) {
    if (predicate(items[index])) return items[index];
  }
  return undefined;
}

function toActivityItem(item: StableTeamActivity, context: MemberRunContext): WorkbenchActivityItem {
  if (item.kind === "message") {
    const message = item.message;
    const label = message.from_member_id === "host" ? "Host" : context.memberById.get(message.from_member_id ?? "")?.name ?? message.from_member_id ?? "Unknown sender";
    const needsAttention = message.response_intent === "response_required";
    return {
      id: item.id,
      kind: "message",
      glyph: message.kind === "handoff" ? "handoff" : "message",
      title: teamMessageTitle(message.kind),
      body: readableHistoryBody(message.body),
      actor: <><span>{label}</span><Badge tone={messageTone(message.kind)}>{message.kind ?? "message"}</Badge>{message.response_intent && <Badge tone="muted">{message.response_intent === "response_required" ? "reply requested" : "informational"}</Badge>}</>,
      timestamp: formatTime(item.at),
      occurredAt: item.at,
      tone: messageTone(message.kind),
      evidenceRefs: message.evidence_refs,
      action: message.correlation_id || message.work_id ? (
        <span className="flex flex-wrap items-center justify-end gap-1">
          {message.work_id && <Badge tone="info" title={`Discusses Work ${message.work_id}`}><ShieldCheck className="size-2.5" /> Work</Badge>}
          {message.correlation_id && (
            <Badge
              tone="muted"
              title={`correlation ${message.correlation_id}${message.causation_id ? ` · caused by ${message.causation_id}` : ""}`}
            >
              <Link2 className="size-2.5" /> {message.causation_id ? "reply linked" : "linked"}
            </Badge>
          )}
        </span>
      ) : undefined,
      prominence: message.kind === "handoff" || needsAttention ? "primary" : "detail",
      source: "harness",
      rawText: `${message.body ?? ""} ${message.work_id ?? ""} ${message.correlation_id ?? ""} ${message.causation_id ?? ""}`,
      actorLabel: label,
      statusLabel: message.response_intent ?? message.kind ?? "message",
    };
  }
  if (item.kind === "action") {
    const action = item.action;
    const statusLine = action.provider_status || action.semantic_status
      ? `provider ${action.provider_status ?? "unknown"} · semantic ${action.semantic_status ?? "not classified"}`
      : undefined;
    return {
      id: item.id,
      kind: (action.evidence_refs?.length ?? 0) > 0 ? "evidence" : "action",
      glyph: (action.evidence_refs?.length ?? 0) > 0 ? "artifact" : "runtime",
      title: action.title ?? action.action_type ?? "Member action",
      body: statusLine ? <><span>{action.summary}</span><span className="mt-1 block text-[10px] text-muted-foreground">{statusLine}</span></> : action.summary,
      actor: context.member.name ?? context.member.id,
      timestamp: formatTime(item.at),
      occurredAt: item.at,
      tone: actionTone(action.status),
      evidenceRefs: action.evidence_refs,
      prominence: (action.evidence_refs?.length ?? 0) > 0 || action.status === "failed" ? "primary" : "detail",
      source: "harness",
      rawText: `${action.title ?? ""}\n${action.summary ?? ""}`,
      actorLabel: context.member.name ?? context.member.id,
      statusLabel: action.status ?? undefined,
    };
  }
  if (item.kind === "work_event") {
    const event = item.workEvent;
    const work = context.works.find((candidate) => candidate.id === event.work_id);
    const isBlocker = /blocked|cancelled/i.test(event.kind);
    const isReview = /submitted|changes_requested|accepted/i.test(event.kind);
    return {
      id: item.id,
      kind: isBlocker ? "blocker" : isReview ? "evidence" : "action",
      glyph: isReview ? "review" : isBlocker ? "runtime" : "complete",
      title: `Work ${humanizeWorkEventKind(event.kind)}`,
      body: work?.title ?? event.work_id,
      actor: event.performed_by_actor.display_name ?? event.performed_by_actor.id,
      timestamp: formatTime(item.at),
      occurredAt: item.at,
      tone: isBlocker ? "bad" : event.kind === "accepted" ? "good" : isReview ? "warn" : "info",
      action: <Badge tone="info" title={event.work_id}><ShieldCheck className="size-2.5" /> Work v{event.resulting_version}</Badge>,
      prominence: isBlocker || isReview ? "primary" : "detail",
      source: "harness",
      rawText: `${work?.title ?? ""} ${event.work_id} ${event.kind}`,
      actorLabel: event.performed_by_actor.display_name ?? event.performed_by_actor.id,
      statusLabel: event.kind,
    };
  }
  if (item.kind === "work_delivery") {
    const delivery = item.workDelivery;
    const work = context.works.find((candidate) => candidate.id === delivery.work_id);
    const failed = delivery.status === "failed";
    const deliveryBody = failed && delivery.failure_reason
      ? `${work?.title ?? delivery.work_id} · ${delivery.failure_reason}`
      : work?.title ?? delivery.work_id;
    return {
      id: item.id,
      kind: failed ? "blocker" : "action",
      glyph: failed ? "runtime" : delivery.status === "queued" ? "queued" : "complete",
      title: failed ? "Work delivery failed" : `Work delivery ${delivery.status}`,
      body: deliveryBody,
      actor: "Team supervisor",
      timestamp: formatTime(item.at),
      occurredAt: item.at,
      tone: failed ? "bad" : delivery.status === "provider_received" ? "good" : "info",
      action: <Badge tone="muted" title={delivery.work_id}>attempt {delivery.attempt}</Badge>,
      prominence: failed ? "pressure" : "detail",
      source: "harness",
      rawText: `${work?.title ?? ""} ${delivery.work_id} ${delivery.status} ${delivery.failure_reason ?? ""}`,
      actorLabel: "Team supervisor",
      statusLabel: delivery.status,
    };
  }
  const event = item.event;
  const isBlocker = /blocked|failed|error/i.test(`${event.operation ?? ""} ${event.summary ?? ""}`);
  return {
    id: item.id,
    kind: isBlocker ? "blocker" : "action",
    glyph: runtimeEventGlyph(event.summary ?? event.operation),
    title: event.summary ?? `${event.entity_type ?? "Team record"} ${event.operation ?? "updated"}`,
    actor: event.source_kind === "member" ? context.member.name ?? context.member.id : event.source_kind ?? "team",
    timestamp: formatTime(item.at),
    occurredAt: item.at,
    tone: isBlocker ? "bad" : event.operation === "completed" ? "good" : "info",
    prominence: isBlocker ? "pressure" : "detail",
    source: "harness",
    rawText: `${event.summary ?? ""}\n${event.operation ?? ""}`,
    actorLabel: event.source_kind === "member" ? context.member.name ?? context.member.id : event.source_kind ?? "team",
    statusLabel: event.operation ?? undefined,
  };
}

function humanizeWorkEventKind(kind: string): string {
  return kind.replace(/_/g, " ");
}

function compareActivityChronology(left: WorkbenchActivityItem, right: WorkbenchActivityItem): number {
  return parseTimestamp(left.occurredAt) - parseTimestamp(right.occurredAt);
}

function nativeToolDetails(summary?: string): ReactNode {
  if (!summary || summary === "provider recorded tool output") return undefined;
  return (
    <details className="group/tool max-w-full rounded-md border border-border/60 bg-muted/20 px-2.5 py-1.5">
      <summary className="cursor-pointer select-none text-[10px] font-medium text-muted-foreground hover:text-foreground">
        Tool details
      </summary>
      <pre className="mt-2 max-h-48 overflow-auto whitespace-pre-wrap break-words font-mono text-[10px] leading-relaxed text-muted-foreground">
        {summary}
      </pre>
    </details>
  );
}

function readableHistoryBody(text?: string | null): ReactNode {
  if (!text) return undefined;
  if (text.length <= 520) return <Markdown source={text} compact />;
  const preview = plainMarkdownPreview(text, 280);
  return (
    <div className="space-y-1.5">
      <p>{preview}</p>
      <details className="group/message max-w-full rounded-md border border-border/60 bg-muted/20 px-2.5 py-1.5">
        <summary className="cursor-pointer select-none text-[10px] font-medium text-muted-foreground hover:text-foreground">
          Show full message
        </summary>
        <div className="mt-2 max-h-80 overflow-auto border-t border-border/60 pt-2">
          <Markdown source={text} compact />
        </div>
      </details>
    </div>
  );
}

function plainMarkdownPreview(text: string, limit: number): string {
  const plain = text
    .replace(/^#{1,6}\s+/gm, "")
    .replace(/```[\s\S]*?```/g, "[code]")
    .replace(/\*\*([^*]+)\*\*/g, "$1")
    .replace(/`([^`]+)`/g, "$1")
    .replace(/^\s*[-*]\s+/gm, "")
    .replace(/\n+/g, " ")
    .trim();
  return plain.length > limit ? `${plain.slice(0, limit).trimEnd()}…` : plain;
}

function nativeToolGlyph(title: string): WorkbenchActivityItem["glyph"] {
  const normalized = title.toLowerCase();
  if (normalized.includes("spawn_agent") || normalized === "agent") return "spawn";
  if (normalized.includes("wait")) return "wait";
  if (normalized.includes("apply_patch") || normalized.includes("edit")) return "edit";
  if (normalized.includes("search") || normalized.includes("find") || normalized === "rg") return "search";
  if (normalized.includes("exec") || normalized.includes("bash") || normalized.includes("shell")) return "command";
  return "runtime";
}

function runtimeEventGlyph(value?: string | null): WorkbenchActivityItem["glyph"] {
  const normalized = (value ?? "").toLowerCase();
  if (normalized.includes("joined")) return "join";
  if (normalized.includes("queued")) return "queued";
  if (normalized.includes("starting") || normalized.includes("started")) return "start";
  if (normalized.includes("completed") || normalized.includes("finished")) return "complete";
  return "runtime";
}

function teamMessageTitle(kind?: string | null): string {
  switch (kind) {
    case "handoff": return "Member handoff";
    case "blocker": return "Blocker reported";
    case "review_request": return "Review requested";
    case "review_result": return "Review result";
    case "question": return "Member question";
    case "answer": return "Member answer";
    case "progress": return "Progress update";
    default: return "Team message";
  }
}

interface EvidenceItem { id: string; label: string; source: string }

function collectEvidence(context: MemberRunContext, model: WorkbenchModel): EvidenceItem[] {
  const entries = [
    ...(context.wave?.artifact_refs ?? []).map((ref) => ({ id: `wave:${ref}`, label: ref, source: "Wave artifact" })),
    ...context.ownedWorks.flatMap((work) => [
      ...(work.artifact_refs ?? []).map((ref) => ({ id: `work:${work.id}:artifact:${ref}`, label: ref, source: `Work artifact · ${work.title}` })),
      ...(work.check_refs ?? []).map((ref) => ({ id: `work:${work.id}:check:${ref}`, label: ref, source: `Work check · ${work.title}` })),
    ]),
    ...context.actionsForMember.flatMap((action) => (action.evidence_refs ?? []).map((ref) => ({ id: `action:${action.id}:${ref}`, label: ref, source: action.title ?? "Member action" }))),
    ...context.messagesForMember.flatMap((message) => (message.evidence_refs ?? []).map((ref) => ({ id: `message:${message.id}:${ref}`, label: ref, source: message.kind ?? "Team message" }))),
  ];
  return Array.from(new Map(entries.map((entry) => [entry.id, entry])).values());
}

function dispatch(onAction: MemberRunFocusProps["onAction"], descriptor: ActionDescriptor): void {
  onAction?.(descriptor.path, descriptor.body);
}

function isCurrentPreview(expiresAt: string | undefined, now: number): boolean {
  if (!expiresAt) return false;
  const timestamp = parseTimestamp(expiresAt);
  return timestamp > now;
}

function parseTimestamp(value?: string | null): number {
  if (!value) return 0;
  if (value.startsWith("unix-ms:")) return Number(value.slice("unix-ms:".length)) || 0;
  return Date.parse(value) || 0;
}

function formatTime(value?: string | null): string {
  const timestamp = parseTimestamp(value);
  if (!timestamp) return "time unavailable";
  return new Date(timestamp).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
}

function formatRelative(value?: string | null): string {
  const timestamp = parseTimestamp(value);
  if (!timestamp) return "Not reported";
  const minutes = Math.max(0, Math.floor((Date.now() - timestamp) / 60_000));
  return minutes < 1 ? "just now" : `${minutes}m ago`;
}

function isFinishedMember(status?: string | null): boolean {
  return ["completed", "failed", "stopped"].includes((status ?? "").toLowerCase());
}

function memberStatusTone(status?: string | null): StatusTone {
  const normalized = (status ?? "").toLowerCase();
  if (normalized === "completed") return "good";
  if (["blocked", "failed", "stopped"].includes(normalized)) return "bad";
  if (["waiting", "reviewing"].includes(normalized)) return "warn";
  if (normalized === "running") return "running";
  if (["queued", "starting"].includes(normalized)) return "info";
  return "idle";
}

function teamStatusTone(status?: string | null): StatusTone {
  if (status === "completed") return "good";
  if (status === "failed" || status === "cancelled") return "bad";
  if (status === "waiting" || status === "reviewing") return "warn";
  if (status === "running") return "running";
  return status === "planning" ? "info" : "idle";
}

function waveGateTone(status?: string | null): StatusTone {
  if (status === "accepted") return "good";
  if (status === "blocked") return "bad";
  if (status === "revise") return "warn";
  return "idle";
}

function messageTone(kind?: string | null): StatusTone {
  if (kind === "blocker") return "bad";
  if (kind === "review_request") return "warn";
  if (kind === "review_result" || kind === "answer") return "good";
  if (kind === "handoff" || kind === "question") return "decision";
  if (kind === "progress") return "running";
  return kind === "broadcast" ? "info" : "idle";
}

function workStatusTone(status?: string | null): StatusTone {
  if (status === "done") return "good";
  if (status === "cancelled") return "bad";
  if (status === "blocked") return "warn";
  if (status === "in_progress") return "running";
  if (status === "review") return "info";
  return "idle";
}

function actionTone(status?: string | null): StatusTone {
  if (status === "succeeded") return "good";
  if (status === "failed" || status === "cancelled") return "bad";
  if (status === "started") return "running";
  return status === "progress" ? "info" : "idle";
}
