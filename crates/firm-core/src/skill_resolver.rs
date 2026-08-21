use std::path::PathBuf;

/// Result of resolving a skill reference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedSkill {
    /// The skill id (matches `.agents/skills/<id>/`)
    pub id: String,
    /// The absolute or relative path to SKILL.md
    pub path: PathBuf,
    /// The full content of SKILL.md (header + body)
    pub content: String,
}

/// Error type for skill resolution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SkillResolutionError {
    /// The skill reference does not resolve to an existing SKILL.md.
    SkillNotFound { skill_id: String, path: PathBuf },
    /// An IO error occurred while reading the skill file.
    IoError { skill_id: String, reason: String },
}

impl std::fmt::Display for SkillResolutionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SkillResolutionError::SkillNotFound { skill_id, path } => {
                write!(f, "skill '{}' not found at {}", skill_id, path.display())
            }
            SkillResolutionError::IoError { skill_id, reason } => {
                write!(f, "failed to read skill '{}': {}", skill_id, reason)
            }
        }
    }
}

impl std::error::Error for SkillResolutionError {}

/// Resolve a single skill reference using the given skills root directory.
///
/// The contract: a skill_ref `<id>` resolves to `.agents/skills/<id>/SKILL.md`.
/// If the file exists and is readable, returns the content and path.
/// If not found or unreadable, returns SkillResolutionError.
///
/// This function is synchronous and does not require a live provider binary.
pub fn resolve_skill(
    skill_id: &str,
    skills_root: &std::path::Path,
) -> Result<ResolvedSkill, SkillResolutionError> {
    let skill_path = skills_root.join(skill_id).join("SKILL.md");
    let content =
        std::fs::read_to_string(&skill_path).map_err(|e| SkillResolutionError::IoError {
            skill_id: skill_id.to_string(),
            reason: e.to_string(),
        })?;
    Ok(ResolvedSkill {
        id: skill_id.to_string(),
        path: skill_path,
        content,
    })
}

/// Resolve all skill references at once using the given skills root directory.
///
/// Returns a Vec of resolved skills in the order they appear in the input.
/// If any skill fails to resolve, returns an error (fail-fast); the caller
/// must decide whether to report it or continue.
pub fn resolve_skills(
    skill_ids: &[String],
    skills_root: &std::path::Path,
) -> Result<Vec<ResolvedSkill>, SkillResolutionError> {
    let mut resolved = Vec::new();
    for id in skill_ids {
        resolved.push(resolve_skill(id, skills_root)?);
    }
    Ok(resolved)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skill_resolution_error_displays_clearly() {
        let err = SkillResolutionError::SkillNotFound {
            skill_id: "my-skill".to_string(),
            path: PathBuf::from(".agents/skills/my-skill/SKILL.md"),
        };
        let msg = err.to_string();
        assert!(msg.contains("my-skill"));
        assert!(msg.contains(".agents/skills"));
    }

    #[test]
    fn skill_not_found_error() {
        let result = resolve_skill("nonexistent", PathBuf::from(".").as_path());
        assert!(result.is_err());
        match result {
            Err(SkillResolutionError::IoError { skill_id, .. }) => {
                assert_eq!(skill_id, "nonexistent");
            }
            _ => panic!("expected IoError"),
        }
    }
}
