import Ajv2020 from "ajv/dist/2020.js";
import { existsSync, readFileSync, readdirSync, statSync } from "node:fs";
import { join } from "node:path";

const schema = JSON.parse(
  readFileSync("schemas/agent-harness-tool-descriptor.schema.json", "utf8"),
);
const ajv = new Ajv2020({ allErrors: true, strict: false });
const validate = ajv.compile(schema);
const files = [];

function walk(dir) {
  if (!existsSync(dir)) return;
  for (const entry of readdirSync(dir)) {
    const full = join(dir, entry);
    const stat = statSync(full);
    if (stat.isDirectory()) {
      walk(full);
    } else if (full.endsWith(".json") && full.includes("/tool-descriptors/")) {
      files.push(full);
    }
  }
}

function formatErrors(errors) {
  return (errors ?? [])
    .map((error) => `${error.instancePath || "/"} ${error.message}`)
    .join("; ");
}

walk("examples");

const failures = [];
for (const file of files.sort()) {
  const data = JSON.parse(readFileSync(file, "utf8"));
  if (!validate(data)) {
    failures.push(`${file}: ${formatErrors(validate.errors)}`);
  }
}

if (failures.length) {
  console.error(failures.join("\n"));
  process.exit(1);
}

console.log(`validated ${files.length} tool descriptors`);
