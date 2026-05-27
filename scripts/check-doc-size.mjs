import { readdirSync, readFileSync, statSync } from "node:fs";
import { join } from "node:path";

const maxLines = 500;
const roots = ["README.md", "docs", "schemas", ".agents/skills", "examples", "apps"];
const warnings = [];

function walk(path) {
  const stat = statSync(path);
  if (stat.isDirectory()) {
    for (const entry of readdirSync(path)) {
      walk(join(path, entry));
    }
    return;
  }
  if (!path.endsWith(".md")) return;
  const lineCount = readFileSync(path, "utf8").split("\n").length;
  if (lineCount > maxLines) {
    warnings.push(`${path}: ${lineCount} lines exceeds ${maxLines}; keep merged only with a reason`);
  }
}

for (const root of roots) {
  walk(root);
}

if (warnings.length) {
  console.warn(warnings.join("\n"));
} else {
  console.log(`all markdown files are <= ${maxLines} lines`);
}
