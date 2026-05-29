import { useState } from "react";
import { fetchSnapshot } from "./api";
import type { DashboardSnapshot } from "./types";

const apiDefault = "http://127.0.0.1:8787";

export function App() {
  const [apiUrl, setApiUrl] = useState(apiDefault);
  const [snapshot, setSnapshot] = useState<DashboardSnapshot | null>(null);
  const [status, setStatus] = useState("Rebuild paused until page specs and hard layout specs are accepted.");
  const [isLoading, setIsLoading] = useState(false);

  async function loadSnapshot() {
    setIsLoading(true);
    try {
      const next = await fetchSnapshot(apiUrl);
      setSnapshot(next);
      setStatus("Snapshot loaded for architecture/read-model inspection only.");
    } catch (error) {
      setStatus(error instanceof Error ? error.message : String(error));
    } finally {
      setIsLoading(false);
    }
  }

  const counts = snapshot
    ? [
        ["goals", snapshot.goals?.length ?? 0],
        ["teams", snapshot.teams?.length ?? 0],
        ["members", snapshot.members?.length ?? 0],
        ["tasks", snapshot.tasks?.length ?? 0],
        ["messages", snapshot.messages?.length ?? 0],
      ]
    : [];

  return (
    <main className="rebuildShell">
      <section className="rebuildPanel" aria-labelledby="rebuild-title">
        <p className="eyebrow">Agent Workbench</p>
        <h1 id="rebuild-title">Frontend rebuild is intentionally reset</h1>
        <p className="summary">
          The rejected PR #6 Workbench shell and old dashboard component tree are
          no longer the implementation base. The next UI must start from page
          specs, architecture decision, hard layout specs, and screenshot-first
          acceptance.
        </p>

        <div className="sourceRow">
          <label htmlFor="api-url">Harness API</label>
          <input
            id="api-url"
            value={apiUrl}
            onChange={(event) => setApiUrl(event.target.value)}
            spellCheck={false}
          />
          <button type="button" onClick={loadSnapshot} disabled={isLoading}>
            {isLoading ? "Loading" : "Load"}
          </button>
        </div>

        <p className="status" role="status">
          {status}
        </p>

        {counts.length > 0 && (
          <dl className="counts" aria-label="Loaded snapshot counts">
            {counts.map(([label, value]) => (
              <div key={label}>
                <dt>{label}</dt>
                <dd>{value}</dd>
              </div>
            ))}
          </dl>
        )}

        <nav className="docLinks" aria-label="Required design documents">
          <a href="../../docs/dashboard/pages/README.md">Page specs</a>
          <a href="../../docs/dashboard/hard-layout-specs/shell-v2.md">Shell v2 spec</a>
          <a href="../../docs/dashboard/frontend-architecture.md">Architecture</a>
          <a href="../../docs/dashboard/acceptance.md">Acceptance</a>
          <a href="../../docs/dashboard/rejected-implementations/pr-6-agent-workbench-shell.md">
            Rejected PR #6
          </a>
        </nav>
      </section>
    </main>
  );
}
