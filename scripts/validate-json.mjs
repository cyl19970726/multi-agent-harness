import { readFileSync } from "node:fs";
import { join } from "node:path";
import { readdirSync, statSync } from "node:fs";

const roots = ["schemas", "examples"];
const files = [];

function walk(dir) {
  for (const entry of readdirSync(dir)) {
    const full = join(dir, entry);
    if (statSync(full).isDirectory()) {
      walk(full);
    } else if (full.endsWith(".json")) {
      files.push(full);
    }
  }
}

for (const root of roots) {
  walk(root);
}

for (const file of files) {
  JSON.parse(readFileSync(file, "utf8"));
}

console.log(`validated ${files.length} JSON files`);
