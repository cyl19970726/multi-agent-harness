import { useEffect, useState, type ReactNode } from "react";
import {
  ArrowLeft,
  Bot,
  CheckCircle2,
  ChevronRight,
  Clock3,
  FileCheck2,
  FileText,
  GitBranch,
  Link2,
  MessageSquare,
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
  interruptTeamMember,
  resolvePendingInteraction,
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
import { selectMemberRunContext, type MemberRunContext, type StableTeamActivity } from "@/model/teamSelectors";
import type { WorkbenchModel } from "@/model/readModel";
import type { NativeActivityItem, NativeActivityProjection } from "@/types";
import type { SelectionState } from "@/app/selection";

const ACTIONS_DISABLED_HINT = "Connect a live source to message this member";

export interface MemberRunFocusProps {
  model: WorkbenchModel;
  memberRunId?: string;
  onSelectionChange: (selection: Partial<SelectionState>) => void;
  /** True only when the dashboard is connected to a writable live source. */
  actionsEnabled?: boolean;
  /** Posts a harness action and refreshes the dashboard snapshot. */
  onAction?: (path: string, body?: unknown) => void;
  /** Live Harness API used for on-demand provider-native activity reads. */
  apiUrl?: string;
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
  onSelectionChange,
  actionsEnabled = false,
  onAction,
  apiUrl,
  isLoading = false,
}: MemberRunFocusProps) {
  const [now, setNow] = useState(() => Date.now());
  const [draft, setDraft] = useState("");
  const [messageKind, setMessageKind] = useState("question");
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

  useEffect(() => {
    setNativeActivity(undefined);
    setNativeActivityState(apiUrl && memberRunId ? "loading" : "idle");
    if (!apiUrl || !memberRunId) return;
    const project = new URLSearchParams(window.location.search).get("project");
    let cancelled = false;
    fetchNativeMemberActivity(apiUrl, memberRunId, project)
      .then((projection) => {
        if (!cancelled) {
          setNativeActivity(projection);
          setNativeActivityState("ready");
        }
      })
      .catch(() => {
        if (!cancelled) setNativeActivityState("unavailable");
      });
    return () => { cancelled = true; };
  }, [apiUrl, memberRunId, context?.member.native_session?.native_session_id]);

  if (!context) {
    if (isLoading) {
      return <div className="grid min-h-0 flex-1 place-items-center bg-background text-sm text-muted-foreground">Loading member history…</div>;
    }
    return <MemberRunNotFound memberRunId={memberRunId} onSelectionChange={onSelectionChange} />;
  }

  const finished = isFinishedMember(context.member.status);
  const livePreview = isCurrentPreview(context.liveActivity?.expires_at, now)
    ? context.liveActivity
    : undefined;
  const assignment = context.assignments[0];
  const pendingInteraction = context.interactions.find(
    (interaction) => interaction.member_run_id === context.member.id && interaction.status === "pending",
  );
  const activityItems = toActivityItems(context, livePreview?.preview, nativeActivity?.items);
  const shownActivity = showFullActivity
    ? activityItems
    : projectKeyActivity(activityItems);
  const evidence = collectEvidence(context, model);

  const goBackToTeam = () =>
    onSelectionChange({
      surface: "team",
      teamId: context.run.id,
      memberRunId: undefined,
    });

  const dispatchMessage = () => {
    const body = draft.trim();
    if (!body || !actionsEnabled || finished) return;
    const liveSteer = context.member.provider_profile?.execution_mode === "codex_app_server"
      && context.member.status === "running";
    const descriptor = liveSteer
      ? steerTeamMember(context.run.id, context.member.id, body)
      : sendTeamMessage(context.run.id, {
        fromMemberId: "host",
        toMemberIds: [context.member.id],
        kind: messageKind,
        body,
        correlationId: assignment?.correlationId,
        causationId: assignment?.assignment.id,
      });
    dispatch(onAction, descriptor);
    setDraft("");
  };

  return (
    <FocusShell
      className="member-focus-theme min-h-0 bg-[#fdfcf9]"
      headerClassName="h-[152px] bg-[#fdfcf9] px-11 py-0 sm:px-11"
      composerClassName="bg-background px-8 py-2.5 shadow-[0_-12px_30px_-28px_rgba(15,23,42,0.55)]"
      responsiveContextVariant="sheet"
      mainLabel="Member work history"
      header={
        <MemberHeroHeader
          context={context}
          actionsEnabled={actionsEnabled}
          onAction={onAction}
          onBack={goBackToTeam}
        />
      }
      context={
        <MemberContextRail
          context={context}
          evidence={evidence}
          sessionStatus={context.member.native_session?.availability}
          onSelectionChange={onSelectionChange}
        />
      }
      composer={
        <MemberComposer
          value={draft}
          kind={messageKind}
          disabled={!actionsEnabled || finished}
          disabledReason={finished ? "This member run is finished; its history is read-only." : ACTIONS_DISABLED_HINT}
          deliveryHint={context.member.provider_profile?.execution_mode === "codex_app_server" && context.member.status === "running"
            ? "Steers the active Codex turn."
            : "Queues the message for the member's next provider round."}
          onChange={setDraft}
          onKindChange={setMessageKind}
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
        <section className="min-h-[18rem] overflow-hidden bg-background" data-native-activity-state={nativeActivityState}>
          <header className="flex h-[58px] items-center justify-between gap-3 border-b border-border/70">
            <h2 className="text-[20px] font-semibold tracking-[-0.025em] text-foreground">Work history</h2>
            <div className="flex items-center gap-2">
              <span className="rounded-lg border border-[#e5dfd9] bg-[#fffefa] px-3 py-2 text-[11px] font-medium text-foreground">Complete history · {activityItems.length}</span>
              <button type="button" aria-pressed={!showFullActivity} onClick={() => setShowFullActivity((value) => !value)} className="rounded-lg border border-[#e5dfd9] bg-[#fffefa] px-3 py-2 text-[11px] font-medium text-muted-foreground transition-colors hover:border-[#f08068] hover:text-foreground">
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
  actionsEnabled,
  onAction,
  onBack,
}: {
  context: MemberRunContext;
  actionsEnabled: boolean;
  onAction?: MemberRunFocusProps["onAction"];
  onBack: () => void;
}) {
  const name = context.member.name ?? context.member.id;
  return (
    <header className="flex h-full min-w-0 items-center justify-between gap-6">
      <div className="flex min-w-0 items-end gap-9 self-stretch">
        <div className="relative flex h-full w-[130px] shrink-0 items-end justify-center overflow-hidden">
          <span className="absolute inset-x-1 bottom-0 h-[118px] overflow-hidden rounded-t-[64px] border border-b-0 border-[#eadfd7] bg-[linear-gradient(180deg,#fff8f3,#f6ede6)] shadow-[0_22px_44px_-34px_rgba(91,57,36,.7)] [&>span]:size-[116px] [&>span]:rounded-none [&>span]:border-0 [&>span]:ring-0">
            <Avatar name={name} identity={context.member.role ?? context.member.id} tone={memberTone(context.member.status)} size="xl" />
          </span>
        </div>
        <div className="min-w-0 self-center pb-1">
          <h1 className="truncate text-[29px] font-semibold tracking-[-0.035em] text-foreground">{name}</h1>
          <p className="mt-1 text-[12px] text-muted-foreground">{context.member.role ?? "Team member"}</p>
          <div className="mt-4 flex flex-wrap items-center gap-3 text-[11px]">
            <span className="inline-flex items-center gap-1.5 font-medium text-status-good"><StatusDot tone={memberStatusTone(context.member.status)} /> {context.member.status ?? "unknown"}</span>
            <span className="h-4 w-px bg-border" />
            <span className="text-muted-foreground">Provider</span>
            <span className="text-foreground">{context.member.provider ?? "provider"}{context.member.model ? ` · ${context.member.model}` : ""}</span>
          </div>
        </div>
      </div>
      <div className="flex shrink-0 items-center gap-2">
        {context.member.status === "running" && context.member.provider_profile?.supports_cancel && (
          <Button size="sm" variant="outline" disabled={!actionsEnabled} onClick={() => dispatch(onAction, interruptTeamMember(context.run.id, context.member.id))}>
            <Square className="size-3 fill-current" /> Interrupt
          </Button>
        )}
        <Button size="sm" variant="outline" onClick={onBack}><ArrowLeft className="size-3.5" /> Back to team</Button>
      </div>
    </header>
  );
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

function MemberContextRail({
  context,
  evidence,
  sessionStatus,
  onSelectionChange,
}: {
  context: MemberRunContext;
  evidence: EvidenceItem[];
  sessionStatus?: string;
  onSelectionChange: MemberRunFocusProps["onSelectionChange"];
}) {
  const assignment = context.assignments[0];
  const activeMembers = context.members.filter((member) => member.status === "running").length;
  const gateTone = waveGateTone(context.wave?.gate_status);

  return (
    <ContextRail label="Member context" hideHeader className="bg-[#fbfaf7]" contentClassName="flex flex-col gap-4 space-y-0 p-5">
      <ContextModule
        title={context.wave ? `Wave ${context.wave.index} · ${context.wave.title}` : "Wave context unavailable"}
        icon={<GitBranch className="size-3.5" />}
        tone={gateTone}
        className="order-2 rounded-xl bg-card shadow-[0_14px_34px_-32px_rgba(15,23,42,.65)]"
        action={
          context.wave ? (
            <RailOpenButton
              label="Open wave"
              onClick={() => onSelectionChange({ surface: "missions", missionId: context.wave?.mission_id, waveId: context.wave?.id })}
            />
          ) : undefined
        }
      >
        {context.wave ? (
          <div className="space-y-2 text-[12px]">
            <p className="line-clamp-3 leading-relaxed text-foreground">{context.wave.objective}</p>
            <RailKeyValue label="Executor" value={context.wave.executor_kind} />
            <div className="flex flex-wrap gap-1.5 pt-0.5">
              <Badge tone={gateTone}>gate {context.wave.gate_status ?? "pending"}</Badge>
              <Badge tone="muted">attempt {context.attempts.findIndex((attempt) => attempt.id === context.run.id) + 1}/{context.attempts.length}</Badge>
            </div>
          </div>
        ) : (
          <RailEmpty>Parent Wave is not present in this snapshot.</RailEmpty>
        )}
      </ContextModule>

      <ContextModule
        title="Agent Team"
        icon={<Users className="size-3.5" />}
        tone={teamStatusTone(context.run.status)}
        className="order-1 rounded-xl bg-card shadow-[0_14px_34px_-32px_rgba(15,23,42,.65)]"
        action={<RailOpenButton label="Open team" onClick={() => onSelectionChange({ surface: "team", teamId: context.run.id, memberRunId: undefined })} />}
      >
        <TeamRunCompact
          run={context.run}
          members={context.members}
          needsYouCount={context.needsYou.total}
          onOpen={() => onSelectionChange({ surface: "team", teamId: context.run.id, memberRunId: undefined })}
        />
        <p className="mt-2 text-[11px] text-muted-foreground">
          {activeMembers} active · {context.needsYou.total ? `${context.needsYou.total} needs attention` : "no open signals"}
        </p>
      </ContextModule>

      <ContextModule
        title="Assignment"
        icon={<ShieldCheck className="size-3.5" />}
        tone={assignment ? "info" : "warn"}
        collapsible
        defaultOpen={false}
        className="order-5 hidden rounded-xl bg-card"
      >
        {assignment ? (
          <div className="space-y-2.5 text-[12px]">
            <p className="whitespace-pre-wrap text-foreground">{assignment.assignment.body ?? "No assignment body recorded."}</p>
            <RailKeyValue label="From" value={assignment.assignment.from_member_id === "host" ? "Host" : assignment.assignment.from_member_id ?? "Unknown"} />
            <RailKeyValue label="Assigned" value={formatTime(assignment.assignment.created_at)} />
            <RailKeyValue label="Correlation" value={assignment.correlationId ?? "Not recorded"} mono />
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
          <RailEmpty>No assignment recorded. Observed activity does not prove ownership.</RailEmpty>
        )}
      </ContextModule>

      <ContextModule title="Artifacts & evidence" icon={<FileCheck2 className="size-3.5" />} tone={evidence.length ? "good" : "idle"} className="order-4 rounded-xl bg-card shadow-[0_14px_34px_-32px_rgba(15,23,42,.65)]">
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

      <ContextModule title="Runtime" icon={<Wrench className="size-3.5" />} tone={memberStatusTone(context.member.status)} className="order-3 rounded-xl bg-card shadow-[0_14px_34px_-32px_rgba(15,23,42,.65)]">
        <div className="space-y-1.5 text-[12px]">
          <RailKeyValue label="Provider" value={context.member.provider ?? "Not recorded"} />
          <RailKeyValue label="Execution mode" value={context.member.provider_profile?.execution_mode ?? "Not recorded"} />
          <RailKeyValue label="Compatibility" value={context.member.provider_profile?.compatibility_status ?? "unknown"} />
          <RailKeyValue label="Model" value={context.member.model ?? "Not recorded"} />
          <RailKeyValue label="Native session" value={context.member.native_session?.native_session_id ?? "Unavailable"} mono />
          <RailKeyValue label="Resume" value={context.member.native_session?.supports_resume ? "Supported" : "Not verified"} />
          <RailKeyValue label="Actual cwd" value={context.member.workspace_snapshot?.cwd ?? "Not captured (legacy run)"} mono />
          <RailKeyValue label="Git branch" value={context.member.workspace_snapshot?.git_branch ?? "Detached or not captured"} mono />
          <RailKeyValue label="Last activity" value={formatRelative(context.member.last_event_at)} />
          <details className="pt-1 text-[10px] text-muted-foreground">
            <summary className="cursor-pointer font-medium hover:text-foreground">Advanced runtime facts</summary>
            <div className="mt-2 space-y-1.5 border-l border-border pl-2.5 text-[11px]">
              <RailKeyValue label="Provider version" value={context.member.provider_profile?.provider_version ?? "Not reported"} />
              <RailKeyValue label="Adapter contract" value={context.member.provider_profile?.adapter_contract_version ?? "Not recorded"} />
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

      <ContextModule title="Delegations" icon={<Bot className="size-3.5" />} tone={context.delegationsForMember.length ? "decision" : "idle"} collapsible defaultOpen={false} className="order-6 hidden rounded-xl bg-card">
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
        ) : <RailEmpty>No observed child work for this member.</RailEmpty>}
      </ContextModule>
    </ContextRail>
  );
}

function formatWorkspaceRoots(roots?: string[]): string {
  return roots?.length ? roots.join(" · ") : "None discovered or not captured";
}

function MemberComposer({
  value,
  kind,
  disabled,
  disabledReason,
  deliveryHint,
  onChange,
  onKindChange,
  onSend,
}: {
  value: string;
  kind: string;
  disabled: boolean;
  disabledReason: string;
  deliveryHint: string;
  onChange: (value: string) => void;
  onKindChange: (value: string) => void;
  onSend: () => void;
}) {
  if (disabled) {
    return (
      <div className="mx-auto flex h-10 w-full max-w-4xl items-center gap-3 rounded-xl border border-border/70 bg-muted/20 px-3.5 text-[11px] text-muted-foreground">
        <MessageSquare className="size-3.5 shrink-0" />
        <span className="min-w-0 flex-1 truncate">{disabledReason}</span>
        <Badge tone="muted">read only</Badge>
      </div>
    );
  }
  return (
    <form
      className="mx-auto flex w-full max-w-4xl items-end gap-2"
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
      <select
        aria-label="Message type"
        value={kind}
        disabled={disabled}
        onChange={(event) => onKindChange(event.target.value)}
        className="h-8 max-w-28 rounded-md border border-border bg-background px-2 text-[11px] text-foreground outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:opacity-50"
      >
        <option value="question">Clarify</option>
        <option value="review_request">Review</option>
        <option value="handoff">Handoff</option>
      </select>
      <Button type="submit" size="icon" disabled={disabled || !value.trim()} aria-label="Send message">
        <Send className="size-3.5" />
      </Button>
    </form>
  );
}

function RailOpenButton({ label, onClick }: { label: string; onClick: () => void }) {
  return <button type="button" onClick={onClick} className="text-[10px] font-medium text-primary hover:underline">{label}</button>;
}

function RailEmpty({ children }: { children: string }) {
  return <p className="text-[12px] leading-relaxed text-muted-foreground">{children}</p>;
}

function RailKeyValue({ label, value, mono = false }: { label: string; value: string; mono?: boolean }) {
  return <div className="flex min-w-0 items-start justify-between gap-3"><span className="shrink-0 text-muted-foreground">{label}</span><span className={cn("min-w-0 text-right text-foreground", mono && "truncate font-mono text-[11px]")}>{value}</span></div>;
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
  select(visible.find((item) => item.glyph === "assignment"));
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
    const assignment = message.kind === "assignment";
    const needsAttention = message.kind === "blocker" || message.kind === "review_request";
    return {
      id: item.id,
      kind: needsAttention ? "blocker" : assignment ? "decision" : "message",
      glyph: assignment ? "assignment" : message.kind === "handoff" ? "handoff" : message.kind === "review_request" ? "review" : "message",
      title: teamMessageTitle(message.kind),
      body: readableHistoryBody(message.body),
      actor: <><span>{label}</span><Badge tone={messageTone(message.kind)}>{message.kind ?? "message"}</Badge></>,
      timestamp: formatTime(item.at),
      occurredAt: item.at,
      tone: messageTone(message.kind),
      evidenceRefs: message.evidence_refs,
      action: message.correlation_id ? (
        <Badge tone="muted" title={message.correlation_id}>
          <Link2 className="size-2.5" /> linked
        </Badge>
      ) : undefined,
      prominence: assignment || needsAttention || ["handoff", "progress"].includes(message.kind ?? "") ? (needsAttention ? "pressure" : "primary") : "detail",
      source: "harness",
      rawText: message.body,
      actorLabel: label,
      statusLabel: message.kind ?? "message",
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
    case "assignment": return "Host assignment";
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
  return kind === "assignment" || kind === "broadcast" ? "info" : "idle";
}

function actionTone(status?: string | null): StatusTone {
  if (status === "succeeded") return "good";
  if (status === "failed" || status === "cancelled") return "bad";
  if (status === "started") return "running";
  return status === "progress" ? "info" : "idle";
}
