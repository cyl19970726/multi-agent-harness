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

export function App() {
  const [apiUrl, setApiUrl] = useState(apiDefault);
  const [snapshot, setSnapshot] = useState<DashboardSnapshot>(demoSnapshot);
  const [sourceLabel, setSourceLabel] = useState("offline design fixture");
  const [sourceError, setSourceError] = useState<string | null>(null);
  const [isLoading, setIsLoading] = useState(false);
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
      />
    </TooltipProvider>
  );
}
