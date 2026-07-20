#!/usr/bin/env node

import assert from "node:assert/strict";
import { mkdtemp, mkdir, readFile, readdir, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, extname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const runtimeSource = join(here, "..", "src", "company-os", "runtime");

async function loadRuntime() {
  const { default: ts } = await import("typescript");
  const directory = await mkdtemp(join(tmpdir(), "company-os-runtime-"));
  const files = (await readdir(runtimeSource)).filter((file) => extname(file) === ".ts");
  await mkdir(directory, { recursive: true });

  for (const file of files) {
    const source = await readFile(join(runtimeSource, file), "utf8");
    let output = ts.transpileModule(source, {
      compilerOptions: {
        module: ts.ModuleKind.ESNext,
        target: ts.ScriptTarget.ES2022,
      },
      fileName: file,
    }).outputText;
    output = output.replaceAll(/(from\s+["']\.\/[A-Za-z0-9_-]+)(["'])/g, "$1.mjs$2");
    output = output.replaceAll(/(export\s+\*\s+from\s+["']\.\/[A-Za-z0-9_-]+)(["'])/g, "$1.mjs$2");
    await writeFile(join(directory, file.replace(/\.ts$/, ".mjs")), output, "utf8");
  }

  try {
    return await import(pathToFileURL(join(directory, "index.mjs")).href);
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
}

function createClock() {
  let tick = 0;
  return { now: () => `2026-07-20T00:00:${String(tick++).padStart(2, "0")}Z` };
}

function createFixtureSource(runtime) {
  const fixtures = new Map([
    ["view:trademark-applications", [{ id: "CN-2026-018", title: "Brand A", status: "preparing" }]],
    ["view:trademark-work", [{ id: "work-1", title: "Trademark filing for Brand A", assignee: "Trademark Agent", status: "active" }]],
    ["view:trademark-approvals", [{ id: "approval-1", title: "Approve filing commitment", status: "pending", approver: "Brand Owner" }]],
    ["view:trademark-finance", [{ id: "commitment-1", kind: "Commitment", amount: 3000, currency: "CNY", status: "pending" }]],
    ["view:trademark-participants", [
      { id: "human:brand-owner", name: "Brand Owner", kind: "human", responsibility: "accountable" },
      { id: "agent:trademark", name: "Trademark Agent", kind: "standing_agent", responsibility: "assigned" },
      { id: "external:lawyer", name: "External Lawyer", kind: "external", responsibility: "legal review" },
    ]],
  ]);
  return {
    async read(request) {
      assert.deepEqual(Object.keys(request).sort(), ["parameters", "recordTypes", "relationPaths", "viewId"]);
      return structuredClone(fixtures.get(request.viewId) ?? []);
    },
  };
}

function createDeps(runtime, overrides = {}) {
  const calls = [];
  const source = overrides.source ?? createFixtureSource(runtime);
  const transport = overrides.transport ?? {
    async dispatch(envelope) {
      calls.push(envelope);
      return {
        data: { id: "commitment-1", status: "pending" },
        effects: [{ kind: "FinancialCommitment", recordId: "commitment-1" }],
      };
    },
  };
  return {
    calls,
    source,
    transport,
    policy: overrides.policy ?? { canInvoke: () => true },
    fallback: overrides.fallback ?? {
      async render(fallback, context) {
        return { documentId: fallback.owningDocumentId, viewIds: [...fallback.viewIds], context };
      },
    },
  };
}

async function main() {
  const runtime = await loadRuntime();
  let passed = 0;

  const pass = (message) => {
    console.log(`  PASS  ${message}`);
    passed += 1;
  };

  console.log("== Company OS programmable page runtime checks ==");

  {
    const audit = runtime.createPageAuditSink({
      runtimeId: "runtime-query-test",
      definition: runtime.trademarkModuleDefinition,
      packageManifest: runtime.trademarkModulePackage,
      clock: createClock(),
    });
    const queries = runtime.createScopedQueryAdapter({
      definition: runtime.trademarkModuleDefinition,
      source: createFixtureSource(runtime),
      audit,
    });
    await assert.rejects(
      () => queries.query("records.scan-all"),
      (error) => error?.code === "UNDECLARED_QUERY",
    );
    assert.equal(audit.snapshot().events.at(-1)?.kind, "query.denied");
    pass("undeclared queries are rejected before the read-only view source is called");
  }

  {
    const deps = createDeps(runtime);
    const audit = runtime.createPageAuditSink({
      runtimeId: "runtime-action-test",
      definition: runtime.trademarkModuleDefinition,
      packageManifest: runtime.trademarkModulePackage,
      clock: createClock(),
    });
    const dispatcher = runtime.createActionCommandDispatcher({
      definition: runtime.trademarkModuleDefinition,
      transport: deps.transport,
      policy: deps.policy,
      audit,
    });
    const result = await dispatcher.dispatch({
      command: "store.record.write",
      actor: { id: "agent:trademark", kind: "standing_agent" },
    });
    assert.deepEqual([result.status, result.code], ["denied", "UNDECLARED_ACTION"]);
    assert.equal(deps.calls.length, 0);
    pass("undeclared actions and direct-store-shaped actions never reach the command transport");
  }

  {
    const deps = createDeps(runtime, { policy: { canInvoke: () => false } });
    const audit = runtime.createPageAuditSink({
      runtimeId: "runtime-policy-test",
      definition: runtime.trademarkModuleDefinition,
      packageManifest: runtime.trademarkModulePackage,
      clock: createClock(),
    });
    const dispatcher = runtime.createActionCommandDispatcher({
      definition: runtime.trademarkModuleDefinition,
      transport: deps.transport,
      policy: deps.policy,
      audit,
    });
    const result = await dispatcher.dispatch({
      command: "trademark.application.create",
      actor: { id: "external:lawyer", kind: "external" },
    });
    assert.deepEqual([result.status, result.code], ["denied", "POLICY_DENIED"]);
    assert.equal(deps.calls.length, 0);
    pass("page policy denial happens before any governed command transport call");
  }

  {
    const deps = createDeps(runtime);
    const audit = runtime.createPageAuditSink({
      runtimeId: "runtime-approval-test",
      definition: runtime.trademarkModuleDefinition,
      packageManifest: runtime.trademarkModulePackage,
      clock: createClock(),
    });
    const dispatcher = runtime.createActionCommandDispatcher({
      definition: runtime.trademarkModuleDefinition,
      transport: deps.transport,
      policy: deps.policy,
      audit,
    });
    const actor = { id: "agent:finance", kind: "standing_agent" };
    const missing = await dispatcher.dispatch({ command: "finance.commitment.authorize", actor });
    assert.deepEqual([missing.status, missing.code], ["denied", "HUMAN_APPROVAL_REQUIRED"]);
    const agentApproval = await dispatcher.dispatch({
      command: "finance.commitment.authorize",
      actor,
      approval: {
        id: "approval-agent",
        status: "approved",
        decidedBy: { id: "agent:finance", kind: "standing_agent" },
      },
    });
    assert.deepEqual([agentApproval.status, agentApproval.code], ["denied", "HUMAN_APPROVAL_REQUIRED"]);
    const humanApproval = await dispatcher.dispatch({
      command: "finance.commitment.authorize",
      actor,
      input: { amount: 3000, currency: "CNY" },
      approval: {
        id: "approval-human",
        status: "approved",
        decidedBy: { id: "human:brand-owner", kind: "human" },
      },
    });
    assert.equal(humanApproval.status, "accepted");
    assert.equal(deps.calls.length, 1);
    assert.deepEqual(deps.calls[0].policy.allowedEffectKinds, ["FinancialCommitment", "AuditEvent"]);
    pass("sensitive commands require an approved first-class Human Approval before dispatch");
  }

  {
    const deps = createDeps(runtime);
    const page = runtime.createCustomPageRuntime({
      runtimeId: "runtime-fallback-test",
      definition: runtime.trademarkModuleDefinition,
      packageManifest: runtime.trademarkModulePackage,
      renderer: { async render() { throw new Error("renderer unavailable"); } },
      source: deps.source,
      transport: deps.transport,
      policy: deps.policy,
      fallback: deps.fallback,
      clock: createClock(),
    });
    assert.equal(page.loading().status, "loading");
    const result = await page.render({ pageTitle: "Trademark Management" });
    assert.equal(result.status, "fallback");
    assert.equal(result.content.documentId, "document:trademark-management");
    assert.ok(result.content.viewIds.includes("view:trademark-finance"));
    assert.deepEqual(
      result.audit.events.slice(-2).map((event) => event.kind),
      ["runtime.render_failed", "runtime.fallback_ready"],
    );
    pass("render failure returns the registered Document/standard Views with error and audit metadata");
  }

  {
    const deps = createDeps(runtime);
    const page = runtime.createCustomPageRuntime({
      runtimeId: "runtime-trademark-test",
      definition: runtime.trademarkModuleDefinition,
      packageManifest: runtime.trademarkModulePackage,
      renderer: runtime.trademarkModuleRenderer,
      source: deps.source,
      transport: deps.transport,
      policy: deps.policy,
      fallback: deps.fallback,
      clock: createClock(),
    });
    const result = await page.render({
      pageTitle: "Trademark Management",
      focusApplicationId: "CN-2026-018",
    });
    assert.equal(result.status, "ready");
    assert.equal(result.content.finance.committedAmount, 3000);
    assert.equal(result.content.finance.paymentAmount, 0);
    assert.deepEqual(result.content.finance.paymentRecordIds, []);
    pass("Trademark composition renders canonical query results and never converts a Commitment into Payment");
  }

  {
    const deps = createDeps(runtime, {
      transport: {
        async dispatch() {
          return {
            data: { id: "commitment-1" },
            effects: [
              { kind: "FinancialCommitment", recordId: "commitment-1" },
              { kind: "Payment", recordId: "payment-illegal" },
            ],
          };
        },
      },
    });
    const audit = runtime.createPageAuditSink({
      runtimeId: "runtime-effect-test",
      definition: runtime.trademarkModuleDefinition,
      packageManifest: runtime.trademarkModulePackage,
      clock: createClock(),
    });
    const dispatcher = runtime.createActionCommandDispatcher({
      definition: runtime.trademarkModuleDefinition,
      transport: deps.transport,
      policy: deps.policy,
      audit,
    });
    const result = await dispatcher.dispatch({
      command: "finance.commitment.request",
      actor: { id: "agent:finance", kind: "standing_agent" },
    });
    assert.deepEqual([result.status, result.code], ["denied", "INVALID_COMMAND_EFFECT"]);
    pass("a Commitment command cannot report an undeclared Payment effect as accepted");
  }

  {
    assert.throws(
      () => runtime.validateCustomPageContract(
        runtime.trademarkModuleDefinition,
        {
          ...runtime.trademarkModulePackage,
          capabilities: {
            ...runtime.trademarkModulePackage.capabilities,
            actions: [...runtime.trademarkModulePackage.capabilities.actions, "payment.settle"],
          },
        },
      ),
      (error) => error?.code === "UNDECLARED_PACKAGE_ACTION",
    );
    pass("package manifests cannot acquire undeclared query/action capabilities");
  }

  {
    const deps = createDeps(runtime);
    const reducedPackage = {
      ...runtime.trademarkModulePackage,
      capabilities: {
        ...runtime.trademarkModulePackage.capabilities,
        queries: runtime.trademarkModulePackage.capabilities.queries.filter(
          (name) => name !== "trademark.finance",
        ),
      },
    };
    const page = runtime.createCustomPageRuntime({
      runtimeId: "runtime-package-scope-test",
      definition: runtime.trademarkModuleDefinition,
      packageManifest: reducedPackage,
      renderer: runtime.trademarkModuleRenderer,
      source: deps.source,
      transport: deps.transport,
      policy: deps.policy,
      fallback: deps.fallback,
      clock: createClock(),
    });
    const result = await page.render({ pageTitle: "Trademark Management" });
    assert.equal(result.status, "fallback");
    assert.match(result.renderError, /Query is not declared/);
    pass("runtime grants only the query/action capabilities requested by the registered package");
  }

  console.log(`\n   company-os runtime checks: ${passed} pass, 0 fail`);
}

main().catch((error) => {
  console.error(`company-os runtime check crashed: ${error.stack || error}`);
  process.exit(1);
});
