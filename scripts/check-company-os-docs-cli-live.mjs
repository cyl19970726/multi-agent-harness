#!/usr/bin/env node

import { execFileSync, spawn } from "node:child_process";
import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { createServer } from "node:net";

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const harness = join(repoRoot, "target", "debug", "harness");
const NOW = "2026-07-23T09:00:00+08:00";
const token = "docs-cli-live-token";

function freePort() {
  return new Promise((resolvePort, reject) => {
    const server = createServer();
    server.once("error", reject);
    server.listen(0, "127.0.0.1", () => {
      const address = server.address();
      server.close((error) => error ? reject(error) : resolvePort(address.port));
    });
  });
}

async function waitFor(url) {
  const deadline = Date.now() + 30_000;
  let lastError;
  while (Date.now() < deadline) {
    try {
      const response = await fetch(url);
      if (response.ok) return;
      lastError = new Error(`HTTP ${response.status}`);
    } catch (error) {
      lastError = error;
    }
    await new Promise((resolveWait) => setTimeout(resolveWait, 200));
  }
  throw new Error(`server did not become ready: ${lastError?.message ?? "timeout"}`);
}

async function post(base, path, body) {
  const response = await fetch(`${base}${path}`, {
    method: "POST",
    headers: { "content-type": "application/json", "x-harness-company-os-token": token },
    body: JSON.stringify(body),
  });
  const data = await response.json();
  if (!response.ok || data.ok === false) {
    throw new Error(`${path} failed: HTTP ${response.status} ${JSON.stringify(data)}`);
  }
  return data.result ?? data;
}

async function get(base, path) {
  const response = await fetch(`${base}${path}`, { headers: { accept: "application/json" } });
  const data = await response.json();
  if (!response.ok) throw new Error(`${path} failed: HTTP ${response.status} ${JSON.stringify(data)}`);
  return data.result ?? data;
}

function admin(record) {
  return {
    mode: "administrative",
    authority: { actor_type: "human", actor_id: "human-docs-owner" },
    record,
  };
}

async function main() {
  execFileSync("cargo", ["build", "-p", "harness-cli"], { cwd: repoRoot, stdio: "inherit" });
  const root = await mkdtemp(join(tmpdir(), "company-os-docs-cli-live-"));
  const storeRoot = join(root, "store");
  const port = await freePort();
  const base = `http://127.0.0.1:${port}`;
  const env = { ...process.env, HARNESS_ROOT: storeRoot, HARNESS_COMPANY_OS_TOKEN: token };
  const server = spawn(harness, ["serve", "--addr", `127.0.0.1:${port}`, "--no-truncate"], {
    cwd: repoRoot,
    env,
    stdio: ["ignore", "pipe", "pipe"],
  });
  const logs = [];
  server.stdout.on("data", (chunk) => logs.push(chunk.toString()));
  server.stderr.on("data", (chunk) => logs.push(chunk.toString()));
  try {
    await waitFor(`${base}/health`);
    await post(base, "/v1/company-os/actors", {
      actor_type: "human",
      actor: {
        id: "human-docs-owner",
        display_name: "Docs Owner",
        title: "Owner",
        status: "active",
        availability: "available",
        membership_refs: [],
        responsibility_summary: "Owns Docs CLI acceptance.",
        permission_policy_refs: ["company_os.admin", "company.records.write"],
        authority_policy_refs: ["company_os.admin"],
        created_at: NOW,
        updated_at: NOW,
      },
    });
    await post(base, "/v1/company-os/actors", admin({
      actor_type: "agent",
      actor: {
        id: "agent-docs-governance",
        display_name: "Docs Governance Agent",
        role: "Docs governance",
        status: "active",
        availability: "available",
        assignment_capacity: 4,
        exclusive_assignment_ref: null,
        home_org_unit_ref: null,
        membership_refs: [],
        responsibility_summary: "Maintains document structure.",
        capability_refs: ["company.records.write"],
        permission_policy_refs: ["company.records.write"],
        runtime_refs: [],
        provider_session_refs: [],
        created_at: NOW,
        updated_at: NOW,
      },
    }));
    await post(base, "/v1/company-os/documents", admin({
      id: "document-root",
      space_id: "company",
      parent_document_id: null,
      title: "Company Root",
      kind: "page",
      lifecycle_status: "active",
      block_ids: [],
      template_ref: null,
      permission_policy_refs: ["company.records.write"],
      reference_refs: [],
      created_by: { actor_type: "human", actor_id: "human-docs-owner" },
      updated_by: { actor_type: "human", actor_id: "human-docs-owner" },
      created_at: NOW,
      updated_at: NOW,
    }));
    await post(base, "/v1/company-os/documents", admin({
      id: "template-cli-child",
      space_id: "company",
      parent_document_id: "document-root",
      title: "CLI child template",
      kind: "template",
      lifecycle_status: "active",
      block_ids: [],
      template_ref: null,
      permission_policy_refs: ["company.records.write"],
      reference_refs: [],
      created_by: { actor_type: "human", actor_id: "human-docs-owner" },
      updated_by: { actor_type: "human", actor_id: "human-docs-owner" },
      created_at: NOW,
      updated_at: NOW,
    }));
    await post(base, "/v1/company-os/blocks", admin({
      id: "block-template-cli-1",
      document_id: "template-cli-child",
      kind: "callout",
      position: 0,
      content: { title: "Template note", text: "Copied from a template by Docs CLI live acceptance.", tone: "neutral" },
      referenced_entities: [],
      created_by: { actor_type: "human", actor_id: "human-docs-owner" },
      updated_by: { actor_type: "human", actor_id: "human-docs-owner" },
      created_at: NOW,
      updated_at: NOW,
    }));
    await post(base, "/v1/company-os/documents", admin({
      id: "template-cli-child",
      space_id: "company",
      parent_document_id: "document-root",
      title: "CLI child template",
      kind: "template",
      lifecycle_status: "active",
      block_ids: ["block-template-cli-1"],
      template_ref: null,
      permission_policy_refs: ["company.records.write"],
      reference_refs: [],
      created_by: { actor_type: "human", actor_id: "human-docs-owner" },
      updated_by: { actor_type: "human", actor_id: "human-docs-owner" },
      created_at: NOW,
      updated_at: NOW,
    }));
    const cliEnv = { ...env, HARNESS_ROOT: storeRoot, HARNESS_COMPANY_OS_TOKEN: token };
    const module = JSON.parse(execFileSync(harness, [
      "company", "docs", "module", "create",
      "--id", "module-docs-cli",
      "--root-document", "document-root",
      "--name", "Docs CLI",
      "--purpose", "Acceptance module for Docs CLI primitives.",
      "--record-type", "acceptance",
      "--relation-rule-json", "{\"relation_type\":\"source_for\",\"from_kind\":\"document\",\"to_kind\":\"typed_record\",\"required\":true,\"cross_module\":false}",
      "--default-view-id", "view-docs-cli",
      "--default-view-title", "Docs CLI fallback",
      "--authority", "human-docs-owner",
    ], { cwd: repoRoot, env: cliEnv, encoding: "utf8" }));
    if (module.ok !== true || module.result?.module_id !== "module-docs-cli" || module.result?.default_view_id !== "view-docs-cli") {
      throw new Error(`module create did not return expected ids: ${JSON.stringify(module)}`);
    }

    const definition = JSON.parse(execFileSync(harness, [
      "company", "docs", "page-definition", "create",
      "--id", "page-docs-cli",
      "--module", "module-docs-cli",
      "--fallback-view", "view-docs-cli",
      "--purpose", "Declare scoped Docs CLI authoring commands.",
      "--package-id", "package-docs-cli",
      "--fixture-ref", "docs-cli-live",
      "--visual-contract-ref", "docs-cli-live",
      "--authority", "human-docs-owner",
      "--owner", "human-docs-owner",
      "--component", "DocumentEditor",
    ], { cwd: repoRoot, env: cliEnv, encoding: "utf8" }));
    if (definition.ok !== true || definition.result?.definition_id !== "page-docs-cli" || definition.result?.package_id !== "package-docs-cli") {
      throw new Error(`page-definition create did not return expected ids: ${JSON.stringify(definition)}`);
    }

    const scaffoldedPage = JSON.parse(execFileSync(harness, [
      "company", "docs", "page", "scaffold",
      "--id", "page-docs-cli-custom",
      "--module", "module-docs-cli",
      "--fallback-view", "view-docs-cli",
      "--title", "Docs CLI Custom Console",
      "--authority", "human-docs-owner",
      "--artifact-ref", "apps/agent-dashboard/src/company-os/modules/docs-cli/DocsCliCustomConsole.tsx",
      "--visual-contract-ref", "docs/design/company-os/docs-cli-custom/review.html",
    ], { cwd: repoRoot, env: cliEnv, encoding: "utf8" }));
    if (scaffoldedPage.ok !== true || scaffoldedPage.result?.definition_id !== "page-docs-cli-custom") {
      throw new Error(`page scaffold did not create a code-declared page contract: ${JSON.stringify(scaffoldedPage)}`);
    }

    const pagePublish = JSON.parse(execFileSync(harness, [
      "company", "docs", "page", "publish",
      "--definition", "page-docs-cli-custom",
      "--version", "1.0.1",
      "--artifact-ref", "apps/agent-dashboard/src/company-os/modules/docs-cli/DocsCliCustomConsole.tsx",
      "--authority", "human-docs-owner",
    ], { cwd: repoRoot, env: cliEnv, encoding: "utf8" }));
    if (pagePublish.ok !== true || pagePublish.result?.package_version !== "1.0.1") {
      throw new Error(`page publish did not update package metadata: ${JSON.stringify(pagePublish)}`);
    }

    const pageVerify = JSON.parse(execFileSync(harness, [
      "company", "docs", "page", "verify",
      "--definition", "page-docs-cli-custom",
    ], { cwd: repoRoot, env: cliEnv, encoding: "utf8" }));
    if (pageVerify.ok !== true || pageVerify.checks?.module_exists !== true || pageVerify.boundaries?.page_is_not_second_truth !== true) {
      throw new Error(`page verify did not validate the code-declared page boundary: ${JSON.stringify(pageVerify)}`);
    }

    const created = JSON.parse(execFileSync(harness, [
      "company", "docs", "document", "create",
      "--definition", "page-docs-cli",
      "--parent-document", "document-root",
      "--id", "document-cli-child",
      "--title", "CLI Child",
      "--template", "template-cli-child",
      "--instantiate-template",
      "--actor", "agent-docs-governance",
    ], { cwd: repoRoot, env: cliEnv, encoding: "utf8" }));
    if (created.ok !== true) throw new Error(`document create did not return ok: ${JSON.stringify(created)}`);

    const structureParent = JSON.parse(execFileSync(harness, [
      "company", "docs", "document", "create",
      "--definition", "page-docs-cli",
      "--parent-document", "document-root",
      "--id", "document-cli-structure-parent",
      "--title", "CLI Structure Folder",
      "--actor", "agent-docs-governance",
    ], { cwd: repoRoot, env: cliEnv, encoding: "utf8" }));
    if (structureParent.ok !== true) throw new Error(`structure parent create did not return ok: ${JSON.stringify(structureParent)}`);

    const block = JSON.parse(execFileSync(harness, [
      "company", "docs", "block", "append",
      "--definition", "page-docs-cli",
      "--document", "document-cli-child",
      "--id", "block-cli-child-1",
      "--kind", "callout",
      "--content-json", "{\"title\":\"CLI acceptance note\",\"text\":\"Created by Docs CLI live acceptance.\",\"tone\":\"success\"}",
      "--actor", "agent-docs-governance",
    ], { cwd: repoRoot, env: cliEnv, encoding: "utf8" }));
    if (block.ok !== true) throw new Error(`block append did not return ok: ${JSON.stringify(block)}`);

    const secondBlock = JSON.parse(execFileSync(harness, [
      "company", "docs", "block", "append",
      "--definition", "page-docs-cli",
      "--document", "document-cli-child",
      "--id", "block-cli-child-2",
      "--kind", "heading",
      "--text", "Second CLI block",
      "--actor", "agent-docs-governance",
    ], { cwd: repoRoot, env: cliEnv, encoding: "utf8" }));
    if (secondBlock.ok !== true) throw new Error(`second block append did not return ok: ${JSON.stringify(secondBlock)}`);

    const reordered = JSON.parse(execFileSync(harness, [
      "company", "docs", "block", "reorder",
      "--definition", "page-docs-cli",
      "--document", "document-cli-child",
      "--block-order", "block-cli-child-2,block-cli-child-1,block-cli-template-document-cli-child-1-block-template-cli-1",
      "--actor", "agent-docs-governance",
    ], { cwd: repoRoot, env: cliEnv, encoding: "utf8" }));
    if (reordered.ok !== true) throw new Error(`block reorder did not return ok: ${JSON.stringify(reordered)}`);

    const blockUpdateDryRun = JSON.parse(execFileSync(harness, [
      "company", "docs", "block", "update",
      "--definition", "page-docs-cli",
      "--document", "document-cli-child",
      "--block", "block-cli-child-1",
      "--content-json", "{\"title\":\"CLI acceptance note updated\",\"text\":\"Updated by Docs CLI live acceptance.\",\"tone\":\"success\"}",
      "--actor", "agent-docs-governance",
      "--dry-run",
    ], { cwd: repoRoot, env: cliEnv, encoding: "utf8" }));
    if (blockUpdateDryRun.ok !== true || blockUpdateDryRun.dry_run !== true || !blockUpdateDryRun.effects?.includes("block.append") || blockUpdateDryRun.boundaries?.dry_run_does_not_dispatch !== true) {
      throw new Error(`block update dry-run did not preserve dry-run boundary: ${JSON.stringify(blockUpdateDryRun)}`);
    }

    const blockUpdate = JSON.parse(execFileSync(harness, [
      "company", "docs", "block", "update",
      "--definition", "page-docs-cli",
      "--document", "document-cli-child",
      "--block", "block-cli-child-1",
      "--content-json", "{\"title\":\"CLI acceptance note updated\",\"text\":\"Updated by Docs CLI live acceptance.\",\"tone\":\"success\"}",
      "--actor", "agent-docs-governance",
    ], { cwd: repoRoot, env: cliEnv, encoding: "utf8" }));
    if (blockUpdate.ok !== true || blockUpdate.result?.operation !== "update" || blockUpdate.result?.block_id !== "block-cli-child-1") {
      throw new Error(`block update did not return expected result: ${JSON.stringify(blockUpdate)}`);
    }

    const reusableTemplate = JSON.parse(execFileSync(harness, [
      "company", "docs", "template", "create",
      "--definition", "page-docs-cli",
      "--parent-document", "document-root",
      "--id", "template-cli-reusable",
      "--title", "Reusable CLI template",
      "--from-document", "document-cli-child",
      "--actor", "agent-docs-governance",
    ], { cwd: repoRoot, env: cliEnv, encoding: "utf8" }));
    if (reusableTemplate.ok !== true || reusableTemplate.result?.template_id !== "template-cli-reusable" || reusableTemplate.result?.block_copy?.copied_block_count !== 3) {
      throw new Error(`template create did not create reusable template with copied Blocks: ${JSON.stringify(reusableTemplate)}`);
    }

    const templateStatus = JSON.parse(execFileSync(harness, [
      "company", "docs", "template", "status",
      "--definition", "page-docs-cli",
      "--template", "template-cli-reusable",
      "--status", "archived",
      "--actor", "agent-docs-governance",
    ], { cwd: repoRoot, env: cliEnv, encoding: "utf8" }));
    if (templateStatus.ok !== true || templateStatus.result?.template_id !== "template-cli-reusable" || templateStatus.result?.lifecycle_status !== "archived") {
      throw new Error(`template status did not return archived template lifecycle: ${JSON.stringify(templateStatus)}`);
    }

    const typedRecord = JSON.parse(execFileSync(harness, [
      "company", "docs", "typed-record", "append",
      "--definition", "page-docs-cli",
      "--module", "module-docs-cli",
      "--source-document", "document-cli-child",
      "--id", "typed-record-cli-child-1",
      "--record-type", "acceptance",
      "--title", "CLI Typed Record",
      "--fields-json", "{\"status\":\"draft\",\"source\":\"cli-live\"}",
      "--actor", "agent-docs-governance",
    ], { cwd: repoRoot, env: cliEnv, encoding: "utf8" }));
    if (typedRecord.ok !== true) throw new Error(`typed-record append did not return ok: ${JSON.stringify(typedRecord)}`);

    const typedRecordUpdateDryRun = JSON.parse(execFileSync(harness, [
      "company", "docs", "typed-record", "update",
      "--definition", "page-docs-cli",
      "--record", "typed-record-cli-child-1",
      "--title", "CLI Typed Record Updated",
      "--fields-json", "{\"status\":\"accepted\",\"reviewed\":true}",
      "--merge-fields",
      "--actor", "agent-docs-governance",
      "--dry-run",
    ], { cwd: repoRoot, env: cliEnv, encoding: "utf8" }));
    if (typedRecordUpdateDryRun.ok !== true || typedRecordUpdateDryRun.dry_run !== true || typedRecordUpdateDryRun.after?.fields?.source !== "cli-live" || typedRecordUpdateDryRun.after?.fields?.status !== "accepted") {
      throw new Error(`typed-record update dry-run did not merge fields without dispatch: ${JSON.stringify(typedRecordUpdateDryRun)}`);
    }

    const typedRecordUpdate = JSON.parse(execFileSync(harness, [
      "company", "docs", "typed-record", "update",
      "--definition", "page-docs-cli",
      "--record", "typed-record-cli-child-1",
      "--title", "CLI Typed Record Updated",
      "--fields-json", "{\"status\":\"accepted\",\"reviewed\":true}",
      "--merge-fields",
      "--actor", "agent-docs-governance",
    ], { cwd: repoRoot, env: cliEnv, encoding: "utf8" }));
    if (typedRecordUpdate.ok !== true || typedRecordUpdate.result?.operation !== "update" || typedRecordUpdate.result?.fields?.status !== "accepted") {
      throw new Error(`typed-record update did not return expected result: ${JSON.stringify(typedRecordUpdate)}`);
    }

    const typedRecordValidation = JSON.parse(execFileSync(harness, [
      "company", "docs", "typed-record", "validate",
      "--record", "typed-record-cli-child-1",
      "--schema-json", "{\"required\":[\"status\",\"source\"],\"properties\":{\"status\":{\"type\":\"string\"},\"reviewed\":{\"type\":\"boolean\"}}}",
    ], { cwd: repoRoot, env: cliEnv, encoding: "utf8" }));
    if (typedRecordValidation.ok !== true || typedRecordValidation.boundaries?.validate_does_not_dispatch !== true) {
      throw new Error(`typed-record validate did not pass read-only schema validation: ${JSON.stringify(typedRecordValidation)}`);
    }

    const relation = JSON.parse(execFileSync(harness, [
      "company", "docs", "relation", "link",
      "--definition", "page-docs-cli",
      "--from-document", "document-cli-child",
      "--to-record", "typed-record-cli-child-1",
      "--relation-id", "relation-cli-child-to-record",
      "--actor", "agent-docs-governance",
    ], { cwd: repoRoot, env: cliEnv, encoding: "utf8" }));
    if (relation.ok !== true) throw new Error(`relation link did not return ok: ${JSON.stringify(relation)}`);

    const view = JSON.parse(execFileSync(harness, [
      "company", "docs", "view", "create",
      "--definition", "page-docs-cli",
      "--module", "module-docs-cli",
      "--id", "view-cli-child-records",
      "--title", "CLI Records",
      "--source-kind", "typed_record",
      "--query-json", "{\"record_type\":\"acceptance\"}",
      "--actor", "agent-docs-governance",
    ], { cwd: repoRoot, env: cliEnv, encoding: "utf8" }));
    if (view.ok !== true) throw new Error(`view create did not return ok: ${JSON.stringify(view)}`);

    const viewUpdateDryRun = JSON.parse(execFileSync(harness, [
      "company", "docs", "view", "update",
      "--definition", "page-docs-cli",
      "--view", "view-cli-child-records",
      "--query-json", "{\"record_type\":\"acceptance\",\"group_by\":\"lifecycle_status\"}",
      "--actor", "agent-docs-governance",
      "--dry-run",
    ], { cwd: repoRoot, env: cliEnv, encoding: "utf8" }));
    if (viewUpdateDryRun.ok !== true || viewUpdateDryRun.dry_run !== true || viewUpdateDryRun.boundaries?.view_is_presentation_truth_not_record_store !== true) {
      throw new Error(`view update dry-run did not preserve presentation boundary: ${JSON.stringify(viewUpdateDryRun)}`);
    }

    const viewUpdate = JSON.parse(execFileSync(harness, [
      "company", "docs", "view", "update",
      "--definition", "page-docs-cli",
      "--view", "view-cli-child-records",
      "--query-json", "{\"record_type\":\"acceptance\",\"group_by\":\"lifecycle_status\"}",
      "--actor", "agent-docs-governance",
    ], { cwd: repoRoot, env: cliEnv, encoding: "utf8" }));
    if (viewUpdate.ok !== true || viewUpdate.result?.operation !== "update" || viewUpdate.result?.query?.group_by !== "lifecycle_status") {
      throw new Error(`view update did not return expected query config: ${JSON.stringify(viewUpdate)}`);
    }

    const removableBlock = JSON.parse(execFileSync(harness, [
      "company", "docs", "block", "append",
      "--definition", "page-docs-cli",
      "--document", "document-cli-child",
      "--id", "block-cli-child-remove",
      "--kind", "callout",
      "--text", "Remove me from the visible document order.",
      "--actor", "agent-docs-governance",
    ], { cwd: repoRoot, env: cliEnv, encoding: "utf8" }));
    if (removableBlock.ok !== true) throw new Error(`removable block append did not return ok: ${JSON.stringify(removableBlock)}`);

    const blockRemoveDryRun = JSON.parse(execFileSync(harness, [
      "company", "docs", "block", "remove",
      "--definition", "page-docs-cli",
      "--document", "document-cli-child",
      "--block", "block-cli-child-remove",
      "--actor", "agent-docs-governance",
      "--dry-run",
    ], { cwd: repoRoot, env: cliEnv, encoding: "utf8" }));
    if (blockRemoveDryRun.ok !== true || blockRemoveDryRun.dry_run !== true || !blockRemoveDryRun.effects?.includes("document.append") || blockRemoveDryRun.boundaries?.physical_delete !== false) {
      throw new Error(`block remove dry-run did not preserve no-delete boundary: ${JSON.stringify(blockRemoveDryRun)}`);
    }

    let blockRemoveWithoutConfirmFailed = false;
    try {
      execFileSync(harness, [
        "company", "docs", "block", "remove",
        "--definition", "page-docs-cli",
        "--document", "document-cli-child",
        "--block", "block-cli-child-remove",
        "--actor", "agent-docs-governance",
      ], { cwd: repoRoot, env: cliEnv, encoding: "utf8", stdio: "pipe" });
    } catch {
      blockRemoveWithoutConfirmFailed = true;
    }
    if (!blockRemoveWithoutConfirmFailed) throw new Error("block remove succeeded without --confirm");

    const blockRemove = JSON.parse(execFileSync(harness, [
      "company", "docs", "block", "remove",
      "--definition", "page-docs-cli",
      "--document", "document-cli-child",
      "--block", "block-cli-child-remove",
      "--actor", "agent-docs-governance",
      "--confirm",
    ], { cwd: repoRoot, env: cliEnv, encoding: "utf8" }));
    if (blockRemove.ok !== true || blockRemove.result?.operation !== "remove" || blockRemove.result?.block_action !== null || blockRemove.result?.block_ids?.includes("block-cli-child-remove")) {
      throw new Error(`block remove did not remove the Block from Document.block_ids without a Block write: ${JSON.stringify(blockRemove)}`);
    }

    const archivalBlock = JSON.parse(execFileSync(harness, [
      "company", "docs", "block", "append",
      "--definition", "page-docs-cli",
      "--document", "document-cli-child",
      "--id", "block-cli-child-archive",
      "--kind", "callout",
      "--text", "Archive me and remove me from the visible document order.",
      "--actor", "agent-docs-governance",
    ], { cwd: repoRoot, env: cliEnv, encoding: "utf8" }));
    if (archivalBlock.ok !== true) throw new Error(`archival block append did not return ok: ${JSON.stringify(archivalBlock)}`);

    const blockArchiveDryRun = JSON.parse(execFileSync(harness, [
      "company", "docs", "block", "archive",
      "--definition", "page-docs-cli",
      "--document", "document-cli-child",
      "--block", "block-cli-child-archive",
      "--actor", "agent-docs-governance",
      "--dry-run",
    ], { cwd: repoRoot, env: cliEnv, encoding: "utf8" }));
    if (blockArchiveDryRun.ok !== true || blockArchiveDryRun.dry_run !== true || blockArchiveDryRun.after?.block?.content?._archived !== true || blockArchiveDryRun.boundaries?.physical_delete !== false) {
      throw new Error(`block archive dry-run did not show archive metadata and no-delete boundary: ${JSON.stringify(blockArchiveDryRun)}`);
    }

    let blockArchiveWithoutConfirmFailed = false;
    try {
      execFileSync(harness, [
        "company", "docs", "block", "archive",
        "--definition", "page-docs-cli",
        "--document", "document-cli-child",
        "--block", "block-cli-child-archive",
        "--actor", "agent-docs-governance",
      ], { cwd: repoRoot, env: cliEnv, encoding: "utf8", stdio: "pipe" });
    } catch {
      blockArchiveWithoutConfirmFailed = true;
    }
    if (!blockArchiveWithoutConfirmFailed) throw new Error("block archive succeeded without --confirm");

    const blockArchive = JSON.parse(execFileSync(harness, [
      "company", "docs", "block", "archive",
      "--definition", "page-docs-cli",
      "--document", "document-cli-child",
      "--block", "block-cli-child-archive",
      "--actor", "agent-docs-governance",
      "--confirm",
    ], { cwd: repoRoot, env: cliEnv, encoding: "utf8" }));
    if (blockArchive.ok !== true || blockArchive.result?.operation !== "archive" || blockArchive.result?.block_ids?.includes("block-cli-child-archive")) {
      throw new Error(`block archive did not archive and remove the Block from Document.block_ids: ${JSON.stringify(blockArchive)}`);
    }

    const renameDryRun = JSON.parse(execFileSync(harness, [
      "company", "docs", "document", "rename",
      "--definition", "page-docs-cli",
      "--document", "document-cli-child",
      "--title", "CLI Child Renamed",
      "--actor", "agent-docs-governance",
      "--dry-run",
    ], { cwd: repoRoot, env: cliEnv, encoding: "utf8" }));
    if (renameDryRun.ok !== true || renameDryRun.dry_run !== true || renameDryRun.effect !== "document.append" || renameDryRun.boundaries?.dry_run_does_not_dispatch !== true) {
      throw new Error(`document rename dry-run did not preserve dry-run boundary: ${JSON.stringify(renameDryRun)}`);
    }

    const renamed = JSON.parse(execFileSync(harness, [
      "company", "docs", "document", "rename",
      "--definition", "page-docs-cli",
      "--document", "document-cli-child",
      "--title", "CLI Child Renamed",
      "--actor", "agent-docs-governance",
    ], { cwd: repoRoot, env: cliEnv, encoding: "utf8" }));
    if (renamed.ok !== true || renamed.result?.operation !== "rename" || renamed.result?.title !== "CLI Child Renamed") {
      throw new Error(`document rename did not return expected result: ${JSON.stringify(renamed)}`);
    }

    const moveDryRun = JSON.parse(execFileSync(harness, [
      "company", "docs", "document", "move",
      "--definition", "page-docs-cli",
      "--document", "document-cli-child",
      "--parent-document", "document-cli-structure-parent",
      "--actor", "agent-docs-governance",
      "--dry-run",
    ], { cwd: repoRoot, env: cliEnv, encoding: "utf8" }));
    if (moveDryRun.ok !== true || moveDryRun.dry_run !== true || moveDryRun.after?.parent_document_id !== "document-cli-structure-parent") {
      throw new Error(`document move dry-run did not show the new parent without dispatch: ${JSON.stringify(moveDryRun)}`);
    }

    let selfMoveFailed = false;
    try {
      execFileSync(harness, [
        "company", "docs", "document", "move",
        "--definition", "page-docs-cli",
        "--document", "document-cli-child",
        "--parent-document", "document-cli-child",
        "--actor", "agent-docs-governance",
      ], { cwd: repoRoot, env: cliEnv, encoding: "utf8", stdio: "pipe" });
    } catch {
      selfMoveFailed = true;
    }
    if (!selfMoveFailed) throw new Error("document move allowed a self-parent cycle");

    const moved = JSON.parse(execFileSync(harness, [
      "company", "docs", "document", "move",
      "--definition", "page-docs-cli",
      "--document", "document-cli-child",
      "--parent-document", "document-cli-structure-parent",
      "--actor", "agent-docs-governance",
    ], { cwd: repoRoot, env: cliEnv, encoding: "utf8" }));
    if (moved.ok !== true || moved.result?.operation !== "move" || moved.result?.parent_document_id !== "document-cli-structure-parent") {
      throw new Error(`document move did not return expected result: ${JSON.stringify(moved)}`);
    }

    let archiveWithoutConfirmFailed = false;
    try {
      execFileSync(harness, [
        "company", "docs", "document", "archive",
        "--definition", "page-docs-cli",
        "--document", "document-cli-child",
        "--actor", "agent-docs-governance",
      ], { cwd: repoRoot, env: cliEnv, encoding: "utf8", stdio: "pipe" });
    } catch {
      archiveWithoutConfirmFailed = true;
    }
    if (!archiveWithoutConfirmFailed) throw new Error("document archive succeeded without --confirm");

    const archiveDryRun = JSON.parse(execFileSync(harness, [
      "company", "docs", "document", "archive",
      "--definition", "page-docs-cli",
      "--document", "document-cli-child",
      "--actor", "agent-docs-governance",
      "--dry-run",
    ], { cwd: repoRoot, env: cliEnv, encoding: "utf8" }));
    if (archiveDryRun.ok !== true || archiveDryRun.dry_run !== true || archiveDryRun.after?.lifecycle_status !== "archived") {
      throw new Error(`document archive dry-run did not show archived lifecycle: ${JSON.stringify(archiveDryRun)}`);
    }

    const archived = JSON.parse(execFileSync(harness, [
      "company", "docs", "document", "archive",
      "--definition", "page-docs-cli",
      "--document", "document-cli-child",
      "--actor", "agent-docs-governance",
      "--confirm",
    ], { cwd: repoRoot, env: cliEnv, encoding: "utf8" }));
    if (archived.ok !== true || archived.result?.operation !== "archive" || archived.result?.lifecycle_status !== "archived") {
      throw new Error(`document archive did not return archived lifecycle: ${JSON.stringify(archived)}`);
    }

    const documentQuery = JSON.parse(execFileSync(harness, [
      "company", "docs", "query",
      "--document", "document-cli-child",
    ], { cwd: repoRoot, env: cliEnv, encoding: "utf8" }));
    if (documentQuery.command !== "harness company docs query" || documentQuery.source !== "latest_projection") {
      throw new Error(`docs query did not report the expected read source: ${JSON.stringify(documentQuery)}`);
    }
    if (documentQuery.document?.id !== "document-cli-child") {
      throw new Error(`docs query did not return the selected Document: ${JSON.stringify(documentQuery.document)}`);
    }
    if (documentQuery.document?.title !== "CLI Child Renamed" || documentQuery.document?.parent_document_id !== "document-cli-structure-parent" || documentQuery.document?.lifecycle_status !== "archived") {
      throw new Error(`docs query did not return latest structure-maintained Document row: ${JSON.stringify(documentQuery.document)}`);
    }
    if (JSON.stringify(documentQuery.blocks?.map((block) => block.id)) !== JSON.stringify(["block-cli-child-2", "block-cli-child-1", "block-cli-template-document-cli-child-1-block-template-cli-1"])) {
      throw new Error(`docs query did not return ordered Blocks from Document.block_ids: ${JSON.stringify(documentQuery.blocks?.map((block) => block.id))}`);
    }
    if (!documentQuery.typed_records?.some((record) => record.id === "typed-record-cli-child-1")) {
      throw new Error(`docs query did not include the source-linked TypedRecord: ${JSON.stringify(documentQuery.typed_records)}`);
    }
    if (!documentQuery.relations?.some((entry) => entry.id === "relation-cli-child-to-record")) {
      throw new Error(`docs query did not include the Document-to-TypedRecord Relation: ${JSON.stringify(documentQuery.relations)}`);
    }
    if (documentQuery.boundaries?.read_model !== "latest_projection" || documentQuery.boundaries?.future_sql_role !== "derived_read_query_index_layer" || documentQuery.boundaries?.work_side_effects !== false || documentQuery.boundaries?.finance_side_effects !== false) {
      throw new Error(`docs query did not preserve read-only boundary metadata: ${JSON.stringify(documentQuery.boundaries)}`);
    }

    const searchResult = JSON.parse(execFileSync(harness, [
      "company", "docs", "search",
      "--query", "CLI",
      "--module", "module-docs-cli",
    ], { cwd: repoRoot, env: cliEnv, encoding: "utf8" }));
    if (searchResult.boundaries?.read_only !== true || !searchResult.matches?.some((entry) => entry.id === "document-cli-child")) {
      throw new Error(`docs search did not return projection-backed context: ${JSON.stringify(searchResult)}`);
    }

    const traversed = JSON.parse(execFileSync(harness, [
      "company", "docs", "traverse",
      "--document", "document-root",
      "--depth", "2",
    ], { cwd: repoRoot, env: cliEnv, encoding: "utf8" }));
    if (traversed.boundaries?.read_only !== true || traversed.tree?.document?.id !== "document-root" || !traversed.tree?.children?.some((entry) => entry.document?.id === "document-cli-structure-parent")) {
      throw new Error(`docs traverse did not return bounded Document tree: ${JSON.stringify(traversed)}`);
    }

    const refs = JSON.parse(execFileSync(harness, [
      "company", "docs", "refs",
      "--document", "document-cli-child",
    ], { cwd: repoRoot, env: cliEnv, encoding: "utf8" }));
    if (refs.boundaries?.read_only !== true || !refs.refs?.relations?.some((entry) => entry.id === "relation-cli-child-to-record")) {
      throw new Error(`docs refs did not return active relations: ${JSON.stringify(refs)}`);
    }

    const related = JSON.parse(execFileSync(harness, [
      "company", "docs", "related",
      "--record", "typed-record-cli-child-1",
    ], { cwd: repoRoot, env: cliEnv, encoding: "utf8" }));
    if (related.boundaries?.read_only !== true || !related.related_refs?.some((entry) => entry.id === "document-cli-child")) {
      throw new Error(`docs related did not return relation-derived refs: ${JSON.stringify(related)}`);
    }

    const scopedSnapshot = JSON.parse(execFileSync(harness, [
      "company", "docs", "snapshot",
      "--document", "document-cli-child",
    ], { cwd: repoRoot, env: cliEnv, encoding: "utf8" }));
    if (scopedSnapshot.boundaries?.read_only !== true || scopedSnapshot.primary?.id !== "document-cli-child") {
      throw new Error(`docs snapshot did not return selected projection bundle: ${JSON.stringify(scopedSnapshot)}`);
    }

    const diff = JSON.parse(execFileSync(harness, [
      "company", "docs", "diff",
      "--document", "document-cli-child",
      "--proposed-json", "{\"id\":\"document-cli-child\",\"title\":\"Proposed title\"}",
    ], { cwd: repoRoot, env: cliEnv, encoding: "utf8" }));
    if (diff.boundaries?.diff_does_not_dispatch !== true || !diff.changed_fields?.some((entry) => entry.field === "title")) {
      throw new Error(`docs diff did not return review-only changed fields: ${JSON.stringify(diff)}`);
    }

    const changeReport = JSON.parse(execFileSync(harness, [
      "company", "docs", "change-report",
      "--action-json", JSON.stringify(typedRecordUpdateDryRun.action),
    ], { cwd: repoRoot, env: cliEnv, encoding: "utf8" }));
    if (changeReport.boundaries?.read_only !== true || changeReport.review_boundary !== "report_only_no_dispatch" || !changeReport.changed_fields?.length) {
      throw new Error(`docs change-report did not explain proposed action: ${JSON.stringify(changeReport)}`);
    }

    const relationRelinkDryRun = JSON.parse(execFileSync(harness, [
      "company", "docs", "relation", "relink",
      "--definition", "page-docs-cli",
      "--relation", "relation-cli-child-to-record",
      "--to-record", "typed-record-cli-child-1",
      "--actor", "agent-docs-governance",
      "--dry-run",
    ], { cwd: repoRoot, env: cliEnv, encoding: "utf8" }));
    if (relationRelinkDryRun.ok !== true || relationRelinkDryRun.dry_run !== true || relationRelinkDryRun.boundaries?.physical_delete !== false) {
      throw new Error(`relation relink dry-run did not expose archive-plus-link cleanup boundary: ${JSON.stringify(relationRelinkDryRun)}`);
    }

    const relationUnlinkDryRun = JSON.parse(execFileSync(harness, [
      "company", "docs", "relation", "unlink",
      "--definition", "page-docs-cli",
      "--relation", "relation-cli-child-to-record",
      "--actor", "agent-docs-governance",
      "--dry-run",
    ], { cwd: repoRoot, env: cliEnv, encoding: "utf8" }));
    if (relationUnlinkDryRun.ok !== true || relationUnlinkDryRun.dry_run !== true || relationUnlinkDryRun.after?.lifecycle_status !== "archived" || relationUnlinkDryRun.boundaries?.physical_delete !== false) {
      throw new Error(`relation unlink dry-run did not preserve archive/no-delete boundary: ${JSON.stringify(relationUnlinkDryRun)}`);
    }

    let relationUnlinkWithoutConfirmFailed = false;
    try {
      execFileSync(harness, [
        "company", "docs", "relation", "unlink",
        "--definition", "page-docs-cli",
        "--relation", "relation-cli-child-to-record",
        "--actor", "agent-docs-governance",
      ], { cwd: repoRoot, env: cliEnv, encoding: "utf8", stdio: "pipe" });
    } catch {
      relationUnlinkWithoutConfirmFailed = true;
    }
    if (!relationUnlinkWithoutConfirmFailed) throw new Error("relation unlink succeeded without --confirm");

    const relationUnlink = JSON.parse(execFileSync(harness, [
      "company", "docs", "relation", "unlink",
      "--definition", "page-docs-cli",
      "--relation", "relation-cli-child-to-record",
      "--actor", "agent-docs-governance",
      "--confirm",
    ], { cwd: repoRoot, env: cliEnv, encoding: "utf8" }));
    if (relationUnlink.ok !== true || relationUnlink.result?.operation !== "unlink" || relationUnlink.result?.lifecycle_status !== "archived") {
      throw new Error(`relation unlink did not return archived lifecycle: ${JSON.stringify(relationUnlink)}`);
    }

    const unlinkedDocumentQuery = JSON.parse(execFileSync(harness, [
      "company", "docs", "query",
      "--document", "document-cli-child",
    ], { cwd: repoRoot, env: cliEnv, encoding: "utf8" }));
    if (unlinkedDocumentQuery.relations?.some((entry) => entry.id === "relation-cli-child-to-record")) {
      throw new Error(`docs query still returned an archived Relation as active context: ${JSON.stringify(unlinkedDocumentQuery.relations)}`);
    }
    if (!unlinkedDocumentQuery.health_findings?.some((entry) => entry.kind === "missing_document_record_relation" && entry.subject?.id === "typed-record-cli-child-1")) {
      throw new Error(`docs query did not surface missing relation health finding after unlink: ${JSON.stringify(unlinkedDocumentQuery.health_findings)}`);
    }

    const moduleQuery = JSON.parse(execFileSync(harness, [
      "company", "docs", "query",
      "--module", "module-docs-cli",
    ], { cwd: repoRoot, env: cliEnv, encoding: "utf8" }));
    if (moduleQuery.business_module?.id !== "module-docs-cli" || moduleQuery.document?.id !== "document-root") {
      throw new Error(`docs query --module did not return module plus root Document context: ${JSON.stringify(moduleQuery)}`);
    }
    if (!moduleQuery.views?.some((entry) => entry.id === "view-cli-child-records") || !moduleQuery.custom_page_definitions?.some((entry) => entry.id === "page-docs-cli")) {
      throw new Error(`docs query --module did not include module Views and page definitions: ${JSON.stringify({ views: moduleQuery.views, definitions: moduleQuery.custom_page_definitions })}`);
    }
    if (!moduleQuery.available_commands?.some((entry) => entry.command?.startsWith("harness company docs query --document") && entry.effect === "read_only")) {
      throw new Error(`docs query did not expose read-only available commands: ${JSON.stringify(moduleQuery.available_commands)}`);
    }

    const snapshot = await get(base, "/v1/company-os/snapshot");
    const documents = snapshot.documents ?? [];
    const blocks = snapshot.blocks ?? [];
    const typedRecords = snapshot.typed_records ?? [];
    const views = snapshot.views ?? [];
    const relations = snapshot.relations ?? [];
    const modules = snapshot.business_modules ?? [];
    const definitions = snapshot.custom_page_definitions ?? [];
    const policies = snapshot.action_policy_definitions ?? [];
    const child = documents.find((document) => document.id === "document-cli-child");
    const reusableTemplateDocument = documents.find((document) => document.id === "template-cli-reusable");
    const appendedBlock = blocks.find((entry) => entry.id === "block-cli-child-1");
    const appendedSecondBlock = blocks.find((entry) => entry.id === "block-cli-child-2");
    const removedBlock = blocks.find((entry) => entry.id === "block-cli-child-remove");
    const archivedBlock = blocks.find((entry) => entry.id === "block-cli-child-archive");
    const copiedTemplateBlock = blocks.find((entry) => entry.id === "block-cli-template-document-cli-child-1-block-template-cli-1");
    const appendedTypedRecord = typedRecords.find((entry) => entry.id === "typed-record-cli-child-1");
    const createdView = views.find((entry) => entry.id === "view-cli-child-records");
    const createdModule = modules.find((entry) => entry.id === "module-docs-cli");
    const createdDefinition = definitions.find((entry) => entry.id === "page-docs-cli");
    const linkedRelation = relations.find((entry) => entry.id === "relation-cli-child-to-record");
    const structureFolder = documents.find((document) => document.id === "document-cli-structure-parent");
    if (!child || child.parent_document_id !== "document-cli-structure-parent" || child.title !== "CLI Child Renamed" || child.lifecycle_status !== "archived") {
      throw new Error(`CLI child Document did not preserve latest rename/move/archive state: ${JSON.stringify(child)}`);
    }
    if (!structureFolder || structureFolder.parent_document_id !== "document-root") {
      throw new Error(`CLI structure parent Document missing or not parented under root: ${JSON.stringify(structureFolder)}`);
    }
    if (!reusableTemplateDocument || reusableTemplateDocument.kind !== "template" || reusableTemplateDocument.template_ref !== null || reusableTemplateDocument.lifecycle_status !== "archived") {
      throw new Error(`CLI reusable template Document missing or not template kind: ${JSON.stringify(reusableTemplateDocument)}`);
    }
    if (!createdModule?.relation_rules?.some((rule) => rule.relation_type === "source_for" && rule.from_kind === "document" && rule.to_kind === "typed_record")) {
      throw new Error(`CLI module did not preserve Document-to-TypedRecord relation rule: ${JSON.stringify(createdModule)}`);
    }
    if (child.template_ref !== "template-cli-child") throw new Error(`CLI child Document did not preserve template_ref provenance: ${JSON.stringify(child)}`);
    if (!appendedBlock || appendedBlock.document_id !== "document-cli-child") throw new Error("CLI Block is missing or not attached to child document_id");
    if (!appendedSecondBlock || appendedSecondBlock.document_id !== "document-cli-child" || appendedSecondBlock.kind !== "heading") {
      throw new Error("CLI second Block is missing or not attached as a heading");
    }
    if (!copiedTemplateBlock || copiedTemplateBlock.document_id !== "document-cli-child" || copiedTemplateBlock.content?.title !== "Template note") {
      throw new Error(`CLI template instantiation did not copy template Block content: ${JSON.stringify(copiedTemplateBlock)}`);
    }
    if (appendedBlock.kind !== "callout" || appendedBlock.content?.title !== "CLI acceptance note updated" || appendedBlock.content?.tone !== "success") {
      throw new Error(`CLI structured Block did not preserve kind/content: ${JSON.stringify(appendedBlock)}`);
    }
    if (JSON.stringify(child.block_ids) !== JSON.stringify(["block-cli-child-2", "block-cli-child-1", "block-cli-template-document-cli-child-1-block-template-cli-1"])) {
      throw new Error(`CLI block reorder did not preserve and reorder Document.block_ids: ${JSON.stringify(child.block_ids)}`);
    }
    if (reusableTemplateDocument.block_ids.length !== 3 || !reusableTemplateDocument.block_ids.every((id) => id.startsWith("block-cli-template-template-cli-reusable"))) {
      throw new Error(`CLI reusable template did not receive copied source Blocks: ${JSON.stringify(reusableTemplateDocument.block_ids)}`);
    }
    if (!removedBlock || removedBlock.document_id !== "document-cli-child" || child.block_ids.includes("block-cli-child-remove")) {
      throw new Error(`CLI block remove did not preserve the Block row while removing it from Document.block_ids: ${JSON.stringify({ removedBlock, blockIds: child.block_ids })}`);
    }
    if (!archivedBlock || archivedBlock.content?._archived !== true || child.block_ids.includes("block-cli-child-archive")) {
      throw new Error(`CLI block archive did not preserve archived metadata while removing it from Document.block_ids: ${JSON.stringify({ archivedBlock, blockIds: child.block_ids })}`);
    }
    if (!appendedTypedRecord || appendedTypedRecord.source_document_ref !== "document-cli-child" || appendedTypedRecord.module_id !== "module-docs-cli" || appendedTypedRecord.title !== "CLI Typed Record Updated" || appendedTypedRecord.fields?.status !== "accepted" || appendedTypedRecord.fields?.source !== "cli-live") {
      throw new Error(`CLI TypedRecord is missing, not scoped, or did not preserve merged fields: ${JSON.stringify(appendedTypedRecord)}`);
    }
    if (!createdView || createdView.module_id !== "module-docs-cli" || !createdView.source_kinds?.includes("typed_record")) {
      throw new Error("CLI View is missing or not scoped to the module/typed_record source");
    }
    if (!createdModule || !createdModule.default_view_refs?.includes("view-docs-cli") || !createdModule.custom_page_definition_refs?.includes("page-docs-cli")) {
      throw new Error("CLI BusinessModule is missing its fallback View or CustomPageDefinition reference");
    }
    if (!createdDefinition || !createdDefinition.action_command_refs?.includes("relation.append")) {
      throw new Error("CLI CustomPageDefinition is missing or does not declare relation.append");
    }
    if (policies.filter((entry) => entry.definition_ref === "page-docs-cli").length !== 5) {
      throw new Error("CLI CustomPageDefinition did not install the five expected ActionPolicyDefinition records");
    }
    if (!linkedRelation || linkedRelation.from_ref?.id !== "document-cli-child" || linkedRelation.to_ref?.id !== "typed-record-cli-child-1" || linkedRelation.lifecycle_status !== "archived") {
      throw new Error(`CLI Relation is missing, incorrectly linked, or not archived by unlink: ${JSON.stringify(linkedRelation)}`);
    }
    if ((snapshot.work_items ?? []).length || (snapshot.approvals ?? []).length || (snapshot.financial_records ?? []).length) {
      throw new Error("Docs CLI authoring created Work, Approval, or Finance side effects");
    }
    console.log(JSON.stringify({
      status: "passed",
      module_id: createdModule.id,
      definition_id: createdDefinition.id,
      document_id: child.id,
      document_title: child.title,
      document_parent_id: child.parent_document_id,
      document_lifecycle_status: child.lifecycle_status,
      template_id: child.template_ref,
      reusable_template_id: reusableTemplateDocument.id,
      reusable_template_status: reusableTemplateDocument.lifecycle_status,
      block_id: appendedBlock.id,
      copied_template_block_id: copiedTemplateBlock.id,
      reusable_template_block_count: reusableTemplateDocument.block_ids.length,
      reordered_block_ids: child.block_ids,
      typed_record_id: appendedTypedRecord.id,
      relation_id: linkedRelation.id,
      relation_lifecycle_status: linkedRelation.lifecycle_status,
      view_id: createdView.id,
      query_boundaries: unlinkedDocumentQuery.boundaries,
      query_block_count: unlinkedDocumentQuery.blocks.length,
      policy_count: policies.filter((entry) => entry.definition_ref === "page-docs-cli").length,
      command_count: (snapshot.action_commands ?? []).length,
      side_effects: { work_items: 0, approvals: 0, financial_records: 0 },
    }, null, 2));
  } finally {
    server.kill("SIGTERM");
    await new Promise((resolveStop) => server.once("exit", resolveStop));
    await rm(root, { recursive: true, force: true });
  }
}

main().catch((error) => {
  console.error(error.stack || error.message);
  process.exit(1);
});
