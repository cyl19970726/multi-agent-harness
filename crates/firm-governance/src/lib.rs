//! Project-portable doc-governance gates, compiled into the harness binary.
//!
//! This crate is the harness-native home of the four documentation/skill gates
//! that were historically `scripts/check-doc-*.mjs` + `check-skills.mjs` (node /
//! pnpm only). The logic is a faithful 1:1 port — same roots, same rules, same
//! messages — so a project the harness operates on gets the SAME closed-loop
//! governance with zero hosted scripts and no node/pnpm dependency. Gate
//! parameters come from a per-project [`GovernanceConfig`] (today: this repo's
//! `.firm/governance.toml`, which mirrors the old hardcoded constants), so a
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
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

mod retired_skills;
pub use retired_skills::RetiredSkillsConfig;

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

/// Per-project governance configuration (`.governance.toml`). Absent a
/// config file the firm default ([`GovernanceConfig::default_firm`]) is
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
    /// Source roots scanned by the `size` gate (maintained Rust and JavaScript
    /// family sources). Empty disables the source half of the gate.
    #[serde(default)]
    pub source_roots: Vec<String>,
    /// Max source line count before the `size` gate blocks the change.
    #[serde(default = "default_source_max_lines")]
    pub source_max_lines: usize,
    /// Root scanned for `*-agent-member.json` skill_ref validation (optional).
    #[serde(default)]
    pub member_data_root: Option<String>,
    /// Registry gate config. Absent → the `registry` gate is skipped (a project
    /// with no doc registry still gets links/size/skills).
    #[serde(default)]
    pub registry: Option<RegistryConfig>,
    /// Optional blocker that prevents explicitly retired product language from
    /// returning to active registered documents. Archival/deprecated entries
    /// and lines that clearly label migration/history remain readable.
    #[serde(default)]
    pub retired_vocabulary: Option<RetiredVocabularyConfig>,
    /// Optional blocker that keeps retired skill names out of every
    /// `skill_roots` entry, including ignored or untracked local copies.
    #[serde(default)]
    pub retired_skills: Option<RetiredSkillsConfig>,
    /// Exact document contracts that must keep teaching the same product
    /// invariants. Unlike retired vocabulary, this can cover unregistered
    /// repository entry points such as `AGENTS.md`.
    #[serde(default)]
    pub document_invariants: Vec<DocumentInvariantConfig>,
}

/// Config for the `registry` gate (the doc-governance registry validator).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryConfig {
    pub path: String,
    pub schema: String,
    pub required_fields: Vec<String>,
    pub allowed_statuses: Vec<String>,
    pub allowed_lifecycles: Vec<String>,
    /// Semantic role of the document, independent of maturity/lifecycle.
    #[serde(default)]
    pub allowed_authority_classes: Vec<String>,
    /// Honest implementation maturity of the capability described by the doc.
    #[serde(default)]
    pub allowed_implementation_states: Vec<String>,
    /// Allowed typed references from documentation claims to executable truth.
    #[serde(default)]
    pub allowed_truth_ref_kinds: Vec<String>,
    pub core_docs: Vec<String>,
    /// Roots whose Markdown files must all appear in the registry. This catches
    /// important but invisible documents, complementing `core_docs`.
    #[serde(default)]
    pub coverage_roots: Vec<String>,
    /// Exact repository-relative paths intentionally excluded from coverage.
    #[serde(default)]
    pub coverage_exclude: Vec<String>,
}

/// Rules for the active-document retired-vocabulary gate.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetiredVocabularyConfig {
    /// Exact, case-sensitive phrases that must not be taught as current.
    pub terms: Vec<String>,
    /// Registered paths that intentionally own compatibility or migration text.
    #[serde(default)]
    pub allowed_paths: Vec<String>,
    /// Case-insensitive markers that make a matching line explicitly historical.
    #[serde(default)]
    pub context_markers: Vec<String>,
}

/// Required and forbidden exact terms for a set of authoritative documents.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentInvariantConfig {
    /// Stable label used in gate failures.
    pub name: String,
    /// Repository-relative files. Every file is checked independently.
    pub paths: Vec<String>,
    /// Every path must contain every required term.
    #[serde(default)]
    pub required_terms: Vec<String>,
    /// No path may contain any forbidden term.
    #[serde(default)]
    pub forbidden_terms: Vec<String>,
}

impl GovernanceConfig {
    /// The default profile for THIS repository — a faithful mirror of the
    /// constants in the historic `scripts/check-doc-*.mjs` + `check-skills.mjs`.
    pub fn default_firm() -> Self {
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
            source_roots: vec!["crates".into(), "apps".into()],
            source_max_lines: default_source_max_lines(),
            member_data_root: Some(s(".agents/data")),
            registry: Some(RegistryConfig {
                path: s("docs/registry.json"),
                schema: s("agent_harness.docs_registry.v1"),
                required_fields: [
                    "path",
                    "ownerRole",
                    "status",
                    "lifecycle",
                    "authorityClass",
                    "implementationState",
                    "truthRefs",
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
                allowed_authority_classes: [
                    "entry",
                    "canonical_contract",
                    "implementation_reference",
                    "design_intent",
                    "actual_evidence",
                    "research",
                    "historical_evidence",
                ]
                .iter()
                .map(|v| s(v))
                .collect(),
                allowed_implementation_states: [
                    "design_only",
                    "partial",
                    "implemented",
                    "verified",
                ]
                .iter()
                .map(|v| s(v))
                .collect(),
                allowed_truth_ref_kinds: [
                    "schema",
                    "store",
                    "api",
                    "ui",
                    "test",
                    "decision",
                    "runtime_evidence",
                ]
                .iter()
                .map(|v| s(v))
                .collect(),
                core_docs: [
                    "README.md",
                    "docs/README.md",
                    "docs/documentation-governance.md",
                    "docs/prd.md",
                    "docs/design-basis.md",
                    "docs/architecture.md",
                    "docs/operations.md",
                    "docs/schemas.md",
                    "docs/decisions/README.md",
                    "docs/company-os/product-system-map.md",
                ]
                .iter()
                .map(|v| s(v))
                .collect(),
                coverage_roots: [
                    "docs/company-os",
                    "docs/dashboard/pages",
                    "docs/integration",
                ]
                .iter()
                .map(|v| s(v))
                .collect(),
                coverage_exclude: Vec::new(),
            }),
            retired_vocabulary: None,
            retired_skills: None,
            document_invariants: Vec::new(),
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
            source_roots: vec!["crates".into(), "apps".into()],
            source_max_lines: default_source_max_lines(),
            member_data_root: None,
            registry: None,
            retired_vocabulary: None,
            retired_skills: None,
            document_invariants: Vec::new(),
        }
    }

    /// Load `<root>/.governance.toml`, or the light default when absent.
    ///
    /// The committed config lives at the PROJECT ROOT (not under `.firm/`,
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
        check_size(
            root,
            &config.doc_roots,
            config.max_lines,
            &config.source_roots,
            config.source_max_lines,
        ),
        check_skills(
            root,
            &config.skill_roots,
            config.member_data_root.as_deref(),
            config.retired_skills.as_ref(),
        ),
    ];
    if let Some(reg) = &config.registry {
        gates.push(check_governance(root, reg, today));
        if let Some(retired) = &config.retired_vocabulary {
            gates.push(check_retired_vocabulary(root, reg, retired));
        }
    }
    if !config.document_invariants.is_empty() {
        gates.push(check_document_invariants(root, &config.document_invariants));
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
            if root.join(&target).exists() {
                continue;
            }
            // A file reached through a symlinked doc root writes its relative
            // links against its REAL location, not the symlink path — resolve
            // once more from the canonicalized parent before failing.
            let real_target_exists = std::fs::canonicalize(root.join(rel))
                .ok()
                .and_then(|real| real.parent().map(|p| p.join(without_hash)))
                .map(|p| p.exists())
                .unwrap_or(false);
            if !real_target_exists {
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
// gate: size
// ---------------------------------------------------------------------------

/// Markdown size: warn (never block) when a file exceeds `max_lines`.
fn default_source_max_lines() -> usize {
    1500
}

/// Collect maintained sources beneath the configured roots, skipping hidden,
/// build-output, dependency, and vendored trees.
fn collect_sources(root: &Path, source_roots: &[String]) -> Vec<String> {
    let mut files = Vec::new();
    for source_root in source_roots {
        let base = root.join(source_root);
        let mut stack = vec![base];
        while let Some(dir) = stack.pop() {
            let Ok(entries) = std::fs::read_dir(&dir) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                let name = entry.file_name().to_string_lossy().into_owned();
                if path.is_dir() {
                    if name != "target" && name != "node_modules" && !name.starts_with('.') {
                        stack.push(path);
                    }
                } else if matches!(
                    path.extension().and_then(|extension| extension.to_str()),
                    Some("rs" | "ts" | "tsx" | "js" | "mjs" | "cjs")
                ) {
                    if let Ok(rel) = path.strip_prefix(root) {
                        files.push(rel.display().to_string());
                    }
                }
            }
        }
    }
    files.sort();
    files
}

pub fn check_size(
    root: &Path,
    doc_roots: &[String],
    max_lines: usize,
    source_roots: &[String],
    source_max_lines: usize,
) -> GateReport {
    let files = collect_markdown(root, doc_roots);
    let mut warnings = Vec::new();
    let mut failures = Vec::new();
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
    for rel in collect_sources(root, source_roots) {
        let Ok(text) = std::fs::read_to_string(root.join(&rel)) else {
            continue;
        };
        let line_count = text.split('\n').count();
        if line_count > source_max_lines {
            failures.push(format!(
                "{rel}: {line_count} lines exceeds {source_max_lines}; extract a cohesive owner boundary"
            ));
        }
    }
    // Faithful to check-doc-size.mjs: it prints EITHER the warnings OR the
    // success line, never both. An empty summary suppresses the success line when
    // there are warnings (the printer skips empty summaries).
    let summary = if warnings.is_empty() && failures.is_empty() {
        format!(
            "all markdown files are <= {max_lines} lines and maintained sources are <= {source_max_lines} lines"
        )
    } else {
        String::new()
    };
    GateReport {
        kind: "size".into(),
        severity: Severity::Blocker,
        failures,
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
    let allowed_authority_classes: BTreeSet<&str> = cfg
        .allowed_authority_classes
        .iter()
        .map(String::as_str)
        .collect();
    let allowed_implementation_states: BTreeSet<&str> = cfg
        .allowed_implementation_states
        .iter()
        .map(String::as_str)
        .collect();
    let allowed_truth_ref_kinds: BTreeSet<&str> = cfg
        .allowed_truth_ref_kinds
        .iter()
        .map(String::as_str)
        .collect();
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut canonical_owners: BTreeMap<String, String> = BTreeMap::new();

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
        match doc.get("authorityClass").and_then(|v| v.as_str()) {
            Some(value) if allowed_authority_classes.contains(value) => {}
            other => failures.push(format!(
                "{label}: invalid authorityClass {}",
                other.unwrap_or("undefined")
            )),
        }
        let implementation_state = doc
            .get("implementationState")
            .and_then(|value| value.as_str());
        match implementation_state {
            Some(value) if allowed_implementation_states.contains(value) => {}
            other => failures.push(format!(
                "{label}: invalid implementationState {}",
                other.unwrap_or("undefined")
            )),
        }
        let mut has_acceptance_truth = false;
        match doc.get("truthRefs").and_then(|value| value.as_array()) {
            Some(refs) => {
                for (truth_index, truth_ref) in refs.iter().enumerate() {
                    let truth_label = format!("{label}: truthRefs[{truth_index}]");
                    let kind = truth_ref.get("kind").and_then(|value| value.as_str());
                    let reference = truth_ref.get("ref").and_then(|value| value.as_str());
                    if !matches!(kind, Some(value) if allowed_truth_ref_kinds.contains(value)) {
                        failures.push(format!(
                            "{truth_label}: invalid kind {}",
                            kind.unwrap_or("undefined")
                        ));
                    }
                    if reference.map(str::is_empty).unwrap_or(true) {
                        failures.push(format!("{truth_label}: ref must be a non-empty string"));
                    }
                    if kind == Some("test") || kind == Some("runtime_evidence") {
                        has_acceptance_truth = true;
                    }
                }
                if implementation_state == Some("verified") && !has_acceptance_truth {
                    failures.push(format!(
                        "{label}: verified implementationState requires a test or runtime_evidence truthRef"
                    ));
                }
                if matches!(implementation_state, Some("implemented" | "verified"))
                    && refs.is_empty()
                {
                    failures.push(format!(
                        "{label}: implemented or verified implementationState requires at least one truthRef"
                    ));
                }
            }
            None => failures.push(format!("{label}: truthRefs must be an array")),
        }
        if !is_non_empty_string_array(doc.get("canonicalFor")) {
            failures.push(format!(
                "{label}: canonicalFor must be a non-empty string array"
            ));
        } else {
            let status = doc.get("status").and_then(|value| value.as_str());
            let lifecycle = doc.get("lifecycle").and_then(|value| value.as_str());
            let is_active =
                !matches!(status, Some("deprecated" | "archival")) && lifecycle != Some("archival");
            if is_active {
                for scope in doc
                    .get("canonicalFor")
                    .and_then(|value| value.as_array())
                    .into_iter()
                    .flatten()
                    .filter_map(|value| value.as_str())
                {
                    if let Some(owner) = canonical_owners.get(scope) {
                        failures.push(format!(
                            "{label}: active canonical scope `{scope}` is already owned by {owner}"
                        ));
                    } else {
                        canonical_owners.insert(scope.to_string(), doc_path.clone());
                    }
                }
            }
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

    let coverage_exclude: BTreeSet<&str> =
        cfg.coverage_exclude.iter().map(String::as_str).collect();
    for path in collect_markdown(root, &cfg.coverage_roots) {
        if !coverage_exclude.contains(path.as_str()) && !seen.contains(&path) {
            failures.push(format!(
                "{registry_path}: active coverage path is not registered: {path}"
            ));
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
// gate: retired vocabulary
// ---------------------------------------------------------------------------

/// Prevent retired product vocabulary from being presented as current in
/// active registry documents. Historical and migration material remains
/// available through archival/deprecated entries or explicit context markers.
/// One scan unit for the retired-vocabulary check: the sentence text plus the
/// 1-based line where it starts, so a failure still points at a real line.
struct RetiredVocabularySentence {
    text: String,
    line: usize,
    /// The block's source lines, so a failure points at the line carrying the
    /// term rather than at the start of a wrapped paragraph.
    lines: Vec<(usize, String)>,
}

impl RetiredVocabularySentence {
    fn line_of(&self, term: &str) -> usize {
        self.lines
            .iter()
            .find(|(_, line)| line.contains(term))
            .map(|(number, _)| *number)
            .unwrap_or(self.line)
    }
}

/// Split a Markdown document into sentence-sized scan units.
///
/// Blocks break on blank lines, headings, fences and table rows, so unrelated
/// prose never shares a unit. Inside a block the wrapped lines are joined
/// first (a hard wrap must not tear a labeled sentence in half) and then split
/// on sentence punctuation; table rows split per cell instead, because a
/// marker in one cell says nothing about another.
fn retired_vocabulary_sentences(text: &str) -> Vec<RetiredVocabularySentence> {
    let mut units = Vec::new();
    let mut block: Vec<(usize, &str)> = Vec::new();
    let push_block = |block: &mut Vec<(usize, &str)>,
                      units: &mut Vec<RetiredVocabularySentence>| {
        if block.is_empty() {
            return;
        }
        let start_line = block[0].0;
        let block_lines: Vec<(usize, String)> =
            block.iter().map(|(n, l)| (*n, (*l).to_string())).collect();
        let is_table_row = block.len() == 1 && block[0].1.trim_start().starts_with('|');
        let joined = block
            .iter()
            .map(|(_, line)| line.trim())
            .collect::<Vec<_>>()
            .join(" ");
        block.clear();
        // Map each character offset in the joined text back to its source
        // line, so a failure points at the line carrying the term even when
        // the sentence is wrapped across several lines.
        let mut offsets: Vec<(usize, usize)> = Vec::new();
        let mut cursor = 0usize;
        for (number, line) in &block_lines {
            offsets.push((cursor, *number));
            cursor += line.trim().chars().count() + 1;
        }
        let pieces: Vec<String> = if is_table_row {
            // A table row is one statement: its status/disposition cell labels
            // the whole row, so per-cell splitting would flag correctly
            // labeled rows.
            vec![joined.clone()]
        } else {
            split_into_sentences(&joined)
        };
        let mut consumed = 0usize;
        for piece in pieces {
            let piece_start = consumed;
            consumed += piece.chars().count();
            if piece.trim().is_empty() {
                continue;
            }
            let piece_end = consumed;
            let lines = block_lines
                .iter()
                .zip(offsets.iter())
                .filter(|(_, (offset, _))| *offset < piece_end)
                .filter(|((_, line), (offset, _))| {
                    offset + line.trim().chars().count() >= piece_start
                })
                .map(|((number, line), _)| (*number, line.clone()))
                .collect::<Vec<_>>();
            units.push(RetiredVocabularySentence {
                text: piece,
                line: lines.first().map(|(n, _)| *n).unwrap_or(start_line),
                lines,
            });
        }
    };
    let mut in_fence = false;
    for (index, line) in text.lines().enumerate() {
        let line_no = index + 1;
        let trimmed = line.trim_start();
        let is_fence = trimmed.starts_with("```") || trimmed.starts_with("~~~");
        if is_fence {
            push_block(&mut block, &mut units);
            in_fence = !in_fence;
            continue;
        }
        // Fenced content is line-oriented (status blocks, code, command
        // transcripts); joining it as prose would fuse unrelated fields.
        let standalone = in_fence || trimmed.starts_with('#') || trimmed.starts_with('|');
        if trimmed.is_empty() {
            push_block(&mut block, &mut units);
            continue;
        }
        if standalone {
            push_block(&mut block, &mut units);
            block.push((line_no, line));
            push_block(&mut block, &mut units);
            continue;
        }
        block.push((line_no, line));
    }
    push_block(&mut block, &mut units);
    units
}

/// Sentence split on terminal punctuation followed by whitespace. Keeps the
/// punctuation with its sentence so quoted contract phrases stay intact.
fn split_into_sentences(text: &str) -> Vec<String> {
    let mut sentences = Vec::new();
    let mut current = String::new();
    let mut depth = 0i32;
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        current.push(ch);
        match ch {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth = (depth - 1).max(0),
            _ => {}
        }
        // A parenthetical `(DOC-108; ADR 0027 superseded)` is one clause: its
        // label must still cover the term it qualifies.
        if depth == 0
            && matches!(ch, '.' | ';' | '!' | '?')
            && chars.peek().is_some_and(|next| next.is_whitespace())
        {
            sentences.push(std::mem::take(&mut current));
        }
    }
    if !current.trim().is_empty() {
        sentences.push(current);
    }
    sentences
}

pub fn check_retired_vocabulary(
    root: &Path,
    registry_cfg: &RegistryConfig,
    cfg: &RetiredVocabularyConfig,
) -> GateReport {
    let mut failures = Vec::new();
    let registry_path = root.join(&registry_cfg.path);
    let raw = match std::fs::read_to_string(&registry_path) {
        Ok(raw) => raw,
        Err(e) => {
            failures.push(format!("{}: {e}", registry_cfg.path));
            return retired_vocabulary_report(failures, 0);
        }
    };
    let registry: serde_json::Value = match serde_json::from_str(&raw) {
        Ok(value) => value,
        Err(e) => {
            failures.push(format!("{}: {e}", registry_cfg.path));
            return retired_vocabulary_report(failures, 0);
        }
    };
    let Some(documents) = registry.get("documents").and_then(|value| value.as_array()) else {
        failures.push(format!("{}: documents must be an array", registry_cfg.path));
        return retired_vocabulary_report(failures, 0);
    };

    let allowed_paths: BTreeSet<&str> = cfg.allowed_paths.iter().map(String::as_str).collect();
    let context_markers: Vec<String> = cfg
        .context_markers
        .iter()
        .map(|marker| marker.to_lowercase())
        .collect();
    let mut checked = 0usize;

    for doc in documents {
        let status = doc
            .get("status")
            .and_then(|value| value.as_str())
            .unwrap_or("");
        let lifecycle = doc
            .get("lifecycle")
            .and_then(|value| value.as_str())
            .unwrap_or("");
        if matches!(status, "deprecated" | "archival") || lifecycle == "archival" {
            continue;
        }
        let Some(path) = doc.get("path").and_then(|value| value.as_str()) else {
            continue;
        };
        if allowed_paths.contains(path) || !path.ends_with(".md") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(root.join(path)) else {
            continue;
        };
        checked += 1;
        // Sentence-scoped, not line-scoped. A context marker exempts only the
        // sentence it appears in: a line may carry an unrelated `legacy` (say,
        // modifying a different noun) while a neighbouring clause asserts a
        // retired object as current authority, and line scoping laundered
        // exactly that shape past the gate. Joining a wrapped paragraph before
        // splitting also stops a hard wrap from tearing a legitimately labeled
        // sentence in half.
        for sentence in retired_vocabulary_sentences(&text) {
            let lower = sentence.text.to_lowercase();
            if context_markers.iter().any(|marker| lower.contains(marker)) {
                continue;
            }
            for term in &cfg.terms {
                if sentence.text.contains(term) {
                    failures.push(format!(
                        "{path}:{}: retired vocabulary `{term}` needs explicit historical context or replacement",
                        sentence.line_of(term)
                    ));
                }
            }
        }
    }

    retired_vocabulary_report(failures, checked)
}

fn retired_vocabulary_report(failures: Vec<String>, checked: usize) -> GateReport {
    GateReport {
        kind: "retired_vocabulary".into(),
        severity: Severity::Blocker,
        failures,
        warnings: Vec::new(),
        summary: format!("checked {checked} active registered markdown documents"),
    }
}

// ---------------------------------------------------------------------------
// gate: authoritative document invariants
// ---------------------------------------------------------------------------

/// Require authoritative entry points to state the same exact product
/// invariants and reject known superseded formulations.
pub fn check_document_invariants(root: &Path, configs: &[DocumentInvariantConfig]) -> GateReport {
    let mut failures = Vec::new();
    let mut checked = 0usize;

    for config in configs {
        if config.paths.is_empty() {
            failures.push(format!(
                "{}: document invariant must declare at least one path",
                config.name
            ));
            continue;
        }
        for path in &config.paths {
            let text = match std::fs::read_to_string(root.join(path)) {
                Ok(text) => text,
                Err(error) => {
                    failures.push(format!(
                        "{}: {path}: cannot read authoritative document: {error}",
                        config.name
                    ));
                    continue;
                }
            };
            checked += 1;
            for term in &config.required_terms {
                if !text.contains(term) {
                    failures.push(format!(
                        "{}: {path}: missing required invariant `{term}`",
                        config.name
                    ));
                }
            }
            for term in &config.forbidden_terms {
                if text.contains(term) {
                    failures.push(format!(
                        "{}: {path}: contains superseded invariant `{term}`",
                        config.name
                    ));
                }
            }
        }
    }

    GateReport {
        kind: "document_invariants".into(),
        severity: Severity::Blocker,
        failures,
        warnings: Vec::new(),
        summary: format!(
            "checked {checked} authoritative documents across {} invariant sets",
            configs.len()
        ),
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
    retired_skills: Option<&RetiredSkillsConfig>,
) -> GateReport {
    let mut failures = Vec::new();
    let mut checked = 0usize;
    let mut resolved: BTreeSet<String> = BTreeSet::new();
    let retired_names = retired_skills::retired_name_set(retired_skills);

    for skills_root in skill_roots {
        let abs_root = root.join(skills_root);
        if !abs_root.exists() {
            continue;
        }
        for entry in sorted_dir(&abs_root) {
            // Retired names fail before the symlink skip: a retired copy
            // symlinked into a skill root is still a retired copy, and the
            // name check does not depend on the entry being a valid skill.
            if retired_names.contains(entry.as_str()) {
                failures.push(retired_skills::retired_skill_finding(skills_root, &entry));
                continue;
            }
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
            // Vendored dependency trees are not repository documentation.
            // `apps/claude-member-runner/node_modules` appears after installing
            // the runner's SDK for live use and must not fail the link/line
            // checks meant for authored docs.
            if entry == "node_modules" {
                continue;
            }
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
#[path = "lib_tests.rs"]
mod tests;
