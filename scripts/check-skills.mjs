import { existsSync, readFileSync, readdirSync, statSync } from "node:fs";
import { basename, join } from "node:path";

const skillsRoot = ".agents/skills";
const failures = [];
const checked = [];

function parseFrontmatter(file) {
  const text = readFileSync(file, "utf8");
  const match = text.match(/^---\n([\s\S]*?)\n---\n/);
  if (!match) return null;
  const fields = {};
  for (const line of match[1].split("\n")) {
    const field = line.match(/^([a-zA-Z0-9_-]+):\s*(.*)$/);
    if (!field) continue;
    fields[field[1]] = field[2].replace(/^["']|["']$/g, "");
  }
  return fields;
}

function validateSkill(skillDir) {
  const skillName = basename(skillDir);
  const skillFile = join(skillDir, "SKILL.md");
  if (!existsSync(skillFile)) {
    failures.push(`${skillDir}: missing SKILL.md`);
    return;
  }

  const fields = parseFrontmatter(skillFile);
  if (!fields) {
    failures.push(`${skillFile}: missing YAML frontmatter`);
    return;
  }

  if (!/^[a-z0-9-]+$/.test(fields.name ?? "")) {
    failures.push(`${skillFile}: name must use lowercase letters, digits, and hyphens`);
  }
  if (fields.name !== skillName) {
    failures.push(`${skillFile}: name must match folder name ${skillName}`);
  }
  if (!fields.description || fields.description.includes("TODO") || fields.description.length < 40) {
    failures.push(`${skillFile}: description must be complete and specific`);
  }

  const metadataFile = join(skillDir, "agents", "openai.yaml");
  if (!existsSync(metadataFile)) {
    failures.push(`${skillDir}: missing agents/openai.yaml`);
  } else {
    const metadata = readFileSync(metadataFile, "utf8");
    for (const key of ["display_name", "short_description", "default_prompt"]) {
      if (!metadata.includes(`${key}:`)) {
        failures.push(`${metadataFile}: missing ${key}`);
      }
    }
    if (metadata.includes("TODO")) {
      failures.push(`${metadataFile}: contains TODO`);
    }
  }

  checked.push(skillFile);
}

if (existsSync(skillsRoot)) {
  for (const entry of readdirSync(skillsRoot)) {
    const skillDir = join(skillsRoot, entry);
    if (statSync(skillDir).isDirectory()) {
      validateSkill(skillDir);
    }
  }
}

if (failures.length) {
  console.error(failures.join("\n"));
  process.exit(1);
}

console.log(`checked ${checked.length} skills`);
