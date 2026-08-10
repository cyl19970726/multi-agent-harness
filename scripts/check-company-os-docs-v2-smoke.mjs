#!/usr/bin/env node
/**
 * AI-first Docs v2 end-to-end smoke acceptance (ADR 0054 Phase 0).
 *
 * Runs the real `target/debug/firm` binary against a throwaway Company
 * Store and proves the full loop: page create -> scoped reads (outline /
 * keyword / section / range, fragment + excerpt honesty) -> full page write
 * with expected-revision -> REVISION_CONFLICT on stale base -> idempotent
 * replay -> append with anchor -> page_embed resolution -> pinned revision
 * reads. No fixtures: every assertion reads the binary's real JSON output.
 */

import { execFileSync } from "node:child_process";
import { existsSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import process from "node:process";

const repoRoot = path.resolve(new URL("..", import.meta.url).pathname);
const harness = path.join(repoRoot, "target", "debug", "firm");

if (!existsSync(harness)) {
  execFileSync("cargo", ["build", "-q", "-p", "firm-cli"], {
    cwd: repoRoot,
    stdio: "inherit",
  });
}

let failures = 0;
const fail = (message) => {
  failures += 1;
  console.error(`FAIL  ${message}`);
};
const pass = (message) => console.log(`PASS  ${message}`);

function check(name, condition, detail = "") {
  if (condition) {
    pass(name);
  } else {
    fail(`${name}${detail ? ` — ${detail}` : ""}`);
  }
}

const home = mkdtempSync(path.join(tmpdir(), "harness-docs-v2-smoke-"));
const env = { ...process.env, HARNESS_HOME: home };

function run(args, expectFail = false) {
  try {
    const stdout = execFileSync(harness, args, { env, encoding: "utf8", cwd: repoRoot });
    return { ok: true, stdout, stderr: "" };
  } catch (error) {
    if (expectFail) {
      return {
        ok: false,
        stdout: error.stdout?.toString() ?? "",
        stderr: `${error.stderr?.toString() ?? ""}${error.message}`,
      };
    }
    throw error;
  }
}

function json(result, name) {
  try {
    return JSON.parse(result.stdout);
  } catch {
    fail(`${name}: output is not JSON: ${result.stdout.slice(0, 200)}`);
    return {};
  }
}

try {
  // Bootstrap a company store.
  run(["company", "init", "--id", "docs-v2-smoke", "--name", "Docs V2 Smoke"]);
  const company = ["--company", "docs-v2-smoke"];

  // --- 1. page create with mixed markdown ------------------------------
  const markdownPath = path.join(home, "page-v1.md");
  writeFileSync(
    markdownPath,
    [
      "# Overview",
      "",
      "Agent-first docs smoke page.",
      "",
      "## Capabilities",
      "",
      "- revisions",
      "- optimistic concurrency",
      "",
      "1. first step",
      "2. second step",
      "",
      "- [x] schema landed",
      "- [ ] cli landed",
      "",
      "> [!note] Scope",
      "> Phase 0 only.",
      "",
      "| surface | status |",
      "| --- | --- |",
      "| cli | green |",
      "",
      "```text",
      "page read/write/append",
      "```",
      "",
      "---",
      "",
      "![[page:document-cli-embedded-target display=card]]",
    ].join("\n"),
  );

  const created = json(
    run([
      ...company,
      "company",
      "docs",
      "page",
      "create",
      "--title",
      "Docs V2 Smoke Page",
      "--actor",
      "agent-smoke",
      "--markdown-file",
      markdownPath,
    ]),
    "page create",
  );
  check("page create returns revision 1", created.revision_number === 1, JSON.stringify(created));
  check("page create returns 64-hex digest", /^[0-9a-f]{64}$/.test(created.content_digest ?? ""));
  check("page create reports blocks", Number(created.blocks) >= 10, `blocks=${created.blocks}`);
  const docId = created.document_id;
  check("page create returns stable document id", typeof docId === "string" && docId.length > 0);

  // --- 2. scoped reads ---------------------------------------------------
  const outline = json(
    run([...company, "company", "docs", "page", "read", "--doc", docId, "--scope", "outline"]),
    "read outline",
  );
  check(
    "outline returns only headings",
    outline.blocks?.length === 2 && outline.blocks.every((b) => b.kind === "heading"),
    JSON.stringify(outline.blocks),
  );
  check("outline is marked fragment", outline.scope?.fragment === true);
  check("outline hides block ids at simple detail", outline.blocks?.every((b) => b.id === undefined));

  const outlineIds = json(
    run([
      ...company,
      "company",
      "docs",
      "page",
      "read",
      "--doc",
      docId,
      "--scope",
      "outline",
      "--detail",
      "with-ids",
    ]),
    "read outline with-ids",
  );
  const capabilitiesHeading = outlineIds.blocks?.find((b) => b.markdown.includes("Capabilities"));
  check("with-ids detail exposes block ids", Boolean(capabilitiesHeading?.id));

  const keyword = json(
    run([
      ...company,
      "company",
      "docs",
      "page",
      "read",
      "--doc",
      docId,
      "--scope",
      "keyword",
      "--keyword",
      "concurrency|schema",
      "--context-before",
      "1",
      "--context-after",
      "1",
    ]),
    "read keyword",
  );
  check(
    "keyword scope returns matched excerpt blocks",
    keyword.scope?.fragment === true &&
      Array.isArray(keyword.scope.excerpts) &&
      keyword.scope.excerpts.length === 2,
    JSON.stringify(keyword.scope),
  );
  check(
    "keyword hits surface in markdown",
    keyword.blocks?.some((b) => b.markdown.includes("concurrency")) &&
      keyword.blocks?.some((b) => b.markdown.includes("schema")),
  );

  if (capabilitiesHeading?.id) {
    const section = json(
      run([
        ...company,
        "company",
        "docs",
        "page",
        "read",
        "--doc",
        docId,
        "--scope",
        "section",
        "--start-block-id",
        capabilitiesHeading.id,
      ]),
      "read section",
    );
    check(
      "section scope spans heading until next same-level heading (here: to end of doc)",
      section.blocks?.length === 9 && section.blocks[0].kind === "heading",
      JSON.stringify(section.blocks?.map((b) => b.kind)),
    );

    const range = json(
      run([
        ...company,
        "company",
        "docs",
        "page",
        "read",
        "--doc",
        docId,
        "--scope",
        "range",
        "--start-block-id",
        capabilitiesHeading.id,
        "--end-block-id",
        capabilitiesHeading.id,
      ]),
      "read range",
    );
    check("range scope returns the exact slice", range.blocks?.length === 1);
  }

  // --- 3. revision-pinned read ------------------------------------------
  const pinned = json(
    run([...company, "company", "docs", "page", "read", "--doc", docId, "--revision", "1"]),
    "read revision 1",
  );
  check("pinned revision read returns revision 1", pinned.revision_number === 1);

  // --- 4. full page write with expected revision ------------------------
  const rewritePath = path.join(home, "page-v2.md");
  writeFileSync(
    rewritePath,
    ["# Overview", "", "Rewritten body.", "", "![[page:document-cli-embedded-target display=inline]]"].join(
      "\n",
    ),
  );
  const written = json(
    run([
      ...company,
      "company",
      "docs",
      "page",
      "write",
      "--doc",
      docId,
      "--expected-revision",
      "1",
      "--markdown-file",
      rewritePath,
      "--summary",
      "smoke rewrite",
      "--actor",
      "agent-smoke",
    ]),
    "page write",
  );
  check("page write advances to revision 2", written.revision_number === 2, JSON.stringify(written));

  const afterWrite = json(
    run([...company, "company", "docs", "page", "read", "--doc", docId, "--detail", "with-ids"]),
    "read after write",
  );
  check(
    "page write replaces the whole block set",
    afterWrite.blocks?.length === 3,
    JSON.stringify(afterWrite.blocks?.map((b) => b.kind)),
  );
  check(
    "page_embed survives the rewrite with inline display",
    afterWrite.blocks?.some((b) => b.kind === "page_embed" && b.markdown.includes("display=inline")),
  );

  // --- 5. stale writer gets REVISION_CONFLICT ----------------------------
  const conflict = run(
    [
      ...company,
      "company",
      "docs",
      "page",
      "write",
      "--doc",
      docId,
      "--expected-revision",
      "1",
      "--markdown",
      "# stale",
      "--actor",
      "agent-late",
    ],
    true,
  );
  check(
    "stale write is rejected with REVISION_CONFLICT",
    conflict.ok === false && conflict.stderr.includes("REVISION_CONFLICT"),
    conflict.stderr.slice(0, 200),
  );

  // --- 6. idempotent replay ----------------------------------------------
  // The document is at revision 2 after step 4; the first call with a fixed
  // action id commits revision 3, and the exact same call replays it.
  const replayArgs = [
    ...company,
    "company",
    "docs",
    "page",
    "write",
    "--doc",
    docId,
    "--expected-revision",
    "2",
    "--markdown-file",
    rewritePath,
    "--summary",
    "smoke rewrite",
    "--actor",
    "agent-smoke",
    "--action-id",
    "smoke-fixed-action-id",
  ];
  const first = json(run(replayArgs), "first committed action");
  const second = json(run(replayArgs), "replayed action");
  check("first command commits revision 3", first.revision_number === 3, JSON.stringify(first));
  check(
    "identical replay returns the same revision without advancing",
    second.replayed === true && second.revision_id === first.revision_id,
    JSON.stringify(second),
  );

  // A divergent payload under the same action id is an idempotency conflict:
  // it must be rejected and must not advance the revision.
  const divergent = run(
    [
      ...company,
      "company",
      "docs",
      "page",
      "write",
      "--doc",
      docId,
      "--expected-revision",
      "3",
      "--markdown",
      "# divergent payload",
      "--actor",
      "agent-smoke",
      "--action-id",
      "smoke-fixed-action-id",
    ],
    true,
  );
  check(
    "divergent payload under same action id returns IDEMPOTENCY_CONFLICT",
    divergent.ok === false && divergent.stderr.includes("IDEMPOTENCY_CONFLICT"),
    divergent.stderr.slice(0, 200),
  );

  // --- 7. append with anchor ----------------------------------------------
  // Full page writes replace the whole block set, so re-read current ids:
  // anchors from superseded revisions are intentionally invalid.
  const currentAfterReplay = json(
    run([...company, "company", "docs", "page", "read", "--doc", docId, "--detail", "with-ids"]),
    "read before append",
  );
  const anchorId = currentAfterReplay.blocks?.[0]?.id;
  const appended = json(
    run([
      ...company,
      "company",
      "docs",
      "page",
      "append",
      "--doc",
      docId,
      "--markdown",
      "## Appended\n\nAppended paragraph.",
      "--after",
      anchorId ?? "",
      "--summary",
      "smoke append",
      "--actor",
      "agent-smoke",
    ]),
    "page append",
  );
  check("append advances the revision", appended.revision_number === 4, JSON.stringify(appended));
  const afterAppend = json(
    run([...company, "company", "docs", "page", "read", "--doc", docId]),
    "read after append",
  );
  check(
    "append inserts after the anchor",
    afterAppend.blocks?.[1]?.kind === "heading" &&
      afterAppend.blocks?.[1]?.markdown.includes("Appended"),
    JSON.stringify(afterAppend.blocks?.map((b) => b.kind)),
  );

  // --- 8. page_embed resolution through query -----------------------------
  run([
    ...company,
    "company",
    "docs",
    "page",
    "create",
    "--title",
    "Embedded Target",
    "--id",
    "document-cli-embedded-target",
    "--actor",
    "agent-smoke",
    "--markdown",
    "Target body for embed cards.",
  ]);
  const embedTarget = run([
    ...company,
    "company",
    "docs",
    "page",
    "read",
    "--doc",
    "document-cli-embedded-target",
  ]);
  check(
    "embedded target page is independently readable",
    embedTarget.ok && embedTarget.stdout.includes("Target body"),
  );

  // --- 9. friction fixes F1/F2/F3/F5 ----------------------------------------
  // F1: missing embed target yields a write-time warning, never a failure.
  const warned = json(
    run([
      ...company,
      "company",
      "docs",
      "page",
      "create",
      "--title",
      "Warned Embeds",
      "--id",
      "document-cli-warned-embeds",
      "--actor",
      "agent-smoke",
      "--markdown",
      "![[page:document-does-not-exist-yet display=card]]",
    ]),
    "create with missing embed target",
  );
  check(
    "F1 missing page_embed target produces a write-time warning",
    Array.isArray(warned.warnings) &&
      warned.warnings.some((w) => w.includes("page_embed target missing: document-does-not-exist-yet")),
    JSON.stringify(warned.warnings),
  );

  // F2: heading addressing and -1 sugar for append.
  const byHeading = json(
    run([
      ...company,
      "company",
      "docs",
      "page",
      "append",
      "--doc",
      docId,
      "--markdown",
      "Appended via heading addressing.",
      "--after",
      "heading:Overview",
      "--actor",
      "agent-smoke",
    ]),
    "append via heading:<text>",
  );
  check("F2 append resolves heading:<text> addressing", typeof byHeading.revision_number === "number", JSON.stringify(byHeading));
  const byEnd = run(
    [
      ...company,
      "company",
      "docs",
      "page",
      "append",
      "--doc",
      docId,
      "--markdown",
      "Appended at end.",
      "--after",
      "-1",
      "--actor",
      "agent-smoke",
      "--format",
      "text",
    ],
    false,
  );
  check(
    "F2/F5 --after -1 with --format text prints a one-line summary",
    byEnd.ok && byEnd.stdout.startsWith("ok ") && byEnd.stdout.includes("sha256:"),
    byEnd.stdout.slice(0, 120),
  );
  const ambiguous = run(
    [
      ...company,
      "company",
      "docs",
      "page",
      "append",
      "--doc",
      docId,
      "--markdown",
      "x",
      "--after",
      "heading:definitely-no-such-heading",
      "--actor",
      "agent-smoke",
    ],
    true,
  );
  check(
    "F2 unmatched heading anchor is rejected with guidance",
    ambiguous.ok === false && ambiguous.stderr.includes("no heading matches"),
    ambiguous.stderr.slice(0, 160),
  );

  // F5: page write --format text prints the one-line summary too.
  const writeText = run(
    [
      ...company,
      "company",
      "docs",
      "page",
      "write",
      "--doc",
      "document-cli-warned-embeds",
      "--expected-revision",
      "1",
      "--markdown",
      "# Warned Embeds\n\nText-format rewrite.",
      "--actor",
      "agent-smoke",
      "--format",
      "text",
    ],
    false,
  );
  check(
    "F5 page write --format text prints a one-line summary",
    writeText.ok && writeText.stdout.startsWith("ok ") && writeText.stdout.includes("sha256:"),
    writeText.stdout.slice(0, 120),
  );

  // F3: cross-document projection search (matches page body content).
  const search = json(
    run([...company, "company", "docs", "page", "search", "--keyword", "rewritten body|target body"]),
    "page search",
  );
  check(
    "F3 page search scans across documents and labels itself non-FTS",
    String(search.index ?? "").includes("projection-scan") &&
      new Set((search.matches ?? []).map((m) => m.document_id)).size >= 2 &&
      (search.count ?? 0) >= 2,
    JSON.stringify({ index: search.index, count: search.count, docs: [...new Set((search.matches ?? []).map((m) => m.document_id))] }),
  );

  // --- 10. R1 rename / move / archive metadata commands -------------------
  const renamed = json(
    run([
      ...company,
      "company", "docs", "page", "rename",
      "--doc", "document-cli-warned-embeds",
      "--title", "Renamed Embeds Page",
      "--actor", "agent-smoke",
      "--summary", "smoke rename",
    ]),
    "page rename",
  );
  check("R1 rename advances the revision", typeof renamed.revision_number === "number", JSON.stringify(renamed));
  const readRenamed = json(
    run([...company, "company", "docs", "page", "read", "--doc", "document-cli-warned-embeds"]),
    "read renamed",
  );
  check("R1 rename updates the title", readRenamed.title === "Renamed Embeds Page", JSON.stringify(readRenamed.title));

  run([
    ...company,
    "company", "docs", "page", "create",
    "--title", "Move Parent", "--id", "document-cli-move-parent",
    "--actor", "agent-smoke", "--markdown", "# Move Parent",
  ]);
  run([
    ...company,
    "company", "docs", "page", "create",
    "--title", "Move Child", "--id", "document-cli-move-child",
    "--actor", "agent-smoke", "--markdown", "# Move Child",
  ]);
  const moved = json(
    run([
      ...company,
      "company", "docs", "page", "move",
      "--doc", "document-cli-move-child",
      "--parent", "document-cli-move-parent",
      "--actor", "agent-smoke",
    ]),
    "page move",
  );
  check("R1 move succeeds", moved.result === "success", JSON.stringify(moved));
  const readChild = json(
    run([...company, "company", "docs", "page", "read", "--doc", "document-cli-move-child"]),
    "read moved",
  );
  check(
    "R1 read exposes parent_document_id after move",
    readChild.parent_document_id === "document-cli-move-parent",
    JSON.stringify(readChild.parent_document_id),
  );
  const cyclic = run(
    [
      ...company,
      "company", "docs", "page", "move",
      "--doc", "document-cli-move-parent",
      "--parent", "document-cli-move-child",
      "--actor", "agent-smoke",
    ],
    true,
  );
  check(
    "R1 move rejects parent cycles",
    cyclic.ok === false && cyclic.stderr.includes("parent cycle"),
    cyclic.stderr.slice(0, 160),
  );
  json(
    run([
      ...company,
      "company", "docs", "page", "move",
      "--doc", "document-cli-move-child", "--parent", "-1",
      "--actor", "agent-smoke",
    ]),
    "move back to root",
  );
  const readRoot = json(
    run([...company, "company", "docs", "page", "read", "--doc", "document-cli-move-child"]),
    "read root again",
  );
  check("R1 move -1 returns the page to root", readRoot.parent_document_id == null, JSON.stringify(readRoot.parent_document_id));

  const dryArchive = json(
    run([
      ...company,
      "company", "docs", "page", "archive",
      "--doc", "document-cli-move-child",
      "--actor", "agent-smoke",
    ]),
    "archive dry run",
  );
  check("R1 archive without --confirm is a dry run", dryArchive.result === "dry_run", JSON.stringify(dryArchive));
  const readAfterDry = json(
    run([...company, "company", "docs", "page", "read", "--doc", "document-cli-move-child"]),
    "read after dry archive",
  );
  check("R1 dry-run archive leaves lifecycle unchanged", readAfterDry.lifecycle_status === "active", JSON.stringify(readAfterDry.lifecycle_status));
  json(
    run([
      ...company,
      "company", "docs", "page", "archive",
      "--doc", "document-cli-move-child", "--confirm",
      "--actor", "agent-smoke",
    ]),
    "archive confirmed",
  );
  const readArchived = json(
    run([...company, "company", "docs", "page", "read", "--doc", "document-cli-move-child"]),
    "read archived",
  );
  check("R1 archive with --confirm sets lifecycle archived", readArchived.lifecycle_status === "archived", JSON.stringify(readArchived.lifecycle_status));

  const staleRename = run(
    [
      ...company,
      "company", "docs", "page", "rename",
      "--doc", "document-cli-move-parent",
      "--title", "Should Fail",
      "--expected-revision", "0",
      "--actor", "agent-smoke",
    ],
    true,
  );
  check(
    "R1 stale expected revision is rejected on metadata writes",
    staleRename.ok === false && staleRename.stderr.includes("REVISION_CONFLICT"),
    staleRename.stderr.slice(0, 160),
  );

  // --- 11. markdown output format -------------------------------------------
  const markdownOut = run([
    ...company,
    "company",
    "docs",
    "page",
    "read",
    "--doc",
    docId,
    "--format",
    "markdown",
  ]);
  check(
    "markdown format renders serialized blocks",
    markdownOut.ok && markdownOut.stdout.includes("# Overview"),
    markdownOut.stdout.slice(0, 120),
  );
} finally {
  rmSync(home, { recursive: true, force: true });
}

if (failures > 0) {
  console.error(`\ncompany-os docs-v2 smoke: ${failures} failure(s)`);
  process.exit(1);
}
console.log("\ncompany-os docs-v2 smoke: all checks passed");
