import { useEffect, useMemo, useState } from "react";
import { fetchSnapshot } from "./api";
import { ControlPlane } from "./components/ControlPlane";
import { RawViews } from "./components/RawViews";
import { SummaryGrid } from "./components/SummaryGrid";
import { TopBar } from "./components/TopBar";
import { deriveWarnings, normalizeSnapshot } from "./readModel";
import type { DashboardSnapshot } from "./types";

export function App() {
  const [snapshot, setSnapshot] = useState<DashboardSnapshot | null>(null);
  const [jsonInput, setJsonInput] = useState("");
  const [liveUrl, setLiveUrl] = useState("http://127.0.0.1:8787");
  const [isLive, setIsLive] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [selectedGoalId, setSelectedGoalId] = useState<string | undefined>();
  const [selectedTaskId, setSelectedTaskId] = useState<string | undefined>();
  const [selectedMemberId, setSelectedMemberId] = useState<string | undefined>();

  const view = useMemo(() => normalizeSnapshot(snapshot), [snapshot]);
  const warnings = useMemo(() => deriveWarnings(view), [view]);

  useEffect(() => {
    if (!isLive) return;
    let cancelled = false;
    async function load() {
      try {
        const next = await fetchSnapshot(liveUrl);
        if (!cancelled) {
          setSnapshot(next);
          setError(null);
        }
      } catch (loadError) {
        if (!cancelled) setError(loadError instanceof Error ? loadError.message : String(loadError));
      }
    }
    load();
    const timer = window.setInterval(load, 5000);
    return () => {
      cancelled = true;
      window.clearInterval(timer);
    };
  }, [isLive, liveUrl]);

  function loadJson(raw: string) {
    try {
      setSnapshot(JSON.parse(raw) as DashboardSnapshot);
      setError(null);
      setIsLive(false);
    } catch (parseError) {
      setError(parseError instanceof Error ? parseError.message : String(parseError));
    }
  }

  const failed = view.messages.filter((message) => message.delivery_status === "failed").length +
    view.provider_sessions.filter((session) => session.status === "failed").length;

  return (
    <>
      <TopBar
        generatedAt={view.generated_at}
        liveUrl={liveUrl}
        isLive={isLive}
        onLiveUrlChange={setLiveUrl}
        onStartLive={() => setIsLive(true)}
        onStopLive={() => setIsLive(false)}
        onPaste={() => loadJson(jsonInput)}
        onFile={loadJson}
      />
      <main className="page">
        <section className="inputBand">
          <textarea
            spellCheck={false}
            value={jsonInput}
            onChange={(event) => setJsonInput(event.target.value)}
            placeholder="Paste output from: harness dashboard snapshot"
          />
          {error && <div className="loadError">{error}</div>}
        </section>

        <SummaryGrid
          items={[
            { label: "Tasks", value: view.tasks.length },
            { label: "Teams", value: view.teams.length },
            { label: "Members", value: view.members.length },
            { label: "Queued", value: view.messages.filter((message) => message.delivery_status === "queued").length, tone: "warn" },
            { label: "Failed", value: failed, tone: failed ? "bad" : "normal" },
            { label: "Sessions", value: view.provider_sessions.length },
            { label: "Decisions", value: view.decisions.length },
            { label: "Warnings", value: warnings.length, tone: warnings.length ? "warn" : "normal" },
          ]}
        />

        <ControlPlane
          snapshot={view}
          warnings={warnings}
          selectedGoalId={selectedGoalId}
          selectedTaskId={selectedTaskId}
          selectedMemberId={selectedMemberId}
          onSelectGoal={setSelectedGoalId}
          onSelectTask={setSelectedTaskId}
          onSelectMember={setSelectedMemberId}
        />

        <RawViews snapshot={view} />
      </main>
    </>
  );
}
