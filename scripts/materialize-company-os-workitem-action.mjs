#!/usr/bin/env node

import { createHash } from "node:crypto";
import { cp, mkdir, readFile, rm, writeFile } from "node:fs/promises";
import { dirname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const outputRoot = join(repoRoot, "docs/design/company-os-v2/workitem-action-v1");

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

async function main() {
  const capturePath = argument("--capture-manifest");
  if (!capturePath) throw new Error("--capture-manifest is required");
  const absolute = resolve(capturePath);
  const capture = JSON.parse(await readFile(absolute, "utf8"));
  const action = capture.work_item_action;
  if (capture.status !== "passed" || capture.data_mode !== "store-live" || action?.status !== "completed") {
    throw new Error("capture is not a passing completed Store-live WorkItem action run");
  }
  if (action.payment_count !== 0 || action.idempotent_replay !== true || capture.approval_action?.approval_status !== "approved") {
    throw new Error("capture violates the governed WorkItem completion boundary");
  }
  await rm(outputRoot, { recursive: true, force: true });
  await mkdir(outputRoot, { recursive: true });
  const sources = [
    { id: "waiting", title: "1 · Waiting for approval", caption: "Preparation may start, but completion remains governed.", source: action.waiting.file },
    { id: "in-progress", title: "2 · Standing Agent working", caption: "The explicit assignee started preparation through work_item.transition.", source: action.in_progress.file },
    { id: "in-review", title: "3 · Result submitted", caption: "Durable outcome and evidence are ready; completion is blocked until Approval.", source: action.in_review.file },
    { id: "completed", title: "4 · Accountable owner completed", caption: "After Human Approval, the owner accepted the result. No Payment was created.", source: action.completed.file },
  ];
  const states = [];
  for (const item of sources) {
    const destination = join(outputRoot, `${item.id}--desktop-1536x1024.png`);
    await cp(join(repoRoot, item.source), destination);
    states.push({ ...item, file: relative(outputRoot, destination), sha256: await sha256(destination) });
  }
  const manifest = {
    contract: "company-os-workitem-browser-action-v1",
    status: "human_review_pending",
    source_capture: relative(repoRoot, absolute),
    assertions: [
      "the named Standing Agent starts and submits the WorkItem",
      "result, evidence, responsibility, and source provenance survive every transition",
      "completion remains unavailable until every linked Approval is approved",
      "the accountable Human completes the reviewed WorkItem",
      "the exact completion request is idempotent",
      "no Payment is created by a WorkItem transition",
      "the browser capability is not persisted or copied into evidence",
    ],
    work_item_action: action,
    approval_action: {
      approval_id: capture.approval_action.approval_id,
      approval_status: capture.approval_action.approval_status,
      decided_by: capture.approval_action.decided_by,
      commitment_status: capture.approval_action.commitment_status,
      payment_count: capture.approval_action.payment_count,
    },
    states,
  };
  await writeFile(join(outputRoot, "manifest.json"), `${JSON.stringify(manifest, null, 2)}\n`);
  const cards = states.map((state) => `<article><div><strong>${escapeHtml(state.title)}</strong><p>${escapeHtml(state.caption)}</p></div><img src="${escapeHtml(state.file)}" alt="${escapeHtml(state.title)}" /></article>`).join("\n");
  const html = `<!doctype html><html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>Company OS WorkItem browser action V1</title><style>body{margin:0;background:#f3f1ed;color:#252321;font:14px/1.5 Inter,system-ui,sans-serif}header{padding:32px 4vw 20px}h1{font:600 34px/1.1 Georgia,serif;margin:0 0 10px}header p{max-width:900px;color:#69645e}main{display:grid;gap:24px;padding:0 4vw 48px}article{overflow:hidden;border:1px solid #d9d4cc;border-radius:18px;background:#fff;box-shadow:0 10px 35px #544a3b12}article div{padding:16px 20px;border-bottom:1px solid #e6e1da}article p{margin:4px 0 0;color:#716b64}img{display:block;width:100%;height:auto}</style></head><body><header><h1>Governed WorkItem · browser-to-Store proof</h1><p>Waiting → in progress → in review → Human Approval → completed. Responsibility and result provenance remain linked, while Payment remains a separate explicit action.</p></header><main>${cards}</main></body></html>`;
  await writeFile(join(outputRoot, "review.html"), html);
  console.log(JSON.stringify({ status: "materialized", states: states.length, gallery: join(outputRoot, "review.html") }, null, 2));
}

main().catch((error) => { console.error(error.stack || error.message); process.exit(1); });
