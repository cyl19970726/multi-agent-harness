import { useMemo, useState } from "react";
import { AlertTriangle, BriefcaseBusiness, CheckCircle2, CirclePause, Search } from "lucide-react";

type JsonRecord = Record<string, unknown>;

function record(value: unknown): JsonRecord {
  return value && typeof value === "object" && !Array.isArray(value) ? value as JsonRecord : {};
}

function records(value: unknown): JsonRecord[] {
  return Array.isArray(value) ? value.map(record) : [];
}

function text(value: unknown, fallback = ""): string {
  return typeof value === "string" && value.trim() ? value : fallback;
}

function number(value: unknown): number {
  return typeof value === "number" && Number.isFinite(value) ? value : 0;
}

function lifecycleLabel(work: JsonRecord): string {
  const phase = text(work.phase, "open");
  const condition = text(work.condition, "normal");
  const resolution = text(work.resolution);
  if (phase === "closed" && resolution) return `${phase} · ${resolution}`;
  return condition === "normal" ? phase : `${phase} · ${condition.replace("_", " ")}`;
}

function lifecycleTone(work: JsonRecord): string {
  if (text(work.condition) === "blocked") return "border-destructive/30 bg-destructive/[0.04] text-destructive";
  if (text(work.condition) === "on_hold") return "border-amber-500/30 bg-amber-500/[0.05] text-amber-700 dark:text-amber-300";
  if (text(work.phase) === "closed" && text(work.resolution) === "accepted") return "border-emerald-500/30 bg-emerald-500/[0.05] text-emerald-700 dark:text-emerald-300";
  return "border-border bg-muted/35 text-muted-foreground";
}

export function WorkOperatingPage({ source }: { source: unknown }) {
  const root = record(source);
  const company = record(root.company_os);
  const companyRoot = Object.keys(company).length ? company : root;
  const aggregate = record(companyRoot.work);
  const hasAggregateWorks = Object.prototype.hasOwnProperty.call(aggregate, "works");
  const works = hasAggregateWorks ? records(aggregate.works) : records(companyRoot.works);
  const summary = record(aggregate.summary);
  const [query, setQuery] = useState("");
  const [phase, setPhase] = useState("all");
  const [condition, setCondition] = useState("all");

  const filtered = useMemo(() => works.filter((work) => {
    const searchable = [work.id, work.title, work.team_id, work.team_run_id, work.owner_member_id]
      .map((value) => text(value).toLowerCase()).join(" ");
    return (!query.trim() || searchable.includes(query.trim().toLowerCase()))
      && (phase === "all" || text(work.phase) === phase)
      && (condition === "all" || text(work.condition) === condition);
  }), [works, query, phase, condition]);

  const counts = {
    total: number(summary.total) || works.length,
    active: number(summary.active) || works.filter((work) => text(work.phase) === "active").length,
    review: number(summary.review) || works.filter((work) => text(work.phase) === "review").length,
    blocked: number(summary.blocked) || works.filter((work) => text(work.condition) === "blocked").length,
  };

  return (
    <main className="h-full overflow-auto bg-background p-4 sm:p-6 lg:p-8" data-company-work-authority="team-work" data-company-work-read-only="true" data-company-work-projection="company_work_aggregate">
      <div className="mx-auto max-w-[1320px]">
        <header className="flex flex-col gap-4 border-b border-border pb-6 lg:flex-row lg:items-end lg:justify-between">
          <div>
            <p className="text-[11px] font-semibold uppercase tracking-[0.2em] text-primary">Company aggregate · TeamWork authority</p>
            <h1 className="mt-2 text-3xl font-semibold tracking-tight">Work</h1>
            <p className="mt-2 max-w-2xl text-sm leading-6 text-muted-foreground">
              A read-only company view across execution spaces. Every row keeps the original Work id and revision; mutations route to the Team Work surface.
            </p>
          </div>
          <div className="grid grid-cols-4 gap-2">
            {[["Total", counts.total], ["Active", counts.active], ["Review", counts.review], ["Blocked", counts.blocked]].map(([label, value]) => (
              <div key={String(label)} className="min-w-20 rounded-xl border border-border bg-card px-3 py-2 text-center">
                <p className="text-lg font-semibold">{value}</p>
                <p className="text-[10px] uppercase tracking-wider text-muted-foreground">{label}</p>
              </div>
            ))}
          </div>
        </header>

        <section className="mt-5 flex flex-col gap-3 rounded-xl border border-border bg-card p-3 sm:flex-row">
          <label className="flex min-h-10 flex-1 items-center gap-2 rounded-lg border border-border bg-background px-3">
            <Search className="size-4 text-muted-foreground" />
            <input value={query} onChange={(event) => setQuery(event.target.value)} placeholder="Search title, Work id, Team or owner" className="w-full bg-transparent text-sm outline-none placeholder:text-muted-foreground" />
          </label>
          <select value={phase} onChange={(event) => setPhase(event.target.value)} className="min-h-10 rounded-lg border border-border bg-background px-3 text-sm">
            <option value="all">All phases</option><option value="open">Open</option><option value="active">Active</option><option value="review">Review</option><option value="closed">Closed</option>
          </select>
          <select value={condition} onChange={(event) => setCondition(event.target.value)} className="min-h-10 rounded-lg border border-border bg-background px-3 text-sm">
            <option value="all">All conditions</option><option value="normal">Normal</option><option value="blocked">Blocked</option><option value="on_hold">On hold</option>
          </select>
        </section>

        {filtered.length ? (
          <section className="mt-5 grid gap-3 lg:grid-cols-2">
            {filtered.map((work) => {
              const blocked = text(work.condition) === "blocked";
              const accepted = text(work.phase) === "closed" && text(work.resolution) === "accepted";
              const onHold = text(work.condition) === "on_hold";
              const Icon = blocked ? AlertTriangle : accepted ? CheckCircle2 : onHold ? CirclePause : BriefcaseBusiness;
              return (
                <article key={text(work.id)} className="rounded-xl border border-border bg-card p-4 shadow-sm">
                  <div className="flex items-start gap-3">
                    <div className="rounded-lg border border-border bg-muted/35 p-2"><Icon className="size-4 text-primary" /></div>
                    <div className="min-w-0 flex-1">
                      <div className="flex flex-wrap items-center gap-2">
                        <h2 className="font-semibold">{text(work.title, "Untitled Work")}</h2>
                        <span className={`rounded-full border px-2 py-0.5 text-[10px] font-semibold uppercase tracking-wide ${lifecycleTone(work)}`}>{lifecycleLabel(work)}</span>
                      </div>
                      <p className="mt-1 font-mono text-[11px] text-muted-foreground">{text(work.id, "unknown-work")}</p>
                      {text(work.context_markdown) && <p className="mt-3 line-clamp-3 text-sm leading-6 text-muted-foreground">{text(work.context_markdown)}</p>}
                      <dl className="mt-4 grid grid-cols-2 gap-2 text-xs sm:grid-cols-3">
                        <div><dt className="text-muted-foreground">Team</dt><dd className="mt-1 truncate font-medium">{text(work.team_id, "compat scope")}</dd></div>
                        <div><dt className="text-muted-foreground">Team run</dt><dd className="mt-1 truncate font-medium">{text(work.team_run_id, "—")}</dd></div>
                        <div><dt className="text-muted-foreground">Owner</dt><dd className="mt-1 truncate font-medium">{text(work.owner_member_id, "Unassigned")}</dd></div>
                      </dl>
                      {text(work.blocker_reason) && <p className="mt-3 rounded-lg border border-destructive/25 bg-destructive/[0.04] px-3 py-2 text-xs text-destructive">{text(work.blocker_reason)}</p>}
                    </div>
                  </div>
                </article>
              );
            })}
          </section>
        ) : (
          <section className="mt-5 rounded-xl border border-dashed border-border p-8 text-center">
            <BriefcaseBusiness className="mx-auto size-5 text-muted-foreground" />
            <h2 className="mt-3 font-semibold">No matching Work</h2>
            <p className="mt-1 text-sm text-muted-foreground">The aggregate does not invent fallback rows. Adjust filters or create Work in a Team execution space.</p>
          </section>
        )}
      </div>
    </main>
  );
}
