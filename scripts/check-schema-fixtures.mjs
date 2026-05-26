import Ajv2020 from "ajv/dist/2020.js";
import { existsSync, readFileSync, readdirSync, statSync } from "node:fs";
import { basename, join } from "node:path";

const schemaRoot = "schemas";
const fixtureRoot = "schemas/fixtures";
const failures = [];
let validCount = 0;
let invalidCount = 0;

function readJson(path) {
  return JSON.parse(readFileSync(path, "utf8"));
}

function jsonFiles(dir) {
  if (!existsSync(dir)) return [];
  return readdirSync(dir)
    .map((entry) => join(dir, entry))
    .filter((path) => statSync(path).isFile() && path.endsWith(".json"))
    .sort();
}

function formatErrors(errors) {
  return (errors ?? [])
    .map((error) => `${error.instancePath || "/"} ${error.message}`)
    .join("; ");
}

const schemaFiles = readdirSync(schemaRoot)
  .filter((entry) => entry.endsWith(".schema.json"))
  .map((entry) => join(schemaRoot, entry))
  .sort();

for (const schemaFile of schemaFiles) {
  const schema = readJson(schemaFile);
  const ajv = new Ajv2020({ allErrors: true, strict: false });
  const validate = ajv.compile(schema);
  const fixtureName = basename(schemaFile, ".schema.json");
  const validFixtures = jsonFiles(join(fixtureRoot, fixtureName, "valid"));
  const invalidFixtures = jsonFiles(join(fixtureRoot, fixtureName, "invalid"));

  if (validFixtures.length === 0) {
    failures.push(`${schemaFile}: missing valid fixtures`);
  }
  if (invalidFixtures.length === 0) {
    failures.push(`${schemaFile}: missing invalid fixtures`);
  }

  for (const fixture of validFixtures) {
    validCount += 1;
    const data = readJson(fixture);
    if (!validate(data)) {
      failures.push(`${fixture}: expected valid but failed: ${formatErrors(validate.errors)}`);
    }
  }

  for (const fixture of invalidFixtures) {
    invalidCount += 1;
    const data = readJson(fixture);
    if (validate(data)) {
      failures.push(`${fixture}: expected invalid but passed ${schemaFile}`);
    }
  }
}

if (failures.length) {
  console.error(failures.join("\n"));
  process.exit(1);
}

console.log(`validated schema fixtures: ${validCount} valid, ${invalidCount} invalid`);
