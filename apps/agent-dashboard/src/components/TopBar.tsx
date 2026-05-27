import { FileJson, Link2, PauseCircle, PlayCircle } from "lucide-react";
import type { ChangeEvent } from "react";

interface TopBarProps {
  generatedAt: string;
  liveUrl: string;
  isLive: boolean;
  onLiveUrlChange: (value: string) => void;
  onStartLive: () => void;
  onStopLive: () => void;
  onPaste: () => void;
  onFile: (text: string) => void;
}

export function TopBar({
  generatedAt,
  liveUrl,
  isLive,
  onLiveUrlChange,
  onStartLive,
  onStopLive,
  onPaste,
  onFile,
}: TopBarProps) {
  async function handleFile(event: ChangeEvent<HTMLInputElement>) {
    const file = event.target.files?.[0];
    if (!file) return;
    onFile(await file.text());
  }

  return (
    <header className="topbar">
      <div>
        <h1>Agent Dashboard</h1>
        <p>{isLive ? "Live polling" : "Snapshot"} · Generated: {generatedAt || "-"}</p>
      </div>
      <div className="actions">
        <label className="urlShell">
          <Link2 size={15} />
          <input
            className="urlInput"
            type="url"
            value={liveUrl}
            aria-label="Harness API URL"
            onChange={(event) => onLiveUrlChange(event.target.value)}
          />
        </label>
        <button type="button" onClick={onStartLive} title="Load live snapshot">
          <PlayCircle size={15} />
          Live
        </button>
        <button type="button" onClick={onStopLive} title="Stop live polling">
          <PauseCircle size={15} />
          Stop
        </button>
        <label className="fileButton" title="Load snapshot JSON">
          <FileJson size={15} />
          Snapshot
          <input type="file" accept="application/json,.json" onChange={handleFile} />
        </label>
        <button type="button" onClick={onPaste} title="Use pasted JSON">
          <FileJson size={15} />
          Paste
        </button>
      </div>
    </header>
  );
}
