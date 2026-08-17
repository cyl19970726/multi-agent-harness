// INACTIVE HISTORICAL (DOC-108 Stage B): this gate exercised the retired
// legacy CompanyOS surface and is removed from every pipeline. Kept as
// source-only history per the inactive-historical convention (file kept,
// removed from pipelines, named replacement) — see
// docs/current/operations/operations.md.
// Replacement: harness legacy-company-os export|verify (tests/legacy_company_os.rs)

#!/usr/bin/env node
/**
 * AI-first Docs v2 serve API live acceptance (ADR 0054 Phase 0).
 *
 * Boots the real `target/debug/firm serve` against a throwaway Company
 * Store and drives the docs-v2 HTTP contract end to end: page index, create,
 * scoped read (with-ids), write with expected_revision, REVISION_CONFLICT
 * (409), idempotent replay, append, revision history, and transport-token
 * denial. Every assertion hits the live server.
 */

import { execFileSync, spawn } from "node:child_process";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import process from "node:process";

const repoRoot = path.resolve(new URL("..", import.meta.url).pathname);
const harness = path.join(repoRoot, "target", "debug", "firm");
const addr = "127.0.0.1:18913";
const base = `http://${addr}`;
const companyId = "docs-v2-api-smoke";
const token = "docs-v2-api-smoke-token";

let failures = 0;
const fail = (message) => {
  failures += 1;
  console.error(`FAIL  ${message}`);
};
const pass = (message) => console.log(`PASS  ${message}`);
const check = (name, condition, detail = "") => {
  if (condition) {
    pass(name);
  } else {
    fail(`${name}${detail ? ` — ${detail}` : ""}`);
  }
};

const home = mkdtempSync(path.join(tmpdir(), "harness-docs-v2-api-"));
const env = { ...process.env, HARNESS_HOME: home };

let server = null;
try {
  execFileSync(
    harness,
    ["company", "init", "--id", companyId, "--name", "Docs V2 API Smoke"],
    { env, encoding: "utf8", cwd: repoRoot },
  );

  server = spawn(harness, ["serve", "--addr", addr], {
    env: { ...env, HARNESS_COMPANY_OS_TOKEN: token },
    cwd: repoRoot,
    stdio: ["ignore", "pipe", "pipe"],
  });
  let serverLog = "";
  server.stdout.on("data", (chunk) => (serverLog += chunk.toString()));
  server.stderr.on("data", (chunk) => (serverLog += chunk.toString()));

  // Wait for readiness.
  let ready = false;
  for (let attempt = 0; attempt < 60; attempt += 1) {
    try {
      const response = await fetch(`${base}/v1/company-os/docs-v2/pages?company=${companyId}`);
      if (response.ok) {
        ready = true;
        break;
      }
    } catch {
      // not up yet
    }
    await new Promise((resolve) => setTimeout(resolve, 250));
  }
  if (!ready) {
    fail(`serve did not become ready at ${base}\n${serverLog.slice(-800)}`);
    process.exit(1);
  }
  pass("serve is ready with docs-v2 pages endpoint");

  const company = `company=${companyId}`;
  const headers = { "Content-Type": "application/json", "X-Harness-Company-OS-Token": token };
  const post = (urlPath, body, withToken = true) =>
    fetch(`${base}${urlPath}?${company}`, {
      method: "POST",
      headers: withToken ? headers : { "Content-Type": "application/json" },
      body: JSON.stringify(body),
    });
  const get = (urlPath) => fetch(`${base}${urlPath}?${company}`);
  const unwrap = async (response, name) => {
    const payload = await response.json().catch(() => ({}));
    if (!payload.ok) {
      fail(`${name}: ${JSON.stringify(payload).slice(0, 300)}`);
      return {};
    }
    return payload.result ?? {};
  };

  // --- write without token is denied -------------------------------------
  const denied = await post("/v1/company-os/docs-v2/pages", {
    title: "No Token",
    markdown: "# x",
    actor: "agent:smoke",
  }, false);
  check("write without transport token is 403", denied.status === 403, `status=${denied.status}`);

  // --- create ---------------------------------------------------------------
  const created = await unwrap(
    await post("/v1/company-os/docs-v2/pages", {
      id: "document-api-smoke-page",
      title: "API Smoke Page",
      markdown: [
        "# API Overview",
        "",
        "First paragraph.",
        "",
        "## Details",
        "",
        "- alpha",
        "- beta",
        "",
        "![[page:document-api-embedded display=card]]",
      ].join("\n"),
      actor: { actor_type: "agent", actor_id: "agent-api-smoke" },
      summary: "api create",
    }),
    "api create",
  );
  check("api create returns revision 1", created.revision_number === 1, JSON.stringify(created));
  const docId = created.document_id;

  // --- index ------------------------------------------------------------------
  const index = await unwrap(await get("/v1/company-os/docs-v2/pages"), "pages index");
  check(
    "pages index lists the created document",
    index.items?.some((item) => item.document_id === docId && item.revision_number === 1),
    JSON.stringify(index),
  );

  // --- scoped read --------------------------------------------------------------
  const page = await unwrap(await get(`/v1/company-os/docs-v2/pages/${docId}`), "page read");
  check("page read returns blocks with ids", page.blocks?.length === 5 && page.blocks.every((b) => b.id));
  check(
    "page_embed block resolves with card display",
    page.blocks?.some((b) => b.kind === "page_embed" && b.markdown.includes("display=card")),
  );

  // --- write with expected revision ----------------------------------------------
  const write1 = await unwrap(
    await post(`/v1/company-os/docs-v2/pages/${docId}/write`, {
      markdown: "# API Overview\n\nRewritten over HTTP.",
      expected_revision: 1,
      actor: "agent:api-smoke",
      summary: "api write",
    }),
    "api write",
  );
  check("api write advances to revision 2", write1.revision_number === 2, JSON.stringify(write1));

  // --- stale write -> 409 ---------------------------------------------------------
  const stale = await post(`/v1/company-os/docs-v2/pages/${docId}/write`, {
    markdown: "# stale",
    expected_revision: 1,
    actor: "agent:late",
  });
  const stalePayload = await stale.json().catch(() => ({}));
  check(
    "stale write returns 409 REVISION_CONFLICT",
    stale.status === 409 && (stalePayload.detail ?? "").startsWith("REVISION_CONFLICT"),
    `status=${stale.status} body=${JSON.stringify(stalePayload).slice(0, 200)}`,
  );

  // --- idempotent replay -------------------------------------------------------------
  const replayBody = {
    markdown: "# API Overview\n\nReplayed body.",
    expected_revision: 2,
    actor: "agent:api-smoke",
    summary: "api replay",
    action_command_id: "api-smoke-fixed-action",
  };
  const first = await unwrap(await post(`/v1/company-os/docs-v2/pages/${docId}/write`, replayBody), "first fixed write");
  const second = await unwrap(await post(`/v1/company-os/docs-v2/pages/${docId}/write`, replayBody), "replayed write");
  check("fixed action id commits revision 3", first.revision_number === 3, JSON.stringify(first));
  check(
    "identical API replay returns the same revision",
    second.replayed === true && second.revision_id === first.revision_id,
    JSON.stringify(second),
  );

  // --- append ---------------------------------------------------------------------------
  const appended = await unwrap(
    await post(`/v1/company-os/docs-v2/pages/${docId}/append`, {
      markdown: "## Appended\n\nOver HTTP.",
      actor: "agent:api-smoke",
      summary: "api append",
    }),
    "api append",
  );
  check("api append advances to revision 4", appended.revision_number === 4, JSON.stringify(appended));

  // --- revision history --------------------------------------------------------------------
  const history = await unwrap(await get(`/v1/company-os/docs-v2/pages/${docId}/revisions`), "revisions");
  check("revision history has 4 commits", history.count === 4, JSON.stringify(history.items?.map((r) => r.revision_number)));
  check(
    "history digests are 64-hex",
    history.items?.every((r) => /^[0-9a-f]{64}$/.test(r.content_digest)),
  );

  // --- F4: entity_embed live resolution ------------------------------------------------------
  const embedHost = await unwrap(
    await post("/v1/company-os/docs-v2/pages", {
      id: "document-api-embed-host",
      title: "Embed Host",
      markdown: "host page for embed resolution",
      actor: "agent:api-smoke",
    }),
    "embed host create",
  );
  check("embed host page created", embedHost.revision_number === 1, JSON.stringify(embedHost));

  const moduleBody = {
    id: "module-api-smoke",
    name: "API Smoke Module",
    purpose: "embed resolution acceptance",
    root_document_ref: "document-api-embed-host",
    record_types: ["smoke_record"],
    relation_rules: [],
    default_view_refs: [],
    policy_refs: [],
    lifecycle_rules: [],
    metric_definition_refs: [],
    custom_page_definition_refs: [],
    status: "active",
    owner: { actor_type: "human", actor_id: "human-api-root" },
    created_at: "unix-ms:0",
    updated_at: "unix-ms:0",
  };
  const actorSeed = await post("/v1/company-os/actors", {
    actor_type: "human",
    actor: {
      id: "human-api-root",
      display_name: "API Root Human",
      title: null,
      status: "active",
      availability: null,
      membership_refs: [],
      responsibility_summary: "Acceptance bootstrap root",
      permission_policy_refs: ["company_os.admin"],
      authority_policy_refs: [],
      created_at: "unix-ms:0",
      updated_at: "unix-ms:0",
    },
  });
  check("human root actor bootstrapped for administrative seeding", actorSeed.status === 200, `status=${actorSeed.status}`);

  const adminPost = (path, record) =>
    post(path, { mode: "administrative", authority: { actor_type: "human", actor_id: "human-api-root" }, record });
  const moduleRes = await adminPost("/v1/company-os/business-modules", moduleBody);
  const modulePayload = await moduleRes.json().catch(() => ({}));
  check("business module seeded for embed resolution", moduleRes.status === 200 && modulePayload.ok !== false, `status=${moduleRes.status} ${JSON.stringify(modulePayload).slice(0, 160)}`);

  const recordBody = {
    id: "tr-api-smoke-1",
    module_id: "module-api-smoke",
    record_type: "smoke_record",
    title: "Resolved Smoke Record",
    fields: {},
    lifecycle_status: "active",
    source_document_ref: "document-api-embed-host",
    created_by: { actor_type: "human", actor_id: "human-api-root" },
    updated_by: { actor_type: "human", actor_id: "human-api-root" },
    created_at: "unix-ms:0",
    updated_at: "unix-ms:0",
  };
  const recordRes = await adminPost("/v1/company-os/typed-records", recordBody);
  const recordPayload = await recordRes.json().catch(() => ({}));
  check("typed record seeded for embed resolution", recordRes.status === 200 && recordPayload.ok !== false, `status=${recordRes.status} ${JSON.stringify(recordPayload).slice(0, 160)}`);

  await unwrap(
    await post("/v1/company-os/docs-v2/pages", {
      id: "document-api-embed-demo",
      title: "Embed Demo",
      markdown: [
        "![[typed_record:tr-api-smoke-1 display=card]]",
        "",
        "![[typed_record:tr-missing display=card]]",
      ].join("\n"),
      actor: "agent:api-smoke",
    }),
    "embed demo create",
  );
  const embedPage = await unwrap(await get("/v1/company-os/docs-v2/pages/document-api-embed-demo"), "embed demo read");
  const resolved = embedPage.resolved_embeds ?? {};
  check(
    "F4 entity_embed resolves live title from the owning ledger",
    resolved["typed_record:tr-api-smoke-1"]?.found === true &&
      resolved["typed_record:tr-api-smoke-1"]?.title === "Resolved Smoke Record",
    JSON.stringify(resolved),
  );
  check(
    "F4 missing entity_embed target resolves honestly as found:false",
    resolved["typed_record:tr-missing"]?.found === false,
    JSON.stringify(resolved["typed_record:tr-missing"]),
  );

  // --- embedded page is independently readable ---------------------------------------------
  await unwrap(
    await post("/v1/company-os/docs-v2/pages", {
      id: "document-api-embedded",
      title: "API Embedded Target",
      markdown: "Embedded target body.",
      actor: "agent:api-smoke",
    }),
    "embedded create",
  );
  const embedded = await unwrap(await get("/v1/company-os/docs-v2/pages/document-api-embedded"), "embedded read");
  check(
    "embedded page readable through API",
    embedded.blocks?.some((b) => b.markdown.includes("Embedded target body")),
  );
} catch (error) {
  fail(`unexpected error: ${error?.stack ?? error}`);
} finally {
  if (server) {
    server.kill("SIGTERM");
    await new Promise((resolve) => setTimeout(resolve, 300));
    if (!server.killed) {
      server.kill("SIGKILL");
    }
  }
  rmSync(home, { recursive: true, force: true });
}

if (failures > 0) {
  console.error(`\ncompany-os docs-v2 api: ${failures} failure(s)`);
  process.exit(1);
}
console.log("\ncompany-os docs-v2 api: all checks passed");
