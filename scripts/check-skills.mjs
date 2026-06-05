import { existsSync, lstatSync, readFileSync, readdirSync, statSync } from "node:fs";
import { basename, join } from "node:path";

// Skills live in two roots: `skills/` holds the SHIPPED deliverable skills (what
// others install), `.agents/skills/` holds the repo's internal runtime skills
// (auto-discovered by Codex / harness-spawned workers). A deliverable may be
// symlinked into `.agents/skills/` for runtime discovery; we skip symlinks so it
// is validated once, at its real source.
const skillsRoots = ["skills", ".agents/skills"];
const failures = [];
const checked = [];
const resolvedSkills = new Set();

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
  // Track that this skill id exists for later validation
  resolvedSkills.add(skillName);
}

// Validate all skills exist and resolve their ids, across both roots. Skip
// symlinked entries (a deliverable skill symlinked into .agents/skills/ for
// runtime discovery is validated at its real `skills/` source).
for (const skillsRoot of skillsRoots) {
  if (!existsSync(skillsRoot)) continue;
  for (const entry of readdirSync(skillsRoot)) {
    const skillDir = join(skillsRoot, entry);
    if (lstatSync(skillDir).isSymbolicLink()) continue;
    if (statSync(skillDir).isDirectory()) {
      validateSkill(skillDir);
    }
  }
}

// Check for dangling skill_refs in member JSON files
function checkMemberSkillRefs() {
  const dataRoot = ".agents/data";
  if (!existsSync(dataRoot)) {
    return;
  }

  // Find all agent-member JSON files
  function scanDir(dir) {
    try {
      for (const entry of readdirSync(dir)) {
        const path = join(dir, entry);
        const stat = statSync(path);
        if (stat.isDirectory()) {
          scanDir(path);
        } else if (path.endsWith("-agent-member.json")) {
          try {
            const content = readFileSync(path, "utf8");
            const data = JSON.parse(content);
            if (data.skill_refs && Array.isArray(data.skill_refs)) {
              for (const skillRef of data.skill_refs) {
                if (!resolvedSkills.has(skillRef)) {
                  failures.push(
                    `${path}: skill_ref "${skillRef}" does not exist at .agents/skills/${skillRef}/SKILL.md`
                  );
                }
              }
            }
          } catch (e) {
            failures.push(`${path}: failed to parse JSON: ${e.message}`);
          }
        }
      }
    } catch (e) {
      // Directory may not exist or be readable
    }
  }

  scanDir(dataRoot);
}

checkMemberSkillRefs();

if (failures.length) {
  console.error(failures.join("\n"));
  process.exit(1);
}

console.log(`checked ${checked.length} skills and validated all skill_refs in member records`);
