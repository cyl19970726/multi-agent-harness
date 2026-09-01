import { ChevronLeft, ChevronRight, Maximize2, MessageSquare, Minimize2, X } from "lucide-react";
import { useEffect, useRef } from "react";

import { Button } from "@/components/ui/button";
import type { AgentWorkspaceRosterItem, AllowedAction, MessageSummary, WorkSummary } from "@/model/roleViews";
import { MessagesDock } from "./MessagesDock";
import type { AgentDockState, DockModuleStatus } from "./types";
import { clampAgentDockWidth } from "./types";
import { WorkDock } from "./WorkDock";
import "./dock.css";

export function DockShell({
  state,
  onStateChange,
  displayMode = "inline",
  works,
  messages,
  roster,
  selectedAgentId,
  currentWorkId,
  allowedActions = [],
  workStatus,
  messagesStatus,
  initialWorkId,
  initialMessageId,
  openerRef,
  renderAuthorizedActions,
  onSelectWork,
  onSelectMessage,
}: {
  state: AgentDockState;
  onStateChange: (state: AgentDockState) => void;
  displayMode?: "inline" | "overlay";
  works: WorkSummary[];
  messages: MessageSummary[];
  roster: AgentWorkspaceRosterItem[];
  selectedAgentId: string;
  currentWorkId: string | null;
  allowedActions?: AllowedAction[];
  workStatus?: DockModuleStatus;
  messagesStatus?: DockModuleStatus;
  initialWorkId?: string;
  initialMessageId?: string;
  openerRef?: React.RefObject<HTMLElement>;
  renderAuthorizedActions?: (actions: AllowedAction[], work: WorkSummary) => React.ReactNode;
  onSelectWork?: (work: WorkSummary) => void;
  onSelectMessage?: (message: MessageSummary) => void;
}) {
  const shellRef = useRef<HTMLElement>(null);
  const previousOpen = useRef(state.open);
  const expanded = state.width >= 520;
  const set = (patch: Partial<AgentDockState>) => onStateChange({ ...state, ...patch, ...(patch.width == null ? {} : { width: clampAgentDockWidth(patch.width) }) });

  useEffect(() => {
    if (previousOpen.current && !state.open) openerRef?.current?.focus();
    previousOpen.current = state.open;
  }, [state.open, openerRef]);
  useEffect(() => {
    if (!state.open || displayMode !== "overlay") return;
    const shell = shellRef.current;
    shell?.querySelector<HTMLElement>("button")?.focus();
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") { event.preventDefault(); set({ open: false }); return; }
      if (event.key !== "Tab") return;
      const focusable = [...(shell?.querySelectorAll<HTMLElement>('button:not([disabled]), [href], input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])') ?? [])];
      if (!focusable.length) return;
      const first = focusable[0]!, last = focusable[focusable.length - 1]!;
      if (event.shiftKey && document.activeElement === first) { event.preventDefault(); last.focus(); }
      else if (!event.shiftKey && document.activeElement === last) { event.preventDefault(); first.focus(); }
    };
    document.addEventListener("keydown", onKeyDown);
    return () => document.removeEventListener("keydown", onKeyDown);
  }, [state.open, displayMode, state]);

  if (!state.open) return null;
  const startResize = (event: React.PointerEvent<HTMLDivElement>) => {
    if (displayMode === "overlay") return;
    event.currentTarget.setPointerCapture(event.pointerId);
    const startX = event.clientX;
    const startWidth = state.width;
    const target = event.currentTarget;
    const move = (moveEvent: PointerEvent) => set({ width: startWidth + startX - moveEvent.clientX });
    const done = () => { target.removeEventListener("pointermove", move); target.removeEventListener("pointerup", done); target.removeEventListener("pointercancel", done); };
    target.addEventListener("pointermove", move);
    target.addEventListener("pointerup", done);
    target.addEventListener("pointercancel", done);
  };
  const resizeByKey = (event: React.KeyboardEvent<HTMLDivElement>) => {
    if (event.key !== "ArrowLeft" && event.key !== "ArrowRight") return;
    event.preventDefault();
    set({ width: state.width + (event.key === "ArrowLeft" ? 24 : -24) });
  };
  const bodyProps = { works, messages, roster, selectedAgentId, expanded };
  return <>
    {displayMode === "overlay" && <button type="button" className="agent-dock-backdrop" aria-label="Close Work and Messages dock" onClick={() => set({ open: false })}/>}
    <aside ref={shellRef} className="agent-dock-shell" data-display={displayMode} data-expanded={expanded || undefined} style={{ "--agent-dock-width": `${state.width}px` } as React.CSSProperties} aria-label="Work and Messages dock">
      {displayMode === "inline" && (
        <div className="agent-dock-resize" role="separator" aria-label="Resize Work and Messages dock" aria-orientation="vertical" aria-valuemin={320} aria-valuemax={640} aria-valuenow={state.width} tabIndex={0} onPointerDown={startResize} onKeyDown={resizeByKey}/>
      )}
      <header className="agent-dock-header">
        <div role="tablist" aria-label="Dock module">
          <button type="button" role="tab" aria-selected={state.module === "work"} onClick={() => set({ module: "work" })}>Work <span>{works.length}</span></button>
          <button type="button" role="tab" aria-selected={state.module === "messages"} onClick={() => set({ module: "messages" })}><MessageSquare aria-hidden="true"/>Messages <span>{messages.length}</span></button>
        </div>
        <Button size="icon" variant="ghost" aria-label={expanded ? "Use compact dock" : "Expand dock"} onClick={() => set({ width: expanded ? 360 : 560 })}>{expanded ? <Minimize2/> : <Maximize2/>}</Button>
        <Button size="icon" variant="ghost" aria-label="Close Work and Messages dock" onClick={() => set({ open: false })}><X/></Button>
      </header>
      <div className="agent-dock-module" role="tabpanel" hidden={state.module !== "work"} aria-label="Work">
        <WorkDock {...bodyProps} currentWorkId={currentWorkId} initialWorkId={initialWorkId} status={workStatus} allowedActions={allowedActions} renderAuthorizedActions={renderAuthorizedActions} onSelectWork={onSelectWork} onOpenMessages={() => set({ module: "messages" })}/>
      </div>
      <div className="agent-dock-module" role="tabpanel" hidden={state.module !== "messages"} aria-label="Messages">
        <MessagesDock {...bodyProps} initialMessageId={initialMessageId} status={messagesStatus} onSelectMessage={onSelectMessage}/>
      </div>
      {displayMode === "overlay" && <footer className="agent-dock-overlay-footer"><button type="button" onClick={() => set({ module: state.module === "work" ? "messages" : "work" })}>{state.module === "work" ? <><ChevronLeft/>Messages</> : <>Work<ChevronRight/></>}</button></footer>}
    </aside>
  </>;
}
