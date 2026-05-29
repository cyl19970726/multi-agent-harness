import { useMemo, useState } from "react";
import { fetchSnapshot } from "../api";
import { demoSnapshot } from "../model/demoSnapshot";
import { buildWorkbenchModel } from "../model/readModel";
import type { DashboardSnapshot } from "../types";
import { defaultSelection, type SelectionState } from "./selection";
import { WorkbenchShell } from "./WorkbenchShell";

const apiDefault = "http://127.0.0.1:8787";

export function App() {
  const [apiUrl, setApiUrl] = useState(apiDefault);
  const [snapshot, setSnapshot] = useState<DashboardSnapshot>(demoSnapshot);
  const [sourceLabel, setSourceLabel] = useState("offline design fixture");
  const [sourceError, setSourceError] = useState<string | null>(null);
  const [isLoading, setIsLoading] = useState(false);
  const [selection, setSelection] = useState<SelectionState>(defaultSelection);

  const model = useMemo(() => buildWorkbenchModel(snapshot, selection), [snapshot, selection]);

  async function refreshLive() {
    setIsLoading(true);
    setSourceError(null);
    try {
      const next = await fetchSnapshot(apiUrl);
      setSnapshot(next);
      setSourceLabel("live /v1/snapshot");
    } catch (error) {
      setSourceError(error instanceof Error ? error.message : String(error));
      setSourceLabel("offline design fixture");
      setSnapshot(demoSnapshot);
    } finally {
      setIsLoading(false);
    }
  }

  return (
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
    />
  );
}
