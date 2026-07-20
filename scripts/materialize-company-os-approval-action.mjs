#!/usr/bin/env node

import { createHash } from "node:crypto";
import { cp, mkdir, readFile, rm, writeFile } from "node:fs/promises";
import { basename, dirname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const outputRoot = join(repoRoot, "docs/design/company-os-v2/approval-action-v1");

function argument(name) {
  const index = process.argv.indexOf(name);
  return index >= 0 ? process.argv[index + 1] : undefined;
}

async function sha256(path) {
  return `sha256:${createHash("sha256").update(await readFile(path)).digest("hex")}`;
}

function escapeHtml(value) {
  return String(value ?? "").replace(/[&<>\"]/g, (character) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[character]);
}

async function loadCapture(path, expectedStatus) {
  const absolute = resolve(path);
  const capture = JSON.parse(await readFile(absolute, "utf8"));
  if (capture.status !== "passed" || capture.data_mode !== "store-live" || capture.approval_action?.approval_status !== expectedStatus) {
    throw new Error(`${path} is not a passing ${expectedStatus} Store-live approval capture`);
  }
  if (capture.approval_action.payment_count !== 0 || capture.approval_action.commitment_status !== "pending_approval" || capture.approval_action.idempotent_replay !== true) {
    throw new Error(`${path} violates the approval-only acceptance boundary`);
  }
  return { absolute, capture };
}

async function main() {
  const approvedPath = argument("--approved-capture");
  const rejectedPath = argument("--rejected-capture");
  if (!approvedPath || !rejectedPath) throw new Error("--approved-capture and --rejected-capture are required");
  const [approved, rejected] = await Promise.all([
    loadCapture(approvedPath, "approved"),
    loadCapture(rejectedPath, "rejected"),
  ]);
  await rm(outputRoot, { recursive: true, force: true });
  await mkdir(outputRoot, { recursive: true });
  const sources = [
    { id: "requested", title: "1 · Requested", caption: "Store-live Approval before a Human decision.", source: approved.capture.approval_action.before.file },
    { id: "denied", title: "2 · Invalid capability denied", caption: "The browser remains on requested; no ActionCommand effect is applied.", source: approved.capture.approval_action.denied_invalid_capability.file },
    { id: "approved", title: "3A · Human approved", caption: "Approval is approved; Commitment stays pending and Payment stays absent.", source: approved.capture.approval_action.after.file },
    { id: "rejected", title: "3B · Human rejected", caption: "A separate isolated Store proves the native rejected branch.", source: rejected.capture.approval_action.after.file },
  ];
  const states = [];
  for (const item of sources) {
    const source = join(repoRoot, item.source);
    const destination = join(outputRoot, `${item.id}--desktop-1536x1024.png`);
    await cp(source, destination);
    states.push({ ...item, file: relative(outputRoot, destination), sha256: await sha256(destination) });
  }
  const actionSummary = (entry) => ({
    approval_id: entry.approval_id,
    approval_status: entry.approval_status,
    decided_by: entry.decided_by,
    action_command_id: entry.action_command_id,
    action_command_status: entry.action_command_status,
    audit_event_refs: entry.audit_event_refs,
    commitment_status: entry.commitment_status,
    payment_count: entry.payment_count,
    idempotent_replay: entry.idempotent_replay,
    capability_storage: entry.capability_storage,
  });
  const manifest = {
    contract: "company-os-approval-browser-action-v1",
    status: "human_review_pending",
    source_captures: {
      approved: relative(repoRoot, approved.absolute),
      rejected: relative(repoRoot, rejected.absolute),
    },
    assertions: [
      "invalid capability is denied without mutating the Approval",
      "named Human can approve or reject through approval.decide",
      "the ActionCommand is executed with authorization and execution audits",
      "an exact replay is idempotent",
      "the Commitment remains pending_approval",
      "no Payment is created",
      "the capability is not persisted or copied into evidence",
    ],
    approved: actionSummary(approved.capture.approval_action),
    rejected: actionSummary(rejected.capture.approval_action),
    states,
  };
  await writeFile(join(outputRoot, "manifest.json"), `${JSON.stringify(manifest, null, 2)}\n`);
  const cards = states.map((state) => `<article><div><strong>${escapeHtml(state.title)}</strong><p>${escapeHtml(state.caption)}</p></div><img src="${escapeHtml(state.file)}" alt="${escapeHtml(state.title)}" /></article>`).join("\n");
  const html = `<!doctype html><html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>Company OS approval browser action V1</title><style>body{margin:0;background:#f3f1ed;color:#252321;font:14px/1.5 Inter,system-ui,sans-serif}header{padding:32px 4vw 20px}h1{font:600 34px/1.1 Georgia,serif;margin:0 0 10px}header p{max-width:850px;color:#69645e}main{display:grid;gap:24px;padding:0 4vw 48px}article{overflow:hidden;border:1px solid #d9d4cc;border-radius:18px;background:#fff;box-shadow:0 10px 35px #544a3b12}article div{padding:16px 20px;border-bottom:1px solid #e6e1da}article p{margin:4px 0 0;color:#716b64}img{display:block;width:100%;height:auto}</style></head><body><header><h1>Governed Approval · browser-to-Store proof</h1><p>Requested → invalid capability denied → Human approved or rejected. The decision changes only the Approval; the ¥3,000 Commitment remains pending and Payment remains absent.</p></header><main>${cards}</main></body></html>`;
  await writeFile(join(outputRoot, "review.html"), html);
  console.log(JSON.stringify({ status: "materialized", states: states.length, gallery: join(outputRoot, "review.html") }, null, 2));
}

main().catch((error) => { console.error(error.stack || error.message); process.exit(1); });
