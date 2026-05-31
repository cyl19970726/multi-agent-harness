import { useEffect, useMemo, useState } from "react";
import { fetchSnapshot, postAction } from "../api";
import { demoSnapshot } from "../model/demoSnapshot";
import { buildWorkbenchModel } from "../model/readModel";
import type { DashboardSnapshot } from "../types";
import { TooltipProvider } from "@/components/ui/tooltip";
import {
  defaultSelection,
  selectionFromLocation,
  syncSelectionToLocation,
  type SelectionState,
} from "./selection";
import { WorkbenchShell } from "./WorkbenchShell";

const apiDefault = "http://127.0.0.1:8787";
const liveSourceLabel = "live /v1/snapshot";
/** Live-poll cadence: re-fetch /v1/snapshot roughly every 5s while enabled. */
const pollIntervalMs = 5000;

export function App() {
  const [apiUrl, setApiUrl] = useState(apiDefault);
  const [snapshot, setSnapshot] = useState<DashboardSnapshot>(demoSnapshot);
  const [sourceLabel, setSourceLabel] = useState("offline design fixture");
  const [sourceError, setSourceError] = useState<string | null>(null);
  const [isLoading, setIsLoading] = useState(false);
  // Opt-in live polling: off by default so the page is a one-shot read unless
  // the operator explicitly asks for a refreshing view.
  const [pollEnabled, setPollEnabled] = useState(false);
  // Seed selection from the URL so a member view (?surface=member&member=:id,
  // i.e. the /members/:memberId workbench) is directly addressable and
  // deep-linkable without pulling in a router.
  const [selection, setSelection] = useState<SelectionState>(() => selectionFromLocation(defaultSelection));

  // Keep the URL in sync with the selected surface/member so the address bar is
  // shareable, and honour Back/Forward navigation.
  useEffect(() => {
    syncSelectionToLocation(selection);
  }, [selection]);

  useEffect(() => {
    const onPopState = () => setSelection((current) => selectionFromLocation(current));
    window.addEventListener("popstate", onPopState);
    return () => window.removeEventListener("popstate", onPopState);
  }, []);

  const model = useMemo(() => buildWorkbenchModel(snapshot, selection), [snapshot, selection]);

  // Actions are only honest against a live snapshot; the offline fixture is read-only.
  const isLive = sourceLabel === liveSourceLabel;

  async function refreshLive() {
    setIsLoading(true);
    setSourceError(null);
    try {
      const next = await fetchSnapshot(apiUrl);
      setSnapshot(next);
      setSourceLabel(liveSourceLabel);
    } catch (error) {
      setSourceError(error instanceof Error ? error.message : String(error));
      setSourceLabel("offline design fixture");
      setSnapshot(demoSnapshot);
    } finally {
      setIsLoading(false);
    }
  }

  // Opt-in interval poll. Only runs while polling is enabled AND the current
  // source is live (polling the offline fixture is meaningless). A failed poll
  // surfaces the error but keeps the last good snapshot — it does not tear the
  // view down to the demo fixture the way a manual "Load live" failure does.
  // The interval is cleared on unmount, on toggle-off, and whenever apiUrl
  // changes so we never poll a stale endpoint.
  useEffect(() => {
    if (!pollEnabled || !isLive) return;
    let cancelled = false;
    const id = window.setInterval(() => {
      void (async () => {
        try {
          const next = await fetchSnapshot(apiUrl);
          if (!cancelled) {
            setSnapshot(next);
            setSourceError(null);
          }
        } catch (error) {
          if (!cancelled) {
            setSourceError(error instanceof Error ? error.message : String(error));
          }
        }
      })();
    }, pollIntervalMs);
    return () => {
      cancelled = true;
      window.clearInterval(id);
    };
  }, [pollEnabled, isLive, apiUrl]);

  async function runAction(path: string, body?: unknown) {
    if (!isLive) return;
    setSourceError(null);
    try {
      const response = await postAction(apiUrl, path, body);
      if (response.snapshot) {
        setSnapshot(response.snapshot);
      } else {
        await refreshLive();
      }
    } catch (error) {
      setSourceError(error instanceof Error ? error.message : String(error));
    }
  }

  return (
    <TooltipProvider delayDuration={200}>
      <WorkbenchShell
        apiUrl={apiUrl}
        isLoading={isLoading}
        model={model}
        onApiUrlChange={setApiUrl}
        onRefresh={refreshLive}
        onSelectionChange={setSelection}
        selection={selection}
        sourceError={sourceError}
        sourceLabel={sourceLabel}
        actionsEnabled={isLive}
        onAction={(path, body) => void runAction(path, body)}
        pollEnabled={pollEnabled}
        canPoll={isLive}
        onTogglePoll={() => setPollEnabled((on) => !on)}
      />
    </TooltipProvider>
  );
}
