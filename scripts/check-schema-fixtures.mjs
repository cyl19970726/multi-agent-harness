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

function schemaFilesRecursively(dir) {
  if (!existsSync(dir)) return [];
  return readdirSync(dir)
    .flatMap((entry) => {
      const path = join(dir, entry);
      return statSync(path).isDirectory()
        ? schemaFilesRecursively(path)
        : path.endsWith(".schema.json")
          ? [path]
          : [];
    })
    .sort();
}

function formatErrors(errors) {
  return (errors ?? [])
    .map((error) => `${error.instancePath || "/"} ${error.message}`)
    .join("; ");
}

function officialCompositeErrors(_fixtureName, _data) {
  return [];
}

const schemaFiles = readdirSync(schemaRoot)
  .filter((entry) => entry.endsWith(".schema.json"))
  .map((entry) => join(schemaRoot, entry))
  .sort();
const registrySchemaFiles = schemaFilesRecursively(schemaRoot);

// Compile the complete schema tree as one registry. Top-level schemas may
// reference nested protocol schemas (for example Message -> CollaborationScope),
// while fixture enforcement below intentionally remains top-level here.
const ajv = new Ajv2020({ allErrors: true, strict: false });
const schemas = schemaFiles.map((schemaFile) => ({
  schemaFile,
  schema: readJson(schemaFile),
}));
for (const schemaFile of registrySchemaFiles) {
  ajv.addSchema(readJson(schemaFile));
}

for (const { schemaFile, schema } of schemas) {
  const validate = ajv.getSchema(schema.$id) ?? ajv.compile(schema);
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
    const schemaValid = validate(data);
    const semanticErrors = officialCompositeErrors(fixtureName, data);
    if (!schemaValid || semanticErrors.length > 0) {
      failures.push(`${fixture}: expected valid but failed: ${formatErrors(validate.errors)}`);
      if (semanticErrors.length > 0) {
        failures.push(`${fixture}: expected valid but failed: ${semanticErrors.join("; ")}`);
      }
    }
  }

  for (const fixture of invalidFixtures) {
    invalidCount += 1;
    const data = readJson(fixture);
    const schemaValid = validate(data);
    const semanticErrors = officialCompositeErrors(fixtureName, data);
    if (schemaValid && semanticErrors.length === 0) {
      failures.push(`${fixture}: expected invalid but passed ${schemaFile}`);
    }
  }
}

if (failures.length) {
  console.error(failures.join("\n"));
  process.exit(1);
}

console.log(
  `validated official composite schema fixtures: ${validCount} valid, ${invalidCount} invalid`,
);
