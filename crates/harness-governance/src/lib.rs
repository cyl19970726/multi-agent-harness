//! Project-portable doc-governance gates, compiled into the harness binary.
//!
//! This crate is the harness-native home of the four documentation/skill gates
//! that were historically `scripts/check-doc-*.mjs` + `check-skills.mjs` (node /
//! pnpm only). The logic is a faithful 1:1 port — same roots, same rules, same
//! messages — so a project the harness operates on gets the SAME closed-loop
//! governance with zero hosted scripts and no node/pnpm dependency. Gate
//! parameters come from a per-project [`GovernanceConfig`] (today: this repo's
//! `.harness/governance.toml`, which mirrors the old hardcoded constants), so a
//! Go / Python / mdBook / no-node project configures rather than copies scripts.
//!
//! Faithful-port notes (vs the `.mjs`):
//! - directory entries are SORTED before traversal, so failure output is
//!   deterministic (node's `readdirSync` order was not). The SET of files /
//!   failures and the success counts are identical; only line order is stabilized.
//! - the link/size walks SKIP a missing root (the old `check-doc-size.mjs` had no
//!   existence guard and would throw); on a repo where every root exists — like
//!   this one — the output is identical.

use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

/// Whether a gate's failures block the overall result or only warn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Blocker,
    Warning,
}

/// The result of one gate.
#[derive(Debug, Clone)]
pub struct GateReport {
    pub kind: String,
    pub severity: Severity,
    /// Hard violations. For a `Blocker` gate these fail the overall check.
    pub failures: Vec<String>,
    /// Soft notes. Never fail the overall check (mirrors `console.warn`).
    pub warnings: Vec<String>,
    /// The success summary line printed when there are no failures.
    pub summary: String,
}

impl GateReport {
    /// A `Warning`-severity gate never contributes a blocking failure.
    pub fn is_blocking_failure(&self) -> bool {
        self.severity == Severity::Blocker && !self.failures.is_empty()
    }
}

/// The aggregate result of running every configured gate.
#[derive(Debug, Clone)]
pub struct GovernanceReport {
    pub gates: Vec<GateReport>,
}

impl GovernanceReport {
    /// The check passes when no `Blocker` gate produced a failure.
    pub fn passed(&self) -> bool {
        !self.gates.iter().any(GateReport::is_blocking_failure)
    }
}

/// Per-project governance configuration (`.harness/governance.toml`). Absent a
/// config file the harness default ([`GovernanceConfig::default_harness`]) is
/// used, which mirrors the historic `.mjs` constants of this repository.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GovernanceConfig {
    /// Config schema id (`agent_harness.governance.v1`).
    pub schema: String,
    /// Roots walked for the `links` and `size` gates.
    pub doc_roots: Vec<String>,
    /// Roots walked for the `skills` gate.
    pub skill_roots: Vec<String>,
    /// Max markdown line count before the `size` gate warns.
    pub max_lines: usize,
    /// Root scanned for `*-agent-member.json` skill_ref validation (optional).
    #[serde(default)]
    pub member_data_root: Option<String>,
    /// Registry gate config. Absent → the `registry` gate is skipped (a project
    /// with no doc registry still gets links/size/skills).
    #[serde(default)]
    pub registry: Option<RegistryConfig>,
}

/// Config for the `registry` gate (the doc-governance registry validator).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryConfig {
    pub path: String,
    pub schema: String,
    pub required_fields: Vec<String>,
    pub allowed_statuses: Vec<String>,
    pub allowed_lifecycles: Vec<String>,
    pub core_docs: Vec<String>,
}

impl GovernanceConfig {
    /// The default profile for THIS repository — a faithful mirror of the
    /// constants in the historic `scripts/check-doc-*.mjs` + `check-skills.mjs`.
    pub fn default_harness() -> Self {
        let s = |v: &str| v.to_string();
        GovernanceConfig {
            schema: s("agent_harness.governance.v1"),
            doc_roots: [
                "README.md",
                "docs",
                "schemas",
                ".agents/skills",
                "examples",
                "apps",
            ]
            .iter()
            .map(|v| s(v))
            .collect(),
            skill_roots: ["skills", ".agents/skills"].iter().map(|v| s(v)).collect(),
            max_lines: 500,
            member_data_root: Some(s(".agents/data")),
            registry: Some(RegistryConfig {
                path: s("docs/registry.json"),
                schema: s("agent_harness.docs_registry.v1"),
                required_fields: [
                    "path",
                    "ownerRole",
                    "status",
                    "lifecycle",
                    "canonicalFor",
                    "dependsOn",
                    "machineConsumers",
                    "reviewAfter",
                    "lastVerifiedWith",
                    "reorgTrigger",
                ]
                .iter()
                .map(|v| s(v))
                .collect(),
                allowed_statuses: ["idea", "planned", "stable", "deprecated", "archival"]
                    .iter()
                    .map(|v| s(v))
                    .collect(),
                allowed_lifecycles: ["volatile", "stable", "archival"]
                    .iter()
                    .map(|v| s(v))
                    .collect(),
                core_docs: [
                    "README.md",
                    "docs/README.md",
                    "docs/prd.md",
                    "docs/design-basis.md",
                    "docs/architecture.md",
                    "docs/operations.md",
                    "docs/schemas.md",
                    "docs/decisions/README.md",
                ]
                .iter()
                .map(|v| s(v))
                .collect(),
            }),
        }
    }

    /// A LIGHT generic default for a project that has not opted in (no
    /// `.governance.toml`): the cheap, registry-free gates that hold for any
    /// project (links + size + skills-if-present). A project gets real
    /// registry/core-doc governance by committing a `.governance.toml`.
    pub fn default_light() -> Self {
        let s = |v: &str| v.to_string();
        GovernanceConfig {
            schema: s("agent_harness.governance.v1"),
            doc_roots: ["README.md", "docs"].iter().map(|v| s(v)).collect(),
            skill_roots: ["skills", ".agents/skills"].iter().map(|v| s(v)).collect(),
            max_lines: 500,
            member_data_root: None,
            registry: None,
        }
    }

    /// Load `<root>/.governance.toml`, or the light default when absent.
    ///
    /// The committed config lives at the PROJECT ROOT (not under `.harness/`,
    /// which is the gitignored, serve-truncatable store) so it travels with the
    /// repo and survives a store reset.
    pub fn load(root: &Path) -> Result<Self, String> {
        let path = root.join(".governance.toml");
        if !path.exists() {
            return Ok(Self::default_light());
        }
        let text =
            std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        toml::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))
    }

    /// Serialize this config to TOML (used by `harness governance init`).
    pub fn to_toml(&self) -> Result<String, String> {
        toml::to_string_pretty(self).map_err(|e| e.to_string())
    }
}

/// Run every configured gate against `root`, using the real current date for the
/// registry `reviewAfter` staleness check.
pub fn run_check(root: &Path, config: &GovernanceConfig) -> GovernanceReport {
    run_check_at(root, config, &today_ymd())
}

/// Like [`run_check`] but with an injected `today` (`YYYY-MM-DD`) for tests.
pub fn run_check_at(root: &Path, config: &GovernanceConfig, today: &str) -> GovernanceReport {
    // Order mirrors package.json `check:links && check:doc-size && check:skills
    // && check:doc-governance` so green output reads the same as the legacy chain.
    let mut gates = vec![
        check_links(root, &config.doc_roots),
        check_size(root, &config.doc_roots, config.max_lines),
        check_skills(
            root,
            &config.skill_roots,
            config.member_data_root.as_deref(),
        ),
    ];
    if let Some(reg) = &config.registry {
        gates.push(check_governance(root, reg, today));
    }
    GovernanceReport { gates }
}

// ---------------------------------------------------------------------------
// gate: links  (port of scripts/check-doc-links.mjs)
// ---------------------------------------------------------------------------

/// Markdown link integrity: every relative `[text](target)` resolves to a file.
pub fn check_links(root: &Path, doc_roots: &[String]) -> GateReport {
    let files = collect_markdown(root, doc_roots);
    let mut failures = Vec::new();
    for rel in &files {
        let text = match std::fs::read_to_string(root.join(rel)) {
            Ok(t) => t,
            Err(_) => continue,
        };
        for raw in extract_link_targets(&text) {
            if raw.starts_with("https:")
                || raw.starts_with("http:")
                || raw.starts_with("mailto:")
                || raw.starts_with('#')
            {
                continue;
            }
            let without_hash = raw.split('#').next().unwrap_or("");
            if without_hash.is_empty() {
                continue;
            }
            let target = normalize_posix(&join_posix(parent_posix(rel), without_hash));
            if !root.join(&target).exists() {
                failures.push(format!("{rel}: missing link target {raw}"));
            }
        }
    }
    GateReport {
        kind: "links".into(),
        severity: Severity::Blocker,
        failures,
        warnings: Vec::new(),
        summary: format!("checked {} markdown files", files.len()),
    }
}

// ---------------------------------------------------------------------------
// gate: size  (port of scripts/check-doc-size.mjs — WARNING only)
// ---------------------------------------------------------------------------

/// Markdown size: warn (never block) when a file exceeds `max_lines`.
pub fn check_size(root: &Path, doc_roots: &[String], max_lines: usize) -> GateReport {
    let files = collect_markdown(root, doc_roots);
    let mut warnings = Vec::new();
    for rel in &files {
        let text = match std::fs::read_to_string(root.join(rel)) {
            Ok(t) => t,
            Err(_) => continue,
        };
        // Matches JS `text.split("\n").length` (= count of '\n' + 1).
        let line_count = text.split('\n').count();
        if line_count > max_lines {
            warnings.push(format!(
                "{rel}: {line_count} lines exceeds {max_lines}; keep merged only with a reason"
            ));
        }
    }
    // Faithful to check-doc-size.mjs: it prints EITHER the warnings OR the
    // success line, never both. An empty summary suppresses the success line when
    // there are warnings (the printer skips empty summaries).
    let summary = if warnings.is_empty() {
        format!("all markdown files are <= {max_lines} lines")
    } else {
        String::new()
    };
    GateReport {
        kind: "size".into(),
        severity: Severity::Warning,
        failures: Vec::new(),
        warnings,
        summary,
    }
}

// ---------------------------------------------------------------------------
// gate: registry/governance  (port of scripts/check-doc-governance.mjs)
// ---------------------------------------------------------------------------

/// Doc-governance registry validator: required fields, allowed enums, path +
/// dependency existence, no duplicates, all core docs registered, dates valid.
pub fn check_governance(root: &Path, cfg: &RegistryConfig, today: &str) -> GateReport {
    let mut failures = Vec::new();
    let mut warnings = Vec::new();
    let registry_path = &cfg.path;
    let abs = root.join(registry_path);

    if !abs.exists() {
        failures.push(format!("{registry_path}: missing docs governance registry"));
        return governance_report(failures, warnings, registry_path);
    }
    let raw = match std::fs::read_to_string(&abs) {
        Ok(t) => t,
        Err(e) => {
            failures.push(format!("{registry_path}: {e}"));
            return governance_report(failures, warnings, registry_path);
        }
    };
    let registry: serde_json::Value = match serde_json::from_str(&raw) {
        Ok(v) => v,
        Err(e) => {
            failures.push(format!("{registry_path}: {e}"));
            return governance_report(failures, warnings, registry_path);
        }
    };

    if registry.get("schema").and_then(|v| v.as_str()) != Some(cfg.schema.as_str()) {
        failures.push(format!("{registry_path}: schema must be {}", cfg.schema));
    }

    let documents = registry.get("documents").and_then(|v| v.as_array());
    let documents = match documents {
        None => {
            failures.push(format!("{registry_path}: documents must be an array"));
            return governance_report(failures, warnings, registry_path);
        }
        Some(d) => d,
    };

    let allowed_statuses: BTreeSet<&str> =
        cfg.allowed_statuses.iter().map(String::as_str).collect();
    let allowed_lifecycles: BTreeSet<&str> =
        cfg.allowed_lifecycles.iter().map(String::as_str).collect();
    let mut seen: BTreeSet<String> = BTreeSet::new();

    for (index, doc) in documents.iter().enumerate() {
        let label = format!("{registry_path}: documents[{index}]");

        for field in &cfg.required_fields {
            if doc.get(field).is_none() {
                failures.push(format!("{label}: missing {field}"));
            }
        }

        let path_val = doc.get("path");
        if !is_non_empty_string(path_val) {
            failures.push(format!("{label}: path must be a non-empty string"));
            continue;
        }
        let doc_path = path_val.and_then(|v| v.as_str()).unwrap_or("").to_string();
        if seen.contains(&doc_path) {
            failures.push(format!("{label}: duplicate path {doc_path}"));
        }
        seen.insert(doc_path.clone());

        if !root.join(&doc_path).exists() {
            failures.push(format!(
                "{label}: registered path does not exist: {doc_path}"
            ));
        }
        if !is_non_empty_string(doc.get("ownerRole")) {
            failures.push(format!("{label}: ownerRole must be a non-empty string"));
        }
        match doc.get("status").and_then(|v| v.as_str()) {
            Some(s) if allowed_statuses.contains(s) => {}
            other => failures.push(format!(
                "{label}: invalid status {}",
                other.unwrap_or("undefined")
            )),
        }
        match doc.get("lifecycle").and_then(|v| v.as_str()) {
            Some(s) if allowed_lifecycles.contains(s) => {}
            other => failures.push(format!(
                "{label}: invalid lifecycle {}",
                other.unwrap_or("undefined")
            )),
        }
        if !is_non_empty_string_array(doc.get("canonicalFor")) {
            failures.push(format!(
                "{label}: canonicalFor must be a non-empty string array"
            ));
        }
        match doc.get("dependsOn") {
            Some(v) if is_string_array(Some(v)) => {
                for dep in v.as_array().unwrap() {
                    let dep = dep.as_str().unwrap_or("");
                    if !root.join(dep).exists() {
                        failures.push(format!("{label}: dependency does not exist: {dep}"));
                    }
                }
            }
            _ => failures.push(format!("{label}: dependsOn must be a string array")),
        }
        if !is_non_empty_string_array(doc.get("machineConsumers")) {
            failures.push(format!(
                "{label}: machineConsumers must be a non-empty string array"
            ));
        }
        if !is_non_empty_string_array(doc.get("lastVerifiedWith")) {
            failures.push(format!(
                "{label}: lastVerifiedWith must be a non-empty string array"
            ));
        }
        if !is_non_empty_string(doc.get("reorgTrigger")) {
            failures.push(format!("{label}: reorgTrigger must be a non-empty string"));
        }

        let review_after = doc
            .get("reviewAfter")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if !is_valid_date(review_after) {
            failures.push(format!("{label}: reviewAfter must be YYYY-MM-DD"));
        } else if review_after < today {
            warnings.push(format!("{label}: reviewAfter is stale: {review_after}"));
        }
    }

    for core in &cfg.core_docs {
        if !seen.contains(core) {
            failures.push(format!("{registry_path}: missing core doc {core}"));
        }
    }

    governance_report(failures, warnings, registry_path)
}

fn governance_report(
    failures: Vec<String>,
    warnings: Vec<String>,
    registry_path: &str,
) -> GateReport {
    GateReport {
        kind: "registry".into(),
        severity: Severity::Blocker,
        failures,
        warnings,
        summary: format!("checked docs governance registry: {registry_path}"),
    }
}

// ---------------------------------------------------------------------------
// gate: skills  (port of scripts/check-skills.mjs)
// ---------------------------------------------------------------------------

/// Skill hygiene: every skill dir has valid SKILL.md frontmatter + agents
/// metadata, and every member `skill_refs` resolves to a real skill.
pub fn check_skills(
    root: &Path,
    skill_roots: &[String],
    member_data_root: Option<&str>,
) -> GateReport {
    let mut failures = Vec::new();
    let mut checked = 0usize;
    let mut resolved: BTreeSet<String> = BTreeSet::new();

    for skills_root in skill_roots {
        let abs_root = root.join(skills_root);
        if !abs_root.exists() {
            continue;
        }
        for entry in sorted_dir(&abs_root) {
            let abs = abs_root.join(&entry);
            // Skip symlinks: a deliverable symlinked into .agents/skills/ for
            // runtime discovery is validated once at its real source.
            if std::fs::symlink_metadata(&abs)
                .map(|m| m.file_type().is_symlink())
                .unwrap_or(false)
            {
                continue;
            }
            if abs.is_dir() {
                validate_skill(
                    root,
                    &format!("{skills_root}/{entry}"),
                    &mut failures,
                    &mut checked,
                    &mut resolved,
                );
            }
        }
    }

    if let Some(data_root) = member_data_root {
        check_member_skill_refs(root, data_root, &resolved, &mut failures);
    }

    GateReport {
        kind: "skills".into(),
        severity: Severity::Blocker,
        failures,
        warnings: Vec::new(),
        summary: format!("checked {checked} skills and validated all skill_refs in member records"),
    }
}

fn validate_skill(
    root: &Path,
    skill_rel: &str,
    failures: &mut Vec<String>,
    checked: &mut usize,
    resolved: &mut BTreeSet<String>,
) {
    let skill_name = skill_rel
        .rsplit('/')
        .next()
        .unwrap_or(skill_rel)
        .to_string();
    let skill_file_rel = format!("{skill_rel}/SKILL.md");
    let skill_file = root.join(&skill_file_rel);
    if !skill_file.exists() {
        failures.push(format!("{skill_rel}: missing SKILL.md"));
        return;
    }
    let text = std::fs::read_to_string(&skill_file).unwrap_or_default();
    let fields = match parse_frontmatter(&text) {
        Some(f) => f,
        None => {
            failures.push(format!("{skill_file_rel}: missing YAML frontmatter"));
            return;
        }
    };

    let name = fields.get("name").map(String::as_str).unwrap_or("");
    if name.is_empty()
        || !name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        failures.push(format!(
            "{skill_file_rel}: name must use lowercase letters, digits, and hyphens"
        ));
    }
    if name != skill_name {
        failures.push(format!(
            "{skill_file_rel}: name must match folder name {skill_name}"
        ));
    }
    let description = fields.get("description").map(String::as_str).unwrap_or("");
    if description.is_empty() || description.contains("TODO") || description.chars().count() < 40 {
        failures.push(format!(
            "{skill_file_rel}: description must be complete and specific"
        ));
    }

    let metadata_rel = format!("{skill_rel}/agents/openai.yaml");
    let metadata_file = root.join(&metadata_rel);
    if !metadata_file.exists() {
        failures.push(format!("{skill_rel}: missing agents/openai.yaml"));
    } else {
        let metadata = std::fs::read_to_string(&metadata_file).unwrap_or_default();
        for key in ["display_name", "short_description", "default_prompt"] {
            if !metadata.contains(&format!("{key}:")) {
                failures.push(format!("{metadata_rel}: missing {key}"));
            }
        }
        if metadata.contains("TODO") {
            failures.push(format!("{metadata_rel}: contains TODO"));
        }
    }

    *checked += 1;
    resolved.insert(skill_name);
}

fn check_member_skill_refs(
    root: &Path,
    data_root: &str,
    resolved: &BTreeSet<String>,
    failures: &mut Vec<String>,
) {
    let abs = root.join(data_root);
    if !abs.exists() {
        return;
    }
    let mut member_files = Vec::new();
    collect_member_files(&abs, data_root, &mut member_files);
    for rel in member_files {
        let text = match std::fs::read_to_string(root.join(&rel)) {
            Ok(t) => t,
            Err(_) => continue,
        };
        let data: serde_json::Value = match serde_json::from_str(&text) {
            Ok(v) => v,
            Err(e) => {
                failures.push(format!("{rel}: failed to parse JSON: {e}"));
                continue;
            }
        };
        if let Some(refs) = data.get("skill_refs").and_then(|v| v.as_array()) {
            for sref in refs {
                if let Some(sref) = sref.as_str() {
                    if !resolved.contains(sref) {
                        failures.push(format!(
                            "{rel}: skill_ref \"{sref}\" does not exist at .agents/skills/{sref}/SKILL.md"
                        ));
                    }
                }
            }
        }
    }
}

fn collect_member_files(abs_dir: &Path, rel_dir: &str, out: &mut Vec<String>) {
    for entry in sorted_dir(abs_dir) {
        let abs = abs_dir.join(&entry);
        let rel = format!("{rel_dir}/{entry}");
        if abs.is_dir() {
            collect_member_files(&abs, &rel, out);
        } else if rel.ends_with("-agent-member.json") {
            out.push(rel);
        }
    }
}

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

/// Markdown files under `doc_roots`, relative-path strings, sorted/deterministic.
fn collect_markdown(root: &Path, doc_roots: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    for r in doc_roots {
        walk_md(root, r, &mut out);
    }
    out
}

fn walk_md(root: &Path, rel: &str, out: &mut Vec<String>) {
    let abs = root.join(rel);
    if !abs.exists() {
        return;
    }
    if abs.is_dir() {
        for entry in sorted_dir(&abs) {
            walk_md(root, &format!("{rel}/{entry}"), out);
        }
    } else if rel.ends_with(".md") {
        out.push(rel.to_string());
    }
}

/// Directory entry names, sorted for deterministic traversal.
fn sorted_dir(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = match std::fs::read_dir(dir) {
        Ok(rd) => rd
            .filter_map(|e| e.ok())
            .filter_map(|e| e.file_name().into_string().ok())
            .collect(),
        Err(_) => Vec::new(),
    };
    names.sort();
    names
}

/// The targets of every `[label](target)` markdown link — faithful to the JS
/// regex `\[[^\]]+\]\(([^)]+)\)` (non-empty label, non-empty target).
fn extract_link_targets(text: &str) -> Vec<String> {
    let b = text.as_bytes();
    let n = b.len();
    let mut i = 0;
    let mut out = Vec::new();
    while i < n {
        if b[i] == b'[' {
            let mut j = i + 1;
            while j < n && b[j] != b']' {
                j += 1;
            }
            // need: non-empty label (j > i+1), then "](", then non-empty target.
            if j < n && j > i + 1 && j + 1 < n && b[j + 1] == b'(' {
                let mut k = j + 2;
                while k < n && b[k] != b')' {
                    k += 1;
                }
                if k < n && k > j + 2 {
                    out.push(String::from_utf8_lossy(&b[j + 2..k]).into_owned());
                    i = k + 1;
                    continue;
                }
            }
        }
        i += 1;
    }
    out
}

/// The posix dirname of a relative path (`"docs/a/b.md"` -> `"docs/a"`,
/// `"README.md"` -> `""`). Mirrors node `path.dirname` for these inputs.
fn parent_posix(rel: &str) -> &str {
    match rel.rfind('/') {
        Some(idx) => &rel[..idx],
        None => "",
    }
}

/// posix join (`path.join`) of two relative fragments.
fn join_posix(dir: &str, target: &str) -> String {
    if dir.is_empty() {
        target.to_string()
    } else {
        format!("{dir}/{target}")
    }
}

/// Collapse `.` / `..` segments (faithful to `path.normalize` for relative paths).
fn normalize_posix(path: &str) -> String {
    let mut stack: Vec<&str> = Vec::new();
    for seg in path.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                if matches!(stack.last(), Some(&s) if s != "..") {
                    stack.pop();
                } else {
                    stack.push("..");
                }
            }
            s => stack.push(s),
        }
    }
    if stack.is_empty() {
        ".".to_string()
    } else {
        stack.join("/")
    }
}

fn is_non_empty_string(v: Option<&serde_json::Value>) -> bool {
    v.and_then(|v| v.as_str())
        .map(|s| !s.trim().is_empty())
        .unwrap_or(false)
}

fn is_string_array(v: Option<&serde_json::Value>) -> bool {
    match v.and_then(|v| v.as_array()) {
        Some(arr) => arr.iter().all(|e| is_non_empty_string(Some(e))),
        None => false,
    }
}

fn is_non_empty_string_array(v: Option<&serde_json::Value>) -> bool {
    match v.and_then(|v| v.as_array()) {
        Some(arr) => !arr.is_empty() && arr.iter().all(|e| is_non_empty_string(Some(e))),
        None => false,
    }
}

/// Parse leading `---\n ... \n---\n` frontmatter into key/value pairs, faithful
/// to the JS line regex `^([a-zA-Z0-9_-]+):\s*(.*)$` with surrounding-quote strip.
fn parse_frontmatter(text: &str) -> Option<std::collections::BTreeMap<String, String>> {
    let rest = text.strip_prefix("---\n")?;
    let end = rest.find("\n---\n")?;
    let block = &rest[..end];
    let mut fields = std::collections::BTreeMap::new();
    for line in block.split('\n') {
        if let Some(colon) = line.find(':') {
            let key = &line[..colon];
            if !key.is_empty()
                && key
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
            {
                let value = line[colon + 1..].trim_start();
                let value = strip_surrounding_quotes(value);
                fields.insert(key.to_string(), value.to_string());
            }
        }
    }
    Some(fields)
}

fn strip_surrounding_quotes(s: &str) -> &str {
    let s = s.strip_prefix(['"', '\'']).unwrap_or(s);
    s.strip_suffix(['"', '\'']).unwrap_or(s)
}

/// `true` when `s` is a real `YYYY-MM-DD` calendar date (matches JS's
/// regex + `new Date(...)`-validity rejection of impossible dates).
fn is_valid_date(s: &str) -> bool {
    let b = s.as_bytes();
    if b.len() != 10 || b[4] != b'-' || b[7] != b'-' {
        return false;
    }
    if !b.iter().enumerate().all(|(i, c)| {
        if i == 4 || i == 7 {
            true
        } else {
            c.is_ascii_digit()
        }
    }) {
        return false;
    }
    let y: i64 = s[0..4].parse().unwrap_or(0);
    let m: u32 = s[5..7].parse().unwrap_or(0);
    let d: u32 = s[8..10].parse().unwrap_or(0);
    if !(1..=12).contains(&m) {
        return false;
    }
    d >= 1 && d <= days_in_month(y, m)
}

fn days_in_month(year: i64, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if (year % 4 == 0 && year % 100 != 0) || year % 400 == 0 {
                29
            } else {
                28
            }
        }
        _ => 0,
    }
}

/// Today's UTC date as `YYYY-MM-DD`.
fn today_ymd() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let days = secs.div_euclid(86_400);
    let (y, m, d) = civil_from_days(days);
    format!("{y:04}-{m:02}-{d:02}")
}

/// Days-since-Unix-epoch -> (year, month, day). Howard Hinnant's algorithm.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (y + if m <= 2 { 1 } else { 0 }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    fn tmp(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "harness-gov-{tag}-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write(root: &Path, rel: &str, body: &str) {
        let p = root.join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, body).unwrap();
    }

    #[test]
    fn links_flags_missing_target_and_skips_external_and_anchor() {
        let root = tmp("links");
        write(
            &root,
            "docs/a.md",
            "[ok](b.md) [gone](missing.md) [ext](https://x) [anc](#h)",
        );
        write(&root, "docs/b.md", "x");
        let r = check_links(&root, &["docs".into()]);
        assert_eq!(
            r.failures,
            vec!["docs/a.md: missing link target missing.md".to_string()]
        );
        assert!(r.summary.contains("checked 2 markdown files"));
    }

    #[test]
    fn links_resolves_parent_relative() {
        let root = tmp("links-parent");
        write(&root, "README.md", "root");
        write(&root, "docs/a.md", "[up](../README.md)");
        let r = check_links(&root, &["README.md".into(), "docs".into()]);
        assert!(r.failures.is_empty(), "got {:?}", r.failures);
    }

    #[test]
    fn size_warns_over_limit_never_blocks() {
        let root = tmp("size");
        write(&root, "docs/big.md", &"x\n".repeat(600));
        write(&root, "docs/ok.md", "small");
        let r = check_size(&root, &["docs".into()], 500);
        assert_eq!(r.severity, Severity::Warning);
        assert!(!r.is_blocking_failure());
        assert_eq!(r.warnings.len(), 1);
        assert!(r.warnings[0].contains("docs/big.md: 601 lines exceeds 500"));
    }

    fn valid_doc(path: &str) -> serde_json::Value {
        serde_json::json!({
            "path": path, "ownerRole": "lead", "status": "stable", "lifecycle": "stable",
            "canonicalFor": ["x"], "dependsOn": [], "machineConsumers": ["ci"],
            "reviewAfter": "2999-01-01", "lastVerifiedWith": ["test"], "reorgTrigger": "when X"
        })
    }

    fn reg_cfg() -> RegistryConfig {
        let GovernanceConfig { registry, .. } = GovernanceConfig::default_harness();
        let mut r = registry.unwrap();
        r.core_docs = vec!["README.md".into()];
        r
    }

    #[test]
    fn governance_passes_valid_registry() {
        let root = tmp("gov-ok");
        write(&root, "README.md", "x");
        let registry = serde_json::json!({
            "schema": "agent_harness.docs_registry.v1",
            "documents": [valid_doc("README.md")]
        });
        write(&root, "docs/registry.json", &registry.to_string());
        let r = check_governance(&root, &reg_cfg(), "2026-06-21");
        assert!(r.failures.is_empty(), "got {:?}", r.failures);
    }

    #[test]
    fn governance_flags_bad_status_missing_field_and_missing_core_doc() {
        let root = tmp("gov-bad");
        write(&root, "README.md", "x");
        write(&root, "intro.md", "x");
        let mut doc = valid_doc("intro.md");
        doc["status"] = serde_json::json!("nope");
        doc.as_object_mut().unwrap().remove("reorgTrigger");
        let registry = serde_json::json!({
            "schema": "agent_harness.docs_registry.v1", "documents": [doc]
        });
        write(&root, "docs/registry.json", &registry.to_string());
        let r = check_governance(&root, &reg_cfg(), "2026-06-21");
        assert!(r.failures.iter().any(|f| f.contains("invalid status nope")));
        assert!(r
            .failures
            .iter()
            .any(|f| f.contains("missing reorgTrigger")));
        assert!(r
            .failures
            .iter()
            .any(|f| f.contains("missing core doc README.md")));
    }

    #[test]
    fn governance_stale_review_after_is_warning_not_failure() {
        let root = tmp("gov-stale");
        write(&root, "README.md", "x");
        let mut doc = valid_doc("README.md");
        doc["reviewAfter"] = serde_json::json!("2020-01-01");
        let registry = serde_json::json!({
            "schema": "agent_harness.docs_registry.v1", "documents": [doc]
        });
        write(&root, "docs/registry.json", &registry.to_string());
        let r = check_governance(&root, &reg_cfg(), "2026-06-21");
        assert!(r.failures.is_empty(), "got {:?}", r.failures);
        assert!(r
            .warnings
            .iter()
            .any(|w| w.contains("reviewAfter is stale: 2020-01-01")));
    }

    #[test]
    fn governance_missing_registry_fails() {
        let root = tmp("gov-missing");
        let r = check_governance(&root, &reg_cfg(), "2026-06-21");
        assert!(r
            .failures
            .iter()
            .any(|f| f.contains("missing docs governance registry")));
    }

    #[test]
    fn skills_validates_frontmatter_and_metadata() {
        let root = tmp("skills-ok");
        write(&root, "skills/good/SKILL.md", "---\nname: good\ndescription: a sufficiently long and specific description of the skill\n---\nbody");
        write(
            &root,
            "skills/good/agents/openai.yaml",
            "display_name: G\nshort_description: g\ndefault_prompt: do",
        );
        let r = check_skills(&root, &["skills".into()], None);
        assert!(r.failures.is_empty(), "got {:?}", r.failures);
        assert!(r.summary.contains("checked 1 skills"));
    }

    #[test]
    fn skills_flags_name_mismatch_and_short_description() {
        let root = tmp("skills-bad");
        write(
            &root,
            "skills/mine/SKILL.md",
            "---\nname: other\ndescription: short\n---\n",
        );
        write(
            &root,
            "skills/mine/agents/openai.yaml",
            "display_name: M\nshort_description: m\ndefault_prompt: do",
        );
        let r = check_skills(&root, &["skills".into()], None);
        assert!(r
            .failures
            .iter()
            .any(|f| f.contains("name must match folder name mine")));
        assert!(r
            .failures
            .iter()
            .any(|f| f.contains("description must be complete")));
    }

    #[test]
    fn skills_flags_dangling_member_ref() {
        let root = tmp("skills-ref");
        write(&root, "skills/real/SKILL.md", "---\nname: real\ndescription: a sufficiently long and specific description of the skill\n---\n");
        write(
            &root,
            "skills/real/agents/openai.yaml",
            "display_name: R\nshort_description: r\ndefault_prompt: do",
        );
        write(
            &root,
            ".agents/data/x-agent-member.json",
            "{\"skill_refs\":[\"real\",\"ghost\"]}",
        );
        let r = check_skills(&root, &["skills".into()], Some(".agents/data"));
        assert!(r
            .failures
            .iter()
            .any(|f| f.contains("skill_ref \"ghost\" does not exist")));
        assert!(!r.failures.iter().any(|f| f.contains("\"real\"")));
    }

    #[test]
    fn date_validity_matches_calendar() {
        assert!(is_valid_date("2026-06-21"));
        assert!(is_valid_date("2024-02-29"));
        assert!(!is_valid_date("2026-02-30"));
        assert!(!is_valid_date("2026-13-01"));
        assert!(!is_valid_date("2026-6-1"));
        assert!(!is_valid_date("not-a-date"));
    }

    /// The permanent regression gate (design "self-host"): the harness exercises
    /// the exact engine it ships against its own repo, via its committed
    /// `.governance.toml`. Catches port drift and keeps this repo governance-green.
    #[test]
    fn self_host_repo_is_governance_green() {
        let repo = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .expect("repo root from crate manifest")
            .to_path_buf();
        // Only meaningful in a real checkout; skip if the registry is absent.
        if !repo.join("docs").join("registry.json").exists() {
            return;
        }
        let config = GovernanceConfig::load(&repo).expect("load .governance.toml");
        let report = run_check(&repo, &config);
        for gate in &report.gates {
            assert!(
                gate.failures.is_empty(),
                "governance gate `{}` failed on this repo: {:?}",
                gate.kind,
                gate.failures
            );
        }
        assert!(report.passed(), "this repo must be governance-green");
    }

    #[test]
    fn normalize_collapses_parent_segments() {
        assert_eq!(normalize_posix("docs/../README.md"), "README.md");
        assert_eq!(normalize_posix("docs/./a.md"), "docs/a.md");
        assert_eq!(normalize_posix("a/b/../c.md"), "a/c.md");
    }
}
