import { useEffect, useState } from "react";
import {
  ArrowLeft,
  Bot,
  CheckCircle2,
  ChevronRight,
  Clock3,
  FileCheck2,
  FileText,
  GitBranch,
  MessageSquare,
  Send,
  Square,
  ShieldAlert,
  ShieldCheck,
  Sparkles,
  Users,
  Wrench,
} from "lucide-react";

import {
  interruptTeamMember,
  resolvePendingInteraction,
  sendTeamMessage,
  steerTeamMember,
  type ActionDescriptor,
} from "@/api/actions";
import { Avatar } from "@/components/workbench/Avatar";
import { TextArea } from "@/components/workbench/OperatorForms";
import { ActivityStream, type WorkbenchActivityItem } from "@/components/workbench/activity/ActivityStream";
import { ContextModule, ContextRail } from "@/components/workbench/context/ContextRail";
import { TeamRunCompact } from "@/components/workbench/entities/TeamRunControls";
import { FocusHeader, FocusShell } from "@/components/workbench/layout/FocusShell";
import { EmptyState, MonoId, StatusDot, type StatusTone } from "@/components/workbench/atoms";
import { memberTone } from "@/components/workbench/tones";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";
import { selectMemberRunContext, type MemberRunContext, type StableTeamActivity } from "@/model/teamSelectors";
import type { WorkbenchModel } from "@/model/readModel";
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
}: MemberRunFocusProps) {
  const [now, setNow] = useState(() => Date.now());
  const [draft, setDraft] = useState("");
  const [messageKind, setMessageKind] = useState("question");
  const [showFullActivity, setShowFullActivity] = useState(false);

  useEffect(() => {
    const timer = window.setInterval(() => setNow(Date.now()), 1_000);
    return () => window.clearInterval(timer);
  }, []);

  const context = selectMemberRunContext(model.snapshot, memberRunId);

  if (!context) {
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
  const activityItems = toActivityItems(context, livePreview?.preview);
  const shownActivity = showFullActivity
    ? activityItems
    : projectKeyActivity(activityItems);
  const evidence = collectEvidence(context, model);
  const session = (model.snapshot.provider_sessions ?? []).find(
    (candidate) => candidate.id === context.member.provider_session_id,
  );

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
      className="min-h-0"
      headerClassName="min-h-[118px] bg-background py-3 sm:py-3"
      composerClassName="bg-background shadow-[0_-12px_30px_-28px_rgba(15,23,42,0.55)]"
      responsiveContextVariant="sheet"
      header={
        <FocusHeader
          eyebrow="Member run"
          breadcrumb={
            <Breadcrumb
              context={context}
              onSelectionChange={onSelectionChange}
              onBack={goBackToTeam}
            />
          }
          title={
            <span className="flex min-w-0 items-center gap-2">
              <Avatar name={context.member.name ?? context.member.id} tone={memberTone(context.member.status)} />
              <span className="truncate">{context.member.name ?? context.member.id}</span>
            </span>
          }
          description={context.member.role ?? "Team member"}
          meta={
            <>
              <Badge tone={memberStatusTone(context.member.status)}>{context.member.status ?? "unknown"}</Badge>
              <span className="text-[11px] text-muted-foreground">
                {context.member.provider ?? "provider"}{context.member.model ? ` · ${context.member.model}` : ""}
              </span>
              {context.member.slot_id && <MonoId>{context.member.slot_id}</MonoId>}
            </>
          }
          actions={
            <div className="flex items-center gap-1.5">
              {context.member.status === "running" && context.member.provider_profile?.supports_cancel && (
                <Button
                  size="sm"
                  variant="outline"
                  disabled={!actionsEnabled}
                  onClick={() => dispatch(onAction, interruptTeamMember(context.run.id, context.member.id))}
                >
                  <Square className="size-3 fill-current" /> Interrupt
                </Button>
              )}
              <Button size="sm" variant="ghost" onClick={goBackToTeam}>
                <ArrowLeft className="size-3.5" /> Back to team
              </Button>
            </div>
          }
        />
      }
      context={
        <MemberContextRail
          context={context}
          evidence={evidence}
          sessionStatus={session?.status}
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
      <div className="mx-auto flex w-full max-w-[1040px] flex-col px-4 py-2 sm:px-5">
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
        {assignment && (
          <div className="mb-2 rounded-lg border border-border/80 bg-background px-3 py-2.5 shadow-[0_8px_24px_-24px_rgba(15,23,42,0.55)]">
            <div className="flex min-w-0 items-center gap-2">
              <ShieldCheck className="size-3.5 shrink-0 text-status-info" />
              <span className="min-w-0 flex-1 truncate text-[12px] font-semibold text-foreground">
                Assignment: {assignment.assignment.body ?? "Assignment contract"}
              </span>
              {assignment.correlationId && <Badge tone="info">anchored</Badge>}
            </div>
          </div>
        )}
        <section className="min-h-[18rem] overflow-hidden bg-background">
          <header className="flex items-center justify-between gap-3 border-b border-border/70 py-2.5">
            <div>
              <h2 className="text-[12px] font-semibold text-foreground">Member activity</h2>
              <p className="text-[10px] text-muted-foreground">Assignment, work, evidence, and pressure in one record.</p>
            </div>
            <button
              type="button"
              aria-pressed={showFullActivity}
              onClick={() => setShowFullActivity((value) => !value)}
              className="rounded-md border border-border/70 px-2 py-1 text-[10px] font-medium text-muted-foreground transition-colors hover:border-primary/30 hover:text-foreground"
            >
              {showFullActivity ? "Key activity" : `Full record · ${activityItems.length}`}
            </button>
          </header>
          <ActivityStream
            items={shownActivity}
            variant="timeline"
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
    <ContextRail quiet label="Member context">
      <ContextModule
        title={context.wave ? `Wave ${context.wave.index} · ${context.wave.title}` : "Wave context unavailable"}
        icon={<GitBranch className="size-3.5" />}
        tone={gateTone}
        collapsible
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
            <p className="text-foreground">{context.wave.objective}</p>
            <RailKeyValue label="Executor" value={context.wave.executor_kind} />
            <RailKeyValue label="Exit criteria" value={context.wave.exit_criteria ?? "Not recorded"} />
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
        collapsible
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

      <ContextModule title="Outputs & evidence" icon={<FileCheck2 className="size-3.5" />} tone={evidence.length ? "good" : "idle"} collapsible>
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

      <ContextModule title="Runtime" icon={<Wrench className="size-3.5" />} tone={memberStatusTone(context.member.status)} collapsible defaultOpen={false}>
        <div className="space-y-1.5 text-[12px]">
          <RailKeyValue label="Provider" value={context.member.provider ?? "Not recorded"} />
          <RailKeyValue label="Execution mode" value={context.member.provider_profile?.execution_mode ?? "Not recorded"} />
          <RailKeyValue label="Provider version" value={context.member.provider_profile?.provider_version ?? "Not reported"} />
          <RailKeyValue label="Adapter contract" value={context.member.provider_profile?.adapter_contract_version ?? "Not recorded"} />
          <RailKeyValue label="Compatibility" value={context.member.provider_profile?.compatibility_status ?? "unknown"} />
          <RailKeyValue label="Adapter reviewed" value={context.member.provider_profile?.adapter_reviewed_at ?? "Not recorded"} />
          <RailKeyValue label="Interaction" value={context.member.provider_profile?.interaction_mode ?? "Unsupported or unknown"} />
          <RailKeyValue label="Tool events" value={context.member.provider_profile?.tool_event_fidelity ?? "Not reported"} />
          <RailKeyValue label="Model" value={context.member.model ?? "Not recorded"} />
          <RailKeyValue label="Session" value={context.member.provider_session_id ?? context.member.acp_session_id ?? "Unavailable"} mono />
          <RailKeyValue label="Session status" value={sessionStatus ?? "Not reported"} />
          <RailKeyValue label="Worktree" value={context.member.worktree_ref ?? "Not recorded"} mono />
          <RailKeyValue label="Budget" value="Not reported at member scope" />
          <RailKeyValue label="Last activity" value={formatRelative(context.member.last_event_at)} />
        </div>
      </ContextModule>

      <ContextModule title="Delegations" icon={<Bot className="size-3.5" />} tone={context.delegationsForMember.length ? "decision" : "idle"} collapsible defaultOpen={false}>
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

function toActivityItems(context: MemberRunContext, transientPreview?: string): WorkbenchActivityItem[] {
  const durable = context.activityForMember.map((item) => toActivityItem(item, context));
  if (!transientPreview) return durable;
  return [
    {
      id: `live:${context.member.id}`,
      kind: "thinking",
      title: "Current provider preview",
      body: transientPreview,
      actor: context.member.name ?? context.member.id,
      timestamp: "now",
      transient: true,
      tone: "decision",
      glyph: "runtime",
      prominence: "primary",
    },
    ...durable,
  ];
}

/** Keep the default member narrative inside one viewport without rewriting the
 * durable record. Pressure, live state, assignment, latest evidence, and
 * handoff are selected first; Full record exposes every remaining item. */
function projectKeyActivity(items: WorkbenchActivityItem[]): WorkbenchActivityItem[] {
  const visible = items.filter((item) => item.prominence !== "detail");
  if (visible.length <= 6) return visible;

  const selected = new Set<string>();
  const select = (item: WorkbenchActivityItem | undefined) => item && selected.add(item.id);
  visible.filter((item) => item.transient || item.prominence === "pressure").forEach(select);
  select(visible.find((item) => item.glyph === "assignment"));
  select(findLastItem(visible, (item) => item.kind === "evidence"));
  select(findLastItem(visible, (item) => item.glyph === "handoff"));
  for (let index = visible.length - 1; index >= 0; index -= 1) {
    if (selected.size >= 6) break;
    select(visible[index]);
  }
  return visible.filter((item) => selected.has(item.id));
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
      title: message.kind === "assignment" ? "Host assignment" : message.body ?? `${message.kind ?? "message"} message`,
      body: message.kind === "assignment" ? message.body : undefined,
      actor: <><span>{label}</span><Badge tone={messageTone(message.kind)}>{message.kind ?? "message"}</Badge></>,
      timestamp: formatTime(item.at),
      tone: messageTone(message.kind),
      evidenceRefs: message.evidence_refs,
      action: message.correlation_id ? <Badge tone="muted">{message.correlation_id}</Badge> : undefined,
      prominence: assignment || needsAttention || ["handoff", "progress"].includes(message.kind ?? "") ? (needsAttention ? "pressure" : "primary") : "detail",
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
      tone: actionTone(action.status),
      evidenceRefs: action.evidence_refs,
      prominence: (action.evidence_refs?.length ?? 0) > 0 || action.status === "failed" ? "primary" : "detail",
    };
  }
  const event = item.event;
  const isBlocker = /blocked|failed|error/i.test(`${event.operation ?? ""} ${event.summary ?? ""}`);
  return {
    id: item.id,
    kind: isBlocker ? "blocker" : "action",
    glyph: "runtime",
    title: event.summary ?? `${event.entity_type ?? "Team record"} ${event.operation ?? "updated"}`,
    actor: event.source_kind ?? "team",
    timestamp: formatTime(item.at),
    tone: isBlocker ? "bad" : event.operation === "completed" ? "good" : "info",
    prominence: isBlocker ? "pressure" : "detail",
  };
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
