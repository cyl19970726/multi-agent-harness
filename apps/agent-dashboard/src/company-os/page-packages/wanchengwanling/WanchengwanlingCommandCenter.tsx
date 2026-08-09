import {
  ArrowUpRight,
  Bot,
  CheckCircle2,
  CircleDollarSign,
  FileText,
  Flag,
  MapPinned,
  PackageCheck,
  Store,
  UsersRound,
} from "lucide-react";
import type { ReactNode } from "react";

import { Badge } from "@/components/ui/badge";
import { ActorAvatar, ArtField, EditorialTitle, ObjectEmblem } from "../../visuals";
import { preserveCompanyOsWorkbenchContext } from "../../docs/url";

type Json = Record<string, unknown>;

function objects(value: unknown): Json[] {
  if (!Array.isArray(value)) return [];
  return value.filter((item): item is Json => Boolean(item) && typeof item === "object" && !Array.isArray(item));
}

function unbox(value: Json): Json {
  const record = value.record;
  return record && typeof record === "object" && !Array.isArray(record)
    ? { ...(record as Json), ...value }
    : value;
}

function text(value: unknown, fallback = ""): string {
  return typeof value === "string" ? value : fallback;
}

function number(value: unknown, fallback = 0): number {
  return typeof value === "number" && Number.isFinite(value) ? value : fallback;
}

function fields(record?: Json): Json {
  return record?.fields && typeof record.fields === "object" && !Array.isArray(record.fields) ? record.fields as Json : {};
}

function contentText(block?: Json): string {
  const content = block?.content;
  if (!content || typeof content !== "object" || Array.isArray(content)) return "";
  return text((content as Json).text).trim();
}

function amountLabel(record: Json): string {
  const display = text(record.display_amount);
  if (display) return display;
  const amount = record.amount;
  if (amount && typeof amount === "object" && !Array.isArray(amount)) {
    const value = text((amount as Json).amount) || String((amount as Json).value ?? "");
    const currency = text((amount as Json).currency);
    if (value && currency === "CNY") return `¥${value}`;
    if (value && currency) return `${currency} ${value}`;
    if (value) return value;
  }
  if (typeof amount === "number" || typeof amount === "string") return String(amount);
  return "";
}

function href(path: string): string {
  return preserveCompanyOsWorkbenchContext(path) ?? path;
}

function statusLabel(value: unknown): string {
  const raw = text(value, "unknown");
  return raw.replace(/[_-]+/g, " ");
}

function versionParts(value: string): number[] {
  return value.split(".").map((part) => Number.parseInt(part, 10)).map((part) => Number.isFinite(part) ? part : 0);
}

function compareVersion(left: string, right: string): number {
  const a = versionParts(left);
  const b = versionParts(right);
  for (let index = 0; index < Math.max(a.length, b.length); index += 1) {
    const delta = (a[index] ?? 0) - (b[index] ?? 0);
    if (delta !== 0) return delta;
  }
  return 0;
}

interface CommandCenterDoc {
  id: string;
  title: string;
  blocks: number;
  moduleId: string | undefined;
  preview: string[];
}

interface CommandCenterModel {
  docs: {
    total: number;
    withBlocks: number;
    core: CommandCenterDoc[];
    primary: CommandCenterDoc[];
  };
  business: {
    statement: string;
    physicalPrice: number;
    virtualPrice: number;
    merchantShare: number;
    companyShare: number;
    spotCount: number;
    magnetUnlock: number;
    lotteryUnlock: number;
  };
  modules: Array<{ id: string; name: string; documentId?: string; recordCount: number; customCount: number }>;
  work: {
    total: number;
    active: number;
    completed: number;
    waiting: number;
    commandCenter?: Json;
    next: Json[];
  };
  finance: {
    commitments: number;
    payments: number;
    amountLabels: string[];
  };
  actors: {
    governance: Json[];
    business: Json[];
  };
  runtime: {
    definitionId: string;
    packageId?: string;
    artifact?: string;
    state: "implemented_package" | "metadata_only" | "missing_package";
  };
}

function buildModel(source: unknown, pageId: string): CommandCenterModel {
  const root = source && typeof source === "object" && !Array.isArray(source) ? source as Json : {};
  const documents = objects(root.documents).map(unbox).filter((entry) => text(entry.id).startsWith("document-wcw-"));
  const blocks = objects(root.blocks).map(unbox);
  const modules = objects(root.business_modules).map(unbox).filter((entry) => text(entry.id).startsWith("module-wcw-"));
  const records = objects(root.typed_records).map(unbox).filter((entry) => text(entry.id).startsWith("record-wcw-"));
  const workRoot = root.work && typeof root.work === "object" && !Array.isArray(root.work) ? root.work as Json : {};
  const works = (objects(workRoot.works).length ? objects(workRoot.works) : objects(root.works)).map(unbox).filter((entry) => text(entry.id).startsWith("work-wcw-"));
  const commitments = [...objects(root.commitments), ...objects(root.financial_records).filter((entry) => text(unbox(entry).type) === "commitment")]
    .map(unbox)
    .filter((entry, index, entries) => entries.findIndex((candidate) => text(candidate.id) === text(entry.id)) === index);
  const payments = objects(root.payments).map(unbox);
  const actors = objects(root.actors).map(unbox).filter((entry) => /wcw|wanchengwanling/i.test(`${entry.id} ${entry.display_name} ${entry.role ?? ""}`));
  const definitions = objects(root.custom_page_definitions).map(unbox);
  const packages = objects(root.custom_page_packages).map(unbox);
  const definition = definitions.find((entry) => text(entry.id) === pageId);
  const packageRef = packages
    .filter((entry) => text(entry.definition_id) === pageId)
    .sort((left, right) => compareVersion(text(right.version), text(left.version)) || text(right.built_at).localeCompare(text(left.built_at)))[0];
  const byId = new Map(records.map((entry) => [text(entry.id), entry]));
  const projectOverview = byId.get("record-wcw-project-overview-mvp");
  const physical = byId.get("record-wcw-bracelet-physical-nfc");
  const virtual = byId.get("record-wcw-bracelet-virtual");
  const site = byId.get("record-wcw-site-jieyang-ancient-city");
  const active = works.filter((item) => text(item.phase) !== "closed");
  const moduleRecordCounts = new Map<string, number>();
  for (const record of records) {
    const moduleId = text(record.module_id);
    if (moduleId) moduleRecordCounts.set(moduleId, (moduleRecordCounts.get(moduleId) ?? 0) + 1);
  }
  const moduleByDocument = new Map<string, string>();
  for (const module of modules) {
    const documentId = text(module.root_document_ref);
    const moduleId = text(module.id);
    if (documentId && moduleId) moduleByDocument.set(documentId, moduleId);
  }
  const blockById = new Map(blocks.map((block) => [text(block.id), block]));
  const documentPreview = (document: Json): string[] => {
    const documentId = text(document.id);
    return (Array.isArray(document.block_ids) ? document.block_ids : [])
      .map((blockId) => blockById.get(text(blockId)))
      .filter((block): block is Json => {
        if (!block) return false;
        return text(block.document_id, text(block.document_ref)) === documentId;
      })
      .map(contentText)
      .filter(Boolean)
      .filter((value) => !["项目一句话", "MVP 用户闭环", "Company OS 模块地图", "卖什么"].includes(value))
      .slice(0, 2);
  };
  const coreDocs = documents
    .filter((entry) => text(entry.parent_document_id) === "document-wcw-root" || text(entry.id) === "document-wcw-project-home")
    .sort((a, b) => text(a.title).localeCompare(text(b.title)))
    .map((entry) => ({
      id: text(entry.id),
      title: text(entry.title),
      blocks: Array.isArray(entry.block_ids) ? entry.block_ids.length : 0,
      moduleId: moduleByDocument.get(text(entry.id)),
      preview: documentPreview(entry),
    }));
  const hasRenderableArtifact = Boolean(packageRef && /\.tsx?$/.test(text(packageRef.artifact_ref)));

  return {
    docs: {
      total: documents.length,
      withBlocks: documents.filter((entry) => Array.isArray(entry.block_ids) && entry.block_ids.length > 0).length,
      core: coreDocs,
      primary: ["document-wcw-project-home", "document-wcw-business-model"]
        .map((id) => coreDocs.find((entry) => entry.id === id))
        .filter((entry): entry is CommandCenterDoc => Boolean(entry)),
    },
    business: {
      statement: text(fields(projectOverview).statement, "Wanchengwanling commercial operating records are not supplied by this projection."),
      physicalPrice: number(fields(physical).price_cny),
      virtualPrice: number(fields(virtual).price_cny),
      merchantShare: number(fields(physical).merchant_share_cny),
      companyShare: number(fields(physical).company_share_cny),
      spotCount: number(fields(site).mvp_spot_count),
      magnetUnlock: number(fields(site).magnet_unlock_checkins),
      lotteryUnlock: number(fields(site).lottery_unlock_checkins),
    },
    modules: modules.map((entry) => ({
      id: text(entry.id),
      name: text(entry.name, text(entry.id)),
      documentId: text(entry.root_document_ref) || undefined,
      recordCount: moduleRecordCounts.get(text(entry.id)) ?? 0,
      customCount: Array.isArray(entry.custom_page_definition_refs) ? entry.custom_page_definition_refs.length : 0,
    })),
    work: {
      total: works.length,
      active: active.length,
      completed: works.filter((item) => text(item.phase) === "closed" && text(item.resolution) === "accepted").length,
      waiting: works.filter((item) => text(item.phase) === "review").length,
      commandCenter: works.find((item) => text(item.id) === "work-wcw-custom-command-center"),
      next: active.slice(0, 5),
    },
    finance: {
      commitments: commitments.length,
      payments: payments.length,
      amountLabels: commitments.map(amountLabel).filter(Boolean),
    },
    actors: {
      governance: actors.filter((entry) => /governance|lead/i.test(`${entry.id} ${entry.display_name} ${entry.role ?? ""}`)).slice(0, 6),
      business: actors.filter((entry) => !/governance|lead/i.test(`${entry.id} ${entry.display_name} ${entry.role ?? ""}`)).slice(0, 8),
    },
    runtime: {
      definitionId: text(definition?.id, pageId),
      packageId: text(packageRef?.id) || undefined,
      artifact: text(packageRef?.artifact_ref) || undefined,
      state: hasRenderableArtifact ? "implemented_package" : packageRef ? "metadata_only" : "missing_package",
    },
  };
}

function Metric({ label, value, detail }: { label: string; value: string | number; detail?: string }) {
  return (
    <div className="rounded-2xl border border-border bg-card/75 p-4">
      <p className="text-[10px] font-semibold uppercase tracking-[0.18em] text-muted-foreground">{label}</p>
      <p className="mt-3 text-3xl font-semibold tracking-tight">{value}</p>
      {detail && <p className="mt-1 text-xs leading-5 text-muted-foreground">{detail}</p>}
    </div>
  );
}

function LinkCard({ href: target, title, detail, icon }: { href: string; title: string; detail?: string; icon: ReactNode }) {
  return (
    <a href={href(target)} className="group flex min-w-0 items-start gap-3 rounded-xl border border-border bg-card/70 p-3 hover:border-primary/30 hover:bg-primary/[0.045]">
      <span className="mt-0.5 text-primary">{icon}</span>
      <span className="min-w-0 flex-1">
        <span className="block truncate text-sm font-semibold group-hover:text-primary">{title}</span>
        {detail && <span className="mt-1 block text-xs leading-5 text-muted-foreground">{detail}</span>}
      </span>
      <ArrowUpRight className="mt-1 size-3.5 shrink-0 text-muted-foreground" />
    </a>
  );
}

function PrimaryDocCard({ document }: { document: CommandCenterModel["docs"]["primary"][number] }) {
  return (
    <article className="rounded-2xl border border-primary/20 bg-card/85 p-4 shadow-sm" data-company-os-ref={document.id} data-wcw-primary-doc={document.id}>
      <div className="flex items-start justify-between gap-3">
        <div className="min-w-0">
          <Badge tone="info">Default Document renderer</Badge>
          <h3 className="mt-3 truncate text-base font-semibold">{document.title}</h3>
          <p className="mt-1 text-xs text-muted-foreground">{document.blocks} visible Blocks · Store-backed Document truth</p>
        </div>
        <FileText className="size-5 shrink-0 text-primary" />
      </div>
      <div className="mt-4 space-y-2">
        {document.preview.length ? document.preview.map((line) => (
          <p key={line} className="line-clamp-2 rounded-xl border border-border bg-background/65 px-3 py-2 text-xs leading-5 text-muted-foreground">{line}</p>
        )) : (
          <p className="rounded-xl border border-dashed border-border px-3 py-2 text-xs leading-5 text-muted-foreground">No preview Blocks supplied by the current Store projection.</p>
        )}
      </div>
      <div className="mt-4 flex flex-wrap gap-2">
        <a href={href(`?surface=docs&document=${encodeURIComponent(document.id)}`)} className="inline-flex items-center gap-1 rounded-lg bg-primary px-2.5 py-1.5 text-xs font-semibold text-primary-foreground">
          Open Document <ArrowUpRight className="size-3" />
        </a>
        {document.moduleId && (
          <a href={href(`?surface=docs&module=${encodeURIComponent(document.moduleId)}`)} className="inline-flex items-center gap-1 rounded-lg border border-border bg-background px-2.5 py-1.5 text-xs font-medium hover:bg-accent/45">
            Open Module <ArrowUpRight className="size-3" />
          </a>
        )}
      </div>
    </article>
  );
}

export function WanchengwanlingCommandCenter({ source, pageId = "page-wcw-command-center" }: { source: unknown; pageId?: string }) {
  const model = buildModel(source, pageId);
  const readiness = model.docs.total > 0 ? Math.round((model.docs.withBlocks / model.docs.total) * 100) : 0;
  return (
    <main className="company-workbench h-full overflow-auto bg-[radial-gradient(circle_at_78%_-8%,hsl(var(--primary)/0.12),transparent_30%),linear-gradient(to_bottom,hsl(var(--background)),hsl(var(--muted)/0.28))]" data-company-os-custom-page={pageId} data-wcw-command-center="store-live">
      <ArtField />
      <div className="relative mx-auto max-w-[1500px] px-5 py-7 lg:px-9">
        <header className="grid gap-6 border-b border-border/80 pb-6 xl:grid-cols-[minmax(0,1fr)_360px]">
          <div className="min-w-0">
            <div className="flex items-center gap-3">
              <ObjectEmblem kind="module" className="size-12 rounded-2xl" />
              <div>
                <p className="text-[10px] font-semibold uppercase tracking-[0.22em] text-primary">Wanchengwanling · Custom Page Package</p>
                <Badge tone={model.runtime.state === "implemented_package" ? "good" : "warn"}>{model.runtime.state.replace(/_/g, " ")}</Badge>
              </div>
            </div>
            <EditorialTitle className="mt-6">Command Center</EditorialTitle>
            <p className="mt-4 max-w-3xl text-sm leading-6 text-muted-foreground">{model.business.statement}</p>
          </div>
          <aside className="rounded-2xl border border-primary/25 bg-card/80 p-4">
            <p className="text-[10px] font-semibold uppercase tracking-[0.18em] text-primary">Runtime contract</p>
            <dl className="mt-3 space-y-2 text-xs">
              <div className="grid grid-cols-[6.5rem_minmax(0,1fr)] gap-2"><dt className="text-muted-foreground">Definition</dt><dd className="truncate font-medium">{model.runtime.definitionId}</dd></div>
              <div className="grid grid-cols-[6.5rem_minmax(0,1fr)] gap-2"><dt className="text-muted-foreground">Package</dt><dd className="truncate">{model.runtime.packageId ?? "missing"}</dd></div>
              <div className="grid grid-cols-[6.5rem_minmax(0,1fr)] gap-2"><dt className="text-muted-foreground">Artifact</dt><dd className="break-words">{model.runtime.artifact ?? "missing"}</dd></div>
            </dl>
          </aside>
        </header>

        <section className="mt-6 grid gap-4 md:grid-cols-2 xl:grid-cols-4">
          <Metric label="Docs usable" value={`${readiness}%`} detail={`${model.docs.withBlocks}/${model.docs.total} Wanchengwanling docs contain visible Blocks`} />
          <Metric label="TeamWorks" value={model.work.total} detail={`${model.work.active} active · ${model.work.completed} accepted · ${model.work.waiting} in review`} />
          <Metric label="Product price" value={`¥${model.business.physicalPrice || "—"} / ¥${model.business.virtualPrice || "—"}`} detail={`Physical / virtual bracelet`} />
          <Metric label="Check-in rules" value={`${model.business.magnetUnlock || "—"} / ${model.business.lotteryUnlock || "—"}`} detail={`${model.business.spotCount || "—"} MVP spots · magnet / lottery`} />
        </section>

        <section className="mt-6 rounded-2xl border border-border bg-card/75 p-5" data-wcw-primary-docs="true">
          <div className="flex flex-wrap items-start justify-between gap-3">
            <div>
              <p className="text-[10px] font-semibold uppercase tracking-[0.18em] text-primary">Primary operating docs</p>
              <h2 className="company-editorial-title mt-1 text-2xl">Start from 00 Project Home and 01 Business Model</h2>
              <p className="mt-2 max-w-3xl text-xs leading-5 text-muted-foreground">
                The custom page is a readable control room. Default Document pages remain the fallback truth for Blocks and navigation; default Module pages remain the fallback truth for TypedRecords, Views, and Relations.
              </p>
            </div>
            <a href={href("?surface=docs&document=document-wcw-project-home")} className="inline-flex items-center gap-1 rounded-lg border border-primary/25 bg-primary/[0.06] px-3 py-2 text-xs font-semibold text-primary hover:bg-primary/[0.1]">
              Open default Docs route <ArrowUpRight className="size-3" />
            </a>
          </div>
          <div className="mt-5 grid gap-4 xl:grid-cols-2">
            {model.docs.primary.map((document) => <PrimaryDocCard key={document.id} document={document} />)}
          </div>
        </section>

        <section className="mt-6 grid gap-5 xl:grid-cols-[minmax(0,1.15fr)_minmax(320px,0.85fr)]">
          <div className="space-y-5">
            <section className="rounded-2xl border border-border bg-card/75 p-5">
              <div className="flex items-center justify-between gap-3">
                <div>
                  <p className="text-[10px] font-semibold uppercase tracking-[0.18em] text-muted-foreground">Business operating model</p>
                  <h2 className="company-editorial-title mt-1 text-2xl">What this project sells and operates</h2>
                </div>
                <Store className="size-6 text-primary" />
              </div>
              <div className="mt-5 grid gap-3 md:grid-cols-3">
                <Metric label="Physical NFC bracelet" value={`¥${model.business.physicalPrice || "—"}`} detail={`Merchant ¥${model.business.merchantShare || "—"} · Company ¥${model.business.companyShare || "—"}`} />
                <Metric label="Virtual bracelet" value={`¥${model.business.virtualPrice || "—"}`} detail="Mini program purchase path" />
                <Metric label="Finance records" value={model.finance.commitments} detail={`${model.finance.amountLabels.join(", ") || "No amount"} · ${model.finance.payments} payment records`} />
              </div>
            </section>

            <section className="rounded-2xl border border-border bg-card/75 p-5">
              <div className="flex items-center gap-2">
                <Flag className="size-4 text-primary" />
                <h2 className="company-editorial-title text-2xl">Next active Work</h2>
              </div>
              <div className="mt-4 space-y-2">
                {model.work.next.length ? model.work.next.map((item) => (
                  <a key={text(item.id)} href={href(`?surface=work&teamWork=${encodeURIComponent(text(item.id))}`)} data-company-os-ref={text(item.id)} className="grid gap-2 rounded-xl border border-border bg-background/70 p-3 text-sm hover:bg-accent/40 md:grid-cols-[minmax(0,1fr)_8rem]">
                    <span className="min-w-0">
                      <span className="block truncate font-semibold">{text(item.title, text(item.id))}</span>
                      <span className="mt-1 block text-xs text-muted-foreground">{text(item.team_id, "team unavailable")} · run {text(item.team_run_id, "unavailable")}</span>
                    </span>
                    <span className="text-xs capitalize text-muted-foreground">{statusLabel(`${text(item.phase)} · ${text(item.condition)}`)}</span>
                  </a>
                )) : <p className="rounded-xl border border-dashed border-border p-3 text-sm text-muted-foreground">No active Wanchengwanling TeamWorks are supplied.</p>}
              </div>
            </section>
          </div>

          <aside className="space-y-5">
            <section className="rounded-2xl border border-border bg-card/75 p-5">
              <div className="flex items-center gap-2">
                <Bot className="size-4 text-primary" />
                <h2 className="company-editorial-title text-2xl">Governance Agents</h2>
              </div>
              <div className="mt-4 grid gap-3">
                {model.actors.governance.map((actor) => (
                  <div key={text(actor.id)} data-company-os-ref={text(actor.id)} className="flex items-center gap-3 rounded-xl border border-border bg-background/70 p-3">
                    <ActorAvatar identity={text(actor.id)} name={text(actor.display_name, text(actor.id))} />
                    <div className="min-w-0">
                      <p className="truncate text-sm font-semibold">{text(actor.display_name, text(actor.id))}</p>
                      <p className="truncate text-xs text-muted-foreground">{text(actor.role, "Standing Agent")}</p>
                    </div>
                  </div>
                ))}
              </div>
            </section>

            <section className="rounded-2xl border border-border bg-card/75 p-5">
              <div className="flex items-center gap-2">
                <FileText className="size-4 text-primary" />
                <h2 className="company-editorial-title text-2xl">Core Docs</h2>
              </div>
              <div className="mt-4 space-y-2">
                {model.docs.core.slice(0, 8).map((document) => (
                  <LinkCard key={document.id} href={`?surface=docs&document=${encodeURIComponent(document.id)}`} title={document.title} detail={`${document.blocks} visible Block${document.blocks === 1 ? "" : "s"}`} icon={<FileText className="size-4" />} />
                ))}
              </div>
            </section>
          </aside>
        </section>

        <section className="mt-6 rounded-2xl border border-border bg-card/75 p-5">
          <div className="flex flex-wrap items-center justify-between gap-3">
            <div className="flex items-center gap-2">
              <PackageCheck className="size-4 text-primary" />
              <h2 className="company-editorial-title text-2xl">Business modules</h2>
            </div>
            <a href={href("?surface=docs")} className="inline-flex items-center gap-1 text-xs font-medium text-primary hover:underline">Open Docs workspace <ArrowUpRight className="size-3" /></a>
          </div>
          <div className="mt-4 grid gap-3 md:grid-cols-2 xl:grid-cols-3">
            {model.modules.map((module) => (
              <a key={module.id} href={href(`?surface=docs&module=${encodeURIComponent(module.id)}`)} data-company-os-ref={module.id} className="rounded-xl border border-border bg-background/70 p-3 hover:bg-accent/40">
                <p className="truncate text-sm font-semibold">{module.name}</p>
                <p className="mt-1 text-xs text-muted-foreground">{module.recordCount} TypedRecords · {module.customCount} custom pages</p>
              </a>
            ))}
          </div>
        </section>

        <section className="mt-6 grid gap-3 md:grid-cols-4">
          <LinkCard href="?surface=organization" title="Organization" detail={`${model.actors.governance.length + model.actors.business.length} Wanchengwanling actors`} icon={<UsersRound className="size-4" />} />
          <LinkCard href="?surface=work" title="Work board" detail="Milestones and read-only TeamWork aggregation" icon={<Flag className="size-4" />} />
          <LinkCard href="?surface=finance" title="Finance" detail="Commitments, payments, and money effects" icon={<CircleDollarSign className="size-4" />} />
          <LinkCard href="?surface=docs&document=document-wcw-route-ar-experience" title="Route rules" detail="8/12 check-in rules and AR experience" icon={<MapPinned className="size-4" />} />
        </section>

        {model.work.commandCenter && (
          <section className="mt-6 rounded-2xl border border-status-good/30 bg-status-good/[0.06] p-4 text-sm" data-company-os-ref={text(model.work.commandCenter.id)}>
            <div className="flex items-start gap-3">
              <CheckCircle2 className="mt-0.5 size-5 shrink-0 text-status-good" />
              <div>
                <p className="font-semibold">Implementation TeamWork is linked</p>
                <p className="mt-1 text-xs leading-5 text-muted-foreground">{text(model.work.commandCenter.title)} · {statusLabel(`${text(model.work.commandCenter.phase)} · ${text(model.work.commandCenter.condition)}`)}</p>
              </div>
            </div>
          </section>
        )}
      </div>
    </main>
  );
}
