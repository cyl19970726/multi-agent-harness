import { readFileSync, writeFileSync } from "node:fs";

const rustPath = "crates/firm-core/src/agentfirm_api/work_trust.rs";
const schemaPath = "schemas/trust-error.schema.json";
const appendixPath = "docs/current/architecture/member-trust-error-appendix.md";

const rust = readFileSync(rustPath, "utf8");
const block = rust.match(/pub enum TrustErrorCode \{([\s\S]*?)\n\}/)?.[1];
if (!block) throw new Error("TrustErrorCode enum not found");
const variants = [...block.matchAll(/^\s*([A-Z][A-Za-z0-9]+),\s*$/gm)].map((match) => match[1]);
if (variants.length === 0) throw new Error("TrustErrorCode has no variants");
const codes = variants.map((variant) => variant.replace(/([a-z0-9])([A-Z])/g, "$1_$2").toUpperCase());

const schema = `${JSON.stringify({
  $schema: "https://json-schema.org/draft/2020-12/schema",
  $id: "https://agent-harness.local/schemas/trust-error.schema.json",
  title: "Member Execution Trust Error",
  type: "object",
  additionalProperties: false,
  required: ["code", "message", "retryable", "resource_kind", "resource_id"],
  properties: {
    code: { type: "string", enum: codes },
    message: { type: "string", minLength: 1, pattern: "\\S" },
    retryable: { type: "boolean" },
    resource_kind: { type: "string", minLength: 1, pattern: "\\S" },
    resource_id: { type: "string", minLength: 1, pattern: "\\S" },
    current_version: { type: ["integer", "null"], minimum: 0 },
  },
}, null, 2)}\n`;

const rows = codes.map((code) => `| \`${code}\` | Typed kernel rejection; inspect message, resource and current_version. | false unless the returned payload explicitly says otherwise |`).join("\n");
const appendix = `# Member Execution Trust Error Appendix

This file is generated from \`TrustErrorCode\` in \`${rustPath}\`. Do not edit it by hand. Run \`node scripts/generate-member-trust-error-contract.mjs --write\` after changing the Rust enum, then commit the schema and this appendix together.

Protocol: \`agentfirm-member-trust/1\`

| Code | Contract | Default retry guidance |
| --- | --- | --- |
${rows}
`;

const outputs = [[schemaPath, schema], [appendixPath, appendix]];
if (process.argv.includes("--write")) {
  for (const [path, content] of outputs) writeFileSync(path, content);
  console.log(`generated ${codes.length} canonical trust error codes`);
} else {
  const drift = outputs.filter(([path, content]) => {
    try { return readFileSync(path, "utf8") !== content; } catch { return true; }
  });
  if (drift.length) {
    console.error(`generated trust error contract drift: ${drift.map(([path]) => path).join(", ")}`);
    process.exit(1);
  }
  console.log(`validated generated trust error schema/appendix: ${codes.length} codes`);
}
