import { useEffect, useRef, useState, type ReactNode } from "react";
import { CheckCircle2, ListFilter, ListTodo, MessageSquare, SendHorizontal, Users, X } from "lucide-react";

import { cn } from "@/lib/utils";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Avatar } from "@/components/workbench/Avatar";
import { Markdown } from "@/components/workbench/Markdown";
import { StatusDot, type StatusTone } from "@/components/workbench/atoms";
import { Select, TextArea } from "@/components/workbench/OperatorForms";

import {
  canMemberAcceptWork,
  selectFilteredTeamWorks,
  selectWorkOwnerMember,
  type TeamWorksAttentionFilter,
  type TeamWorksOwnerFilter,
} from "../../../model/teamSelectors";
import { assignTeamWork, createTeamWork, reviewTeamWork, type ActionDescriptor } from "../../../api/actions";
import { workIsTerminal, workLifecycleLabel, type MemberRun, type Work, type WorkDelivery, type WorkEvent } from "../../../types";
import { formatTime, memberTone, shortId, workTone } from "./teamFormat";

type WorkLane = "open" | "assigned" | "doing" | "review" | "done";

const WORK_LANES: Array<{ id: WorkLane; label: string; statuses: string[]; tone: StatusTone }> = [
  { id: "open", label: "Open · unassigned", statuses: ["open"], tone: "idle" },
  { id: "assigned", label: "Open · assigned", statuses: ["open"], tone: "info" },
  { id: "doing", label: "Active & blocked", statuses: ["active", "blocked", "on_hold"], tone: "running" },
  { id: "review", label: "Review", statuses: ["review"], tone: "warn" },
  { id: "done", label: "Closed", statuses: ["accepted", "failed", "cancelled", "closed"], tone: "good" },
];

/** Focusable descendants used to keep Tab inside the modal Work sheet. */
function focusableWithin(root: HTMLElement): HTMLElement[] {
  return Array.from(
    root.querySelectorAll<HTMLElement>(
      'a[href], button:not([disabled]), textarea:not([disabled]), input:not([disabled]), select:not([disabled]), [tabindex]:not([tabindex="-1"])',
    ),
  ).filter((element) => element.offsetParent !== null || element === document.activeElement);
}

/**
 * The shared Works board.
 *
 * Lanes are one projection over Work lifecycle and ownership, rendered exactly
 * once: the same lane sections reflow from five desktop columns to a stacked
 * mobile status list through CSS, so a large board never duplicates its cards
 * into a second hidden container.
 */
export function TeamWorksBoard({
  works,
  workEvents,
  workDeliveries,
  members,
  selectedWorkId,
  actionsEnabled,
  teamRunId,
  onSelectWork,
  onOpenMember,
  onDiscuss,
  onAction,
}: {
  works: Work[];
  workEvents: WorkEvent[];
  workDeliveries: WorkDelivery[];
  members: MemberRun[];
  selectedWorkId?: string;
  actionsEnabled: boolean;
  teamRunId: string;
  onSelectWork: (id: string | undefined) => void;
  onOpenMember: (member: MemberRun) => void;
  onDiscuss: (work: Work) => void;
  onAction: (descriptor: ActionDescriptor) => void;
}) {
  const [creating, setCreating] = useState(false);
  const [title, setTitle] = useState("");
  const [criteria, setCriteria] = useState("");
  const [ownerId, setOwnerId] = useState("");
  const [reviewNote, setReviewNote] = useState("");
  const [ownerFilter, setOwnerFilter] = useState<TeamWorksOwnerFilter>("all");
  const [attentionFilter, setAttentionFilter] = useState<TeamWorksAttentionFilter>("all");
  const [filtersOpen, setFiltersOpen] = useState(false);
  const closeButtonRef = useRef<HTMLButtonElement>(null);
  const dialogRef = useRef<HTMLElement>(null);
  const returnFocusRef = useRef<HTMLElement | null>(null);
  const selected = works.find((work) => work.id === selectedWorkId);
  const active = works.filter((work) => !workIsTerminal(work)).length;
  const assignableMembers = members.filter(canMemberAcceptWork);
  const ownerFor = (work: Work) => selectWorkOwnerMember(work, members);
  const visibleWorks = selectFilteredTeamWorks(works, members, ownerFilter, attentionFilter);
  const ownerCount = (filter: TeamWorksOwnerFilter) =>
    selectFilteredTeamWorks(works, members, filter, attentionFilter).length;
  const attentionCount = (filter: TeamWorksAttentionFilter) =>
    selectFilteredTeamWorks(works, members, ownerFilter, filter).length;

  useEffect(() => {
    if (!selected) return undefined;
    closeButtonRef.current?.focus();
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        onSelectWork(undefined);
        return;
      }
      // `aria-modal` promises the rest of the page is inert; without a trap the
      // next Tab silently escapes into the board behind the sheet.
      if (event.key !== "Tab" || !dialogRef.current) return;
      const focusable = focusableWithin(dialogRef.current);
      if (focusable.length === 0) return;
      const first = focusable[0];
      const last = focusable[focusable.length - 1];
      const current = document.activeElement as HTMLElement | null;
      if (!event.shiftKey && current === last) {
        event.preventDefault();
        first.focus();
      } else if (event.shiftKey && current === first) {
        event.preventDefault();
        last.focus();
      } else if (current && !dialogRef.current.contains(current)) {
        event.preventDefault();
        first.focus();
      }
    };
    document.addEventListener("keydown", onKeyDown);
    return () => document.removeEventListener("keydown", onKeyDown);
  }, [selected?.id, onSelectWork]);

  // Restore focus to the card that opened the sheet, so keyboard position is
  // never lost on close.
  useEffect(() => {
    if (selected) return;
    const target = returnFocusRef.current;
    returnFocusRef.current = null;
    if (target && document.contains(target)) target.focus();
  }, [selected?.id]);

  const openWork = (workId: string, trigger: HTMLElement | null) => {
    returnFocusRef.current = trigger;
    onSelectWork(workId);
  };

  const create = () => {
    if (!title.trim() || !criteria.trim()) return;
    onAction(createTeamWork(teamRunId, {
      title: title.trim(),
      contextMarkdown: "Created from the Team Works board.",
      completionCriteriaMarkdown: criteria.trim(),
      activeMemberRunId: ownerId || undefined,
      claimMode: ownerId ? "host_assign" : "team_claim",
    }));
    setTitle("");
    setCriteria("");
    setOwnerId("");
    setCreating(false);
  };

  const laneWorksFor = (lane: (typeof WORK_LANES)[number]) => visibleWorks.filter((work) =>
    lane.statuses.includes(workLifecycleLabel(work))
    && (lane.id === "open"
      ? !work.owner_member_id && !work.active_member_run_id
      : lane.id === "assigned"
        ? Boolean(work.owner_member_id || work.active_member_run_id)
        : true),
  );

  const workCard = (work: Work) => {
    const owner = ownerFor(work);
    return (
      <button
        key={work.id}
        type="button"
        data-work-card={work.id}
        onClick={(event) => openWork(work.id, event.currentTarget)}
        className={cn(
          "w-full rounded-lg border bg-card p-2 text-left shadow-[0_12px_26px_-26px_rgba(15,23,42,.75)] transition hover:-translate-y-px hover:border-primary/30 hover:shadow-md sm:p-2.5",
          selectedWorkId === work.id ? "border-primary/40 ring-1 ring-primary/15" : "border-border/75",
        )}
      >
        <div className="flex items-start justify-between gap-2"><Badge tone={workTone(workLifecycleLabel(work))}>{workLifecycleLabel(work).replace(/_/g, " ")}</Badge><span className="text-[9px] uppercase tracking-wider text-muted-foreground">{work.priority}</span></div>
        {/* The title is the card's primary fact; a two-line clamp hid the rest
            of long Host-written titles behind an ellipsis, which read as an
            incomplete card. The full title wraps and long tokens break. */}
        <h3 className="mt-1.5 break-words text-[12px] font-semibold leading-snug text-foreground">{work.title}</h3>
        <p className="mt-1 line-clamp-1 text-[10px] leading-relaxed text-muted-foreground sm:line-clamp-2">{work.completion_criteria_markdown || "No completion criteria"}</p>
        <div className="mt-1.5 flex items-center gap-1.5 border-t border-border/55 pt-1.5">{owner ? <><Avatar name={owner.name ?? owner.id} tone={memberTone(owner.status)} /><span className="min-w-0 flex-1 break-words text-[10px] leading-snug text-foreground" title={owner.name ?? owner.id}>{owner.name ?? owner.id}</span></> : <><span className="grid size-6 shrink-0 place-items-center rounded-full border border-dashed border-border text-muted-foreground"><Users className="size-3" /></span><span className="text-[10px] text-muted-foreground">Unassigned</span></>}<span className="ml-auto shrink-0 font-mono text-[9px] text-muted-foreground">v{work.version}</span></div>
      </button>
    );
  };

  return (
    <section className="py-2" aria-label="Shared team Works board" data-testid="team-works-board">
      <header className="mb-2 flex flex-wrap items-center justify-between gap-x-2 gap-y-1">
        <div className="min-w-0">
          <div className="flex items-center gap-2"><ListTodo className="size-4 text-primary" /><h2 className="text-sm font-semibold text-foreground">Shared Works</h2><Badge tone="info" title="Works not done or cancelled; the filter line below counts all Works including done ones">{active} active</Badge></div>
          <p className="mt-1 hidden text-[11px] text-muted-foreground sm:block">Durable ownership lives here. Messages discuss Work; they never create ownership.</p>
        </div>
        <div className="flex min-w-0 items-center gap-2">
          {/* The filter toggle shares the header row on mobile: as its own band
              it pushed the first Work card past the 320px first viewport. */}
          <Button
            size="sm"
            variant="secondary"
            className="min-h-11 sm:hidden"
            aria-expanded={filtersOpen}
            aria-controls="team-works-filters"
            onClick={() => setFiltersOpen((value) => !value)}
          >
            <ListFilter className="size-3.5" /> Filter
          </Button>
          <span className="min-w-0 flex-1 truncate text-[10px] text-muted-foreground sm:hidden" aria-live="polite">
            {visibleWorks.length}/{works.length}
          </span>
          <Button size="sm" className="min-h-11 sm:min-h-0" disabled={!actionsEnabled} onClick={() => setCreating((value) => !value)}>
            <ListTodo className="size-3.5" /> New Work
          </Button>
        </div>
      </header>

      {creating && (
        <div className="mb-3 grid gap-2 rounded-xl border border-primary/20 bg-primary/[0.025] p-3 sm:grid-cols-[1.2fr_1.2fr_.8fr_auto] sm:items-end">
          <label className="space-y-1 text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">Title<input className="mt-1 h-11 w-full rounded-md border border-border bg-background px-2.5 text-[12px] font-normal normal-case tracking-normal text-foreground outline-none focus:border-primary/50 sm:h-9" value={title} onChange={(event) => setTitle(event.target.value)} placeholder="What outcome is needed?" /></label>
          <label className="space-y-1 text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">Completion criteria<input className="mt-1 h-11 w-full rounded-md border border-border bg-background px-2.5 text-[12px] font-normal normal-case tracking-normal text-foreground outline-none focus:border-primary/50 sm:h-9" value={criteria} onChange={(event) => setCriteria(event.target.value)} placeholder="Evidence required for acceptance" /></label>
          <label className="space-y-1 text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">Owner<Select className="mt-1 text-[12px] font-normal normal-case tracking-normal" value={ownerId} onChange={(event) => setOwnerId(event.target.value)}><option value="">Unassigned pool</option>{assignableMembers.map((member) => <option key={member.id} value={member.id}>{member.name ?? member.id}</option>)}</Select></label>
          <div className="flex gap-1"><Button size="sm" className="min-h-11 sm:min-h-0" onClick={create} disabled={!title.trim() || !criteria.trim()}>Create</Button><Button size="sm" variant="secondary" className="min-h-11 sm:min-h-0" onClick={() => setCreating(false)}>Cancel</Button></div>
        </div>
      )}

      <div
        id="team-works-filters"
        className={cn(
          "mb-2 space-y-2 rounded-xl border border-border/65 bg-muted/[0.12] p-2.5",
          !filtersOpen && "hidden sm:block",
        )}
        aria-label="Filter Works board"
      >
        <div className="flex flex-wrap items-center gap-1.5" role="group" aria-label="Filter Works by owner">
          <span className="mr-1 text-[9px] font-semibold uppercase tracking-[0.12em] text-muted-foreground">Owner</span>
          <WorkFilterChip
            active={ownerFilter === "all"}
            count={ownerCount("all")}
            label="All"
            onClick={() => setOwnerFilter("all")}
          />
          <WorkFilterChip
            active={ownerFilter === "unassigned"}
            count={ownerCount("unassigned")}
            label="Unassigned"
            icon={<Users className="size-3" />}
            onClick={() => setOwnerFilter("unassigned")}
          />
          {members.map((member) => {
            const filter: TeamWorksOwnerFilter = `member:${member.id}`;
            return (
              <WorkFilterChip
                key={member.id}
                active={ownerFilter === filter}
                count={ownerCount(filter)}
                label={member.name ?? member.id}
                icon={<Avatar name={member.name ?? member.id} tone={memberTone(member.status)} size="xs" />}
                onClick={() => setOwnerFilter(filter)}
              />
            );
          })}
        </div>
        <div className="flex flex-wrap items-center gap-1.5" role="group" aria-label="Filter Works by attention state">
          <span className="mr-1 text-[9px] font-semibold uppercase tracking-[0.12em] text-muted-foreground">Attention</span>
          <WorkFilterChip active={attentionFilter === "all"} count={attentionCount("all")} label="All statuses" onClick={() => setAttentionFilter("all")} />
          <WorkFilterChip active={attentionFilter === "review"} count={attentionCount("review")} label="Needs review" onClick={() => setAttentionFilter("review")} />
          <WorkFilterChip active={attentionFilter === "blocked"} count={attentionCount("blocked")} label="Blocked" onClick={() => setAttentionFilter("blocked")} />
        </div>
        <p className="hidden text-[10px] leading-relaxed text-muted-foreground sm:block" aria-live="polite">
          Showing {visibleWorks.length} of {works.length} Works. Responsibility comes from Work ownership; activity messages do not assign it.
        </p>
      </div>

      {/* One board. Five desktop columns and the stacked mobile status list are
          the same DOM reflowed, never two renderings of the same Work. */}
      <div className="grid gap-2 pb-2 lg:grid-cols-5" data-testid="team-works-lanes">
        {WORK_LANES.map((lane) => {
          const laneWorks = laneWorksFor(lane);
          return (
            <section
              key={lane.id}
              data-work-lane={lane.id}
              className="rounded-xl border border-border/70 bg-muted/[0.18] p-2"
              aria-label={`${lane.label} Works`}
            >
              <header className="mb-1.5 flex items-center justify-between px-1">
                <span className="flex items-center gap-1.5 text-[10px] font-semibold uppercase tracking-[0.12em] text-muted-foreground"><StatusDot tone={lane.tone} />{lane.label}</span>
                <span className="text-[10px] tabular-nums text-muted-foreground">{laneWorks.length}</span>
              </header>
              <div className="space-y-2 sm:grid sm:grid-cols-2 sm:gap-2 sm:space-y-0 lg:block lg:space-y-2">
                {laneWorks.map(workCard)}
                {laneWorks.length === 0 && (
                  <div className="hidden min-h-20 place-items-center rounded-lg border border-dashed border-border/75 px-2 text-center text-[10px] text-muted-foreground sm:grid sm:col-span-2 lg:col-span-1">No Works</div>
                )}
                {/* A single card in a two-column lane left an unexplained blank
                    half. The slot is stated rather than left ambiguous, and it
                    exists only in the 640-1023px two-column regime. */}
                {laneWorks.length % 2 === 1 && (
                  <div
                    data-work-empty-slot={lane.id}
                    aria-hidden="true"
                    className="hidden min-h-20 place-items-center rounded-lg border border-dashed border-border/60 px-2 text-center text-[10px] text-muted-foreground/70 sm:grid lg:hidden"
                  >
                    No further {lane.label.toLowerCase()} Work
                  </div>
                )}
              </div>
            </section>
          );
        })}
      </div>

      {selected && (
        <div className="fixed inset-0 z-50 bg-foreground/15 backdrop-blur-[1px]" onMouseDown={(event) => { if (event.target === event.currentTarget) onSelectWork(undefined); }}>
          <aside
            ref={dialogRef}
            role="dialog"
            aria-modal="true"
            aria-labelledby="selected-work-title"
            data-testid="work-detail-sheet"
            className="absolute inset-x-0 bottom-0 max-h-[86vh] overflow-y-auto rounded-t-2xl border border-border bg-background p-4 shadow-2xl lg:inset-y-0 lg:left-auto lg:right-0 lg:max-h-none lg:w-[34rem] lg:rounded-none lg:border-y-0 lg:border-r-0 lg:p-5"
          >
            <div className="mx-auto mb-3 h-1 w-10 rounded-full bg-border lg:hidden" />
            <div className="flex items-start justify-between gap-3">
              <div className="min-w-0">
                <div className="flex flex-wrap items-center gap-2"><Badge tone={workTone(workLifecycleLabel(selected))}>{workLifecycleLabel(selected).replace(/_/g, " ")}</Badge><span className="font-mono text-[9px] text-muted-foreground">{selected.id}</span></div>
                <h3 id="selected-work-title" className="mt-2 text-lg font-semibold text-foreground">{selected.title}</h3>
              </div>
              <button ref={closeButtonRef} type="button" className="grid size-11 shrink-0 place-items-center rounded-md text-muted-foreground hover:bg-accent hover:text-foreground sm:size-8" onClick={() => onSelectWork(undefined)} aria-label="Close Work details"><X className="size-4" /></button>
            </div>

            <div className="mt-4 space-y-4">
              <section><p className="mb-1 text-[9px] font-semibold uppercase tracking-wider text-muted-foreground">Context</p>{selected.context_markdown ? <Markdown source={selected.context_markdown} compact /> : <p className="text-[11px] text-muted-foreground">No context recorded.</p>}</section>
              <section><p className="mb-1 text-[9px] font-semibold uppercase tracking-wider text-muted-foreground">Completion criteria</p><Markdown source={selected.completion_criteria_markdown || "Not declared"} compact /></section>
              {selected.blocker_reason && <section className="rounded-lg border border-status-bad/25 bg-status-bad/[0.045] p-3"><p className="mb-1 text-[9px] font-semibold uppercase tracking-wider text-status-bad">Blocker</p><Markdown source={selected.blocker_reason} compact /></section>}
              {selected.result_summary && <section><p className="mb-1 text-[9px] font-semibold uppercase tracking-wider text-muted-foreground">Result</p><Markdown source={selected.result_summary} compact /></section>}

              <div className="grid grid-cols-2 gap-2 text-[10px] sm:grid-cols-3">
                <WorkFact label="Owner" value={ownerFor(selected)?.name ?? "Unassigned"} />
                <WorkFact label="Claim mode" value={selected.claim_mode} />
                <WorkFact label="Priority" value={selected.priority} />
                <WorkFact label="Parent" value={selected.parent_work_id ? shortId(selected.parent_work_id) : "None"} />
                <WorkFact label="Prerequisites" value={selected.prerequisite_work_ids?.length ? String(selected.prerequisite_work_ids.length) : "None"} />
                <WorkFact label="Artifacts" value={String(selected.artifact_refs?.length ?? 0)} />
                <WorkFact label="Checks" value={String(selected.check_refs?.length ?? 0)} />
                <WorkFact label="Version" value={`v${selected.version}`} />
              </div>

              {(selected.prerequisite_work_ids?.length ?? 0) > 0 && (
                <section><p className="mb-1 text-[9px] font-semibold uppercase tracking-wider text-muted-foreground">Prerequisite Works</p><div className="flex flex-wrap gap-1.5">{selected.prerequisite_work_ids?.map((id) => <button type="button" key={id} onClick={() => onSelectWork(id)} className="rounded-md border border-border px-2 py-1 font-mono text-[9px] text-primary hover:bg-accent">{shortId(id)}</button>)}</div></section>
              )}

              <section className="space-y-2 rounded-xl border border-border/70 bg-muted/[0.16] p-3">
                <p className="text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">Lead controls</p>
                {selected.phase === "open" && selected.condition === "normal" && !selected.owner_member_id && !selected.active_member_run_id && <label className="block text-[10px] text-muted-foreground">Assign owner<Select className="mt-1" value="" disabled={!actionsEnabled || assignableMembers.length === 0} onChange={(event) => { if (event.target.value) onAction(assignTeamWork(teamRunId, selected.id, event.target.value, selected.version)); }}><option value="">{assignableMembers.length ? "Choose member…" : "No active members available"}</option>{assignableMembers.map((member) => <option key={member.id} value={member.id}>{member.name ?? member.id}</option>)}</Select></label>}
                {selected.phase === "review" && <><TextArea value={reviewNote} onChange={(event) => setReviewNote(event.target.value)} placeholder="Optional review note" className="min-h-16" /><div className="flex flex-wrap gap-2"><Button size="sm" className="min-h-11 sm:min-h-0" disabled={!actionsEnabled} onClick={() => onAction(reviewTeamWork(teamRunId, selected.id, selected.version, "accept", reviewNote))}><CheckCircle2 className="size-3.5" />Accept</Button><Button size="sm" variant="secondary" className="min-h-11 sm:min-h-0" disabled={!actionsEnabled || !reviewNote.trim()} onClick={() => onAction(reviewTeamWork(teamRunId, selected.id, selected.version, "request-changes", reviewNote))}>Request changes</Button></div></>}
                <div className="flex flex-wrap gap-2">
                  <Button size="sm" variant="secondary" className="min-h-11 sm:min-h-0" onClick={() => onDiscuss(selected)}><MessageSquare className="size-3.5" /> Discuss Work</Button>
                  {ownerFor(selected) && <Button size="sm" variant="secondary" className="min-h-11 sm:min-h-0" onClick={() => onOpenMember(ownerFor(selected)!)}>Open member</Button>}
                  {!workIsTerminal(selected) && <Button size="sm" variant="secondary" className="min-h-11 sm:min-h-0" disabled={!actionsEnabled} onClick={() => onAction({ method: "POST", path: `/v1/team-runs/${encodeURIComponent(teamRunId)}/works/${encodeURIComponent(selected.id)}/cancel`, body: { expected_version: selected.version, reason: "Cancelled by Host" } })}>Cancel Work</Button>}
                </div>
              </section>

              <WorkRecordHistory
                events={workEvents.filter((event) => event.work_id === selected.id)}
                deliveries={workDeliveries.filter((delivery) => delivery.work_id === selected.id)}
              />
            </div>
          </aside>
        </div>
      )}
    </section>
  );
}

function WorkFilterChip({
  active,
  count,
  label,
  icon,
  onClick,
}: {
  active: boolean;
  count: number;
  label: string;
  icon?: ReactNode;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      aria-pressed={active}
      onClick={onClick}
      className={cn(
        "flex max-w-full items-center gap-1.5 rounded-full border px-2 py-1 text-[10px] font-medium transition-colors",
        active
          ? "border-primary/35 bg-primary/[0.08] text-primary"
          : "border-border/70 bg-background text-muted-foreground hover:border-primary/25 hover:text-foreground",
      )}
    >
      {icon}
      <span className="max-w-36 truncate" title={label}>{label}</span>
      <span className="tabular-nums opacity-70">{count}</span>
    </button>
  );
}

function WorkFact({ label, value }: { label: string; value: string }) {
  return <div className="rounded-md bg-muted/45 px-2 py-1.5"><span className="block text-[8px] uppercase tracking-wider text-muted-foreground">{label}</span><span className="mt-0.5 block truncate text-foreground">{value}</span></div>;
}

function WorkRecordHistory({ events, deliveries }: { events: WorkEvent[]; deliveries: WorkDelivery[] }) {
  const orderedEvents = [...events].sort((left, right) => left.sequence - right.sequence);
  return (
    <section>
      <div className="mb-2 flex items-center justify-between gap-2">
        <p className="text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">Durable history</p>
        <span className="text-[9px] text-muted-foreground">{events.length} events · {deliveries.length} deliveries</span>
      </div>
      {orderedEvents.length === 0 && deliveries.length === 0 ? (
        <p className="rounded-lg border border-dashed border-border px-3 py-3 text-[10px] text-muted-foreground">No Work events or deliveries are present in this snapshot.</p>
      ) : (
        <div className="divide-y divide-border/60 overflow-hidden rounded-lg border border-border/70 bg-card">
          {orderedEvents.slice(-8).map((event) => (
            <div key={event.id} className="flex items-start gap-2 px-3 py-2 text-[10px]">
              <StatusDot tone={event.kind.includes("block") || event.kind.includes("cancel") ? "bad" : event.kind.includes("accept") || event.kind.includes("complete") ? "good" : "info"} />
              <div className="min-w-0 flex-1"><p className="font-medium text-foreground">{event.kind.replace(/_/g, " ")}</p><p className="mt-0.5 text-muted-foreground">sequence {event.sequence} · v{event.expected_version} → v{event.resulting_version} · {formatTime(event.created_at)}</p></div>
            </div>
          ))}
          {deliveries.slice(-6).map((delivery) => (
            <div key={delivery.id} className="flex items-start gap-2 px-3 py-2 text-[10px]">
              <SendHorizontal className="mt-0.5 size-3 shrink-0 text-primary" />
              <div className="min-w-0 flex-1"><p className="font-medium text-foreground">Delivery · {delivery.status.replace(/_/g, " ")}</p><p className="mt-0.5 truncate text-muted-foreground">to {shortId(delivery.recipient_member_run_id)} · Work v{delivery.work_version} · attempt {delivery.attempt}</p></div>
            </div>
          ))}
        </div>
      )}
    </section>
  );
}
