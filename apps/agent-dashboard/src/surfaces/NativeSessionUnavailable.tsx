const REASONS: Record<string, string> = {
  exact_session_unavailable: "This view cannot currently locate the exact native Session. This does not establish whether its saved history exists.",
  node_daemon_read_unavailable: "The node daemon cannot currently provide native Session history. Coordination messages below are not the full execution transcript.",
};

export function NativeSessionUnavailable({reason, detail}:{reason:string; detail?:string|null}) {
  return <div role="status" className="my-3 rounded-md border border-border px-3 py-2 text-xs leading-relaxed text-muted-foreground">
    <p>{REASONS[reason] ?? "Native Session history is unavailable in this view. Coordination messages are not the full execution transcript."}</p>
    <details className="mt-1 text-[10px]"><summary className="cursor-pointer">Read diagnostics</summary><code>{reason}</code>{detail&&<p>{detail}</p>}</details>
  </div>;
}
