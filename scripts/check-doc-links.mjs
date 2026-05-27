import { existsSync, readFileSync, readdirSync, statSync } from "node:fs";
import { dirname, join, normalize } from "node:path";

const roots = ["README.md", "docs", "schemas", ".agents/skills", "examples", "apps"];
const markdownFiles = [];
const linkPattern = /\[[^\]]+\]\(([^)]+)\)/g;
const failures = [];

function walk(path) {
  if (!existsSync(path)) return;
  const stat = statSync(path);
  if (stat.isDirectory()) {
    for (const entry of readdirSync(path)) {
      walk(join(path, entry));
    }
    return;
  }
  if (path.endsWith(".md")) {
    markdownFiles.push(path);
  }
}

for (const root of roots) {
  walk(root);
}

for (const file of markdownFiles) {
  const text = readFileSync(file, "utf8");
  for (const match of text.matchAll(linkPattern)) {
    const raw = match[1];
    if (/^(https?:|mailto:|#)/.test(raw)) continue;
    const targetWithoutHash = raw.split("#")[0];
    if (!targetWithoutHash) continue;
    const target = normalize(join(dirname(file), targetWithoutHash));
    if (!existsSync(target)) {
      failures.push(`${file}: missing link target ${raw}`);
    }
  }
}

if (failures.length) {
  console.error(failures.join("\n"));
  process.exit(1);
}

console.log(`checked ${markdownFiles.length} markdown files`);
