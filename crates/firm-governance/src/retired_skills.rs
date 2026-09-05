//! Retired-skill exclusion for the `skills` gate.
//!
//! Retired skills must never reappear as directories (or symlinks to
//! directories) directly under any configured `skill_roots` entry. The check
//! scans the filesystem, so an ignored or untracked local copy fails the same
//! way as a committed one. Copies outside the skill roots are never scanned;
//! retired skill sources themselves live only in git history (ADR 0063).

use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// Skill names retired from every active skill root (`.governance.toml`
/// `[retired_skills]`).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RetiredSkillsConfig {
    /// Exact directory names that must not appear under any `skill_roots`
    /// entry.
    pub names: Vec<String>,
}

/// The configured retired names as a lookup set (empty when unconfigured).
pub(crate) fn retired_name_set(retired: Option<&RetiredSkillsConfig>) -> BTreeSet<&str> {
    retired
        .map(|config| config.names.iter().map(String::as_str).collect())
        .unwrap_or_default()
}

/// The blocking finding for one retired skill name found under a skill root.
pub(crate) fn retired_skill_finding(skills_root: &str, entry: &str) -> String {
    format!(
        "{skills_root}/{entry}: retired skill name must not appear under a skill root; \
         remove the copy (retired skill sources live only in git history, \
         never under a skill root)"
    )
}
