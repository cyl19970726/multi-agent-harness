//! Execution Space identity and registry (ADR 0042).
//!
//! An Execution Space owns coordination ledgers. It does not own provider cwd,
//! Git state, project instructions, Skills, or Company OS truth.

use std::path::{Path, PathBuf};

use firm_core::ExecutionSpace;
use serde::{Deserialize, Serialize};

use crate::project;

const REGISTRY_FORMAT_VERSION: u32 = 1;

#[derive(Debug)]
pub enum ExecutionSpaceError {
    Io(std::io::Error),
    Json(serde_json::Error),
    InvalidId(String),
    NoHome,
}

impl std::fmt::Display for ExecutionSpaceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(f, "execution space io error: {error}"),
            Self::Json(error) => write!(f, "execution space json error: {error}"),
            Self::InvalidId(id) => write!(
                f,
                "invalid execution space id `{id}`; use letters, digits, '.', '_' or '-'"
            ),
            Self::NoHome => write!(f, "could not determine harness home"),
        }
    }
}

impl std::error::Error for ExecutionSpaceError {}

impl From<std::io::Error> for ExecutionSpaceError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<serde_json::Error> for ExecutionSpaceError {
    fn from(value: serde_json::Error) -> Self {
        Self::Json(value)
    }
}

pub type ExecutionSpaceResult<T> = Result<T, ExecutionSpaceError>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutionSpaceRegistryEntry {
    pub id: String,
    pub name: String,
    pub store_root: PathBuf,
    #[serde(default)]
    pub default_project_binding_id: Option<String>,
    #[serde(default)]
    pub company_id: Option<String>,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub last_opened_at: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ExecutionSpaceRegistry {
    #[serde(default)]
    pub format_version: u32,
    #[serde(default)]
    pub current_space_id: Option<String>,
    #[serde(default)]
    pub spaces: Vec<ExecutionSpaceRegistryEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutionSpaceMetadata {
    pub space_id: String,
    pub name: String,
    #[serde(default)]
    pub default_project_binding_id: Option<String>,
    #[serde(default)]
    pub company_id: Option<String>,
}

pub fn spaces_dir(firm_home: &Path) -> PathBuf {
    firm_home.join("execution-spaces")
}

pub fn space_store_root(firm_home: &Path, id: &str) -> PathBuf {
    spaces_dir(firm_home).join(id)
}

pub fn registry_path(firm_home: &Path) -> PathBuf {
    spaces_dir(firm_home).join("registry.json")
}

pub fn active_space_path(firm_home: &Path) -> PathBuf {
    firm_home.join("ACTIVE_SPACE")
}

pub fn validate_space_id(id: &str) -> ExecutionSpaceResult<()> {
    if id.is_empty() || id == "." || id == ".." {
        return Err(ExecutionSpaceError::InvalidId(id.to_string()));
    }
    if id
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || "._-".contains(character))
    {
        Ok(())
    } else {
        Err(ExecutionSpaceError::InvalidId(id.to_string()))
    }
}

impl ExecutionSpaceRegistry {
    pub fn load(firm_home: &Path) -> ExecutionSpaceResult<Self> {
        match std::fs::read_to_string(registry_path(firm_home)) {
            Ok(text) if text.trim().is_empty() => Ok(Self::default()),
            Ok(text) => Ok(serde_json::from_str(&text)?),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(error.into()),
        }
    }

    pub fn save(&mut self, firm_home: &Path) -> ExecutionSpaceResult<()> {
        self.format_version = REGISTRY_FORMAT_VERSION;
        std::fs::create_dir_all(spaces_dir(firm_home))?;
        std::fs::write(
            registry_path(firm_home),
            serde_json::to_string_pretty(self)?,
        )?;
        Ok(())
    }

    pub fn find(&self, id: &str) -> Option<&ExecutionSpaceRegistryEntry> {
        self.spaces.iter().find(|space| space.id == id)
    }

    pub fn upsert(&mut self, mut entry: ExecutionSpaceRegistryEntry, now: &str) {
        if let Some(existing) = self.spaces.iter_mut().find(|space| space.id == entry.id) {
            if !existing.created_at.is_empty() {
                entry.created_at = existing.created_at.clone();
            }
            entry.last_opened_at = now.to_string();
            *existing = entry;
        } else {
            if entry.created_at.is_empty() {
                entry.created_at = now.to_string();
            }
            entry.last_opened_at = now.to_string();
            self.spaces.push(entry);
        }
    }
}

pub fn context_for_id(
    firm_home: &Path,
    id: &str,
) -> ExecutionSpaceResult<Option<ExecutionSpace>> {
    validate_space_id(id)?;
    let registry = ExecutionSpaceRegistry::load(firm_home)?;
    if let Some(entry) = registry.find(id) {
        return Ok(Some(ExecutionSpace {
            id: entry.id.clone(),
            name: entry.name.clone(),
            store_root: entry.store_root.clone(),
            default_project_binding_id: entry.default_project_binding_id.clone(),
            company_id: entry.company_id.clone(),
        }));
    }
    let store_root = space_store_root(firm_home, id);
    Ok(read_metadata(&store_root)?.map(|metadata| ExecutionSpace {
        id: metadata.space_id,
        name: metadata.name,
        store_root,
        default_project_binding_id: metadata.default_project_binding_id,
        company_id: metadata.company_id,
    }))
}

pub fn register_and_activate(
    firm_home: &Path,
    id: &str,
    name: &str,
    default_project_binding_id: Option<String>,
    company_id: Option<String>,
    now: &str,
) -> ExecutionSpaceResult<ExecutionSpace> {
    validate_space_id(id)?;
    let context = ExecutionSpace {
        id: id.to_string(),
        name: if name.trim().is_empty() {
            id.to_string()
        } else {
            name.to_string()
        },
        store_root: space_store_root(firm_home, id),
        default_project_binding_id,
        company_id,
    };
    write_metadata(&context)?;
    let mut registry = ExecutionSpaceRegistry::load(firm_home)?;
    registry.upsert(
        ExecutionSpaceRegistryEntry {
            id: context.id.clone(),
            name: context.name.clone(),
            store_root: context.store_root.clone(),
            default_project_binding_id: context.default_project_binding_id.clone(),
            company_id: context.company_id.clone(),
            created_at: String::new(),
            last_opened_at: String::new(),
        },
        now,
    );
    registry.current_space_id = Some(context.id.clone());
    registry.save(firm_home)?;
    write_active_space(firm_home, &context.id)?;
    Ok(context)
}

pub fn switch_current_space(
    firm_home: &Path,
    id: &str,
    now: &str,
) -> ExecutionSpaceResult<ExecutionSpace> {
    let context = context_for_id(firm_home, id)?.ok_or_else(|| {
        ExecutionSpaceError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("unknown execution space id: {id}"),
        ))
    })?;
    let mut registry = ExecutionSpaceRegistry::load(firm_home)?;
    registry.upsert(
        ExecutionSpaceRegistryEntry {
            id: context.id.clone(),
            name: context.name.clone(),
            store_root: context.store_root.clone(),
            default_project_binding_id: context.default_project_binding_id.clone(),
            company_id: context.company_id.clone(),
            created_at: String::new(),
            last_opened_at: String::new(),
        },
        now,
    );
    registry.current_space_id = Some(context.id.clone());
    registry.save(firm_home)?;
    write_active_space(firm_home, &context.id)?;
    Ok(context)
}

pub fn active_space_id(firm_home: &Path) -> ExecutionSpaceResult<Option<String>> {
    let registry = ExecutionSpaceRegistry::load(firm_home)?;
    if registry.current_space_id.is_some() {
        return Ok(registry.current_space_id);
    }
    read_active_space(firm_home)
}

pub fn list_spaces(firm_home: &Path) -> ExecutionSpaceResult<Vec<ExecutionSpace>> {
    let registry = ExecutionSpaceRegistry::load(firm_home)?;
    let mut spaces = registry
        .spaces
        .iter()
        .map(|entry| ExecutionSpace {
            id: entry.id.clone(),
            name: entry.name.clone(),
            store_root: entry.store_root.clone(),
            default_project_binding_id: entry.default_project_binding_id.clone(),
            company_id: entry.company_id.clone(),
        })
        .collect::<Vec<_>>();
    spaces.sort_by(|left, right| left.id.cmp(&right.id));
    spaces.dedup_by(|left, right| left.id == right.id);
    Ok(spaces)
}

pub fn write_metadata(context: &ExecutionSpace) -> ExecutionSpaceResult<()> {
    std::fs::create_dir_all(&context.store_root)?;
    let metadata = ExecutionSpaceMetadata {
        space_id: context.id.clone(),
        name: context.name.clone(),
        default_project_binding_id: context.default_project_binding_id.clone(),
        company_id: context.company_id.clone(),
    };
    std::fs::write(
        context.store_root.join("metadata.json"),
        serde_json::to_string_pretty(&metadata)?,
    )?;
    Ok(())
}

pub fn read_metadata(store_root: &Path) -> ExecutionSpaceResult<Option<ExecutionSpaceMetadata>> {
    match std::fs::read_to_string(store_root.join("metadata.json")) {
        Ok(text) => Ok(Some(serde_json::from_str(&text)?)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

pub fn write_active_space(firm_home: &Path, id: &str) -> ExecutionSpaceResult<()> {
    std::fs::create_dir_all(firm_home)?;
    std::fs::write(active_space_path(firm_home), format!("{id}\n"))?;
    Ok(())
}

pub fn read_active_space(firm_home: &Path) -> ExecutionSpaceResult<Option<String>> {
    match std::fs::read_to_string(active_space_path(firm_home)) {
        Ok(text) => {
            let id = text.trim().to_string();
            Ok((!id.is_empty()).then_some(id))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

pub fn firm_home() -> ExecutionSpaceResult<PathBuf> {
    project::firm_home().map_err(|error| match error {
        project::ProjectError::NoHome => ExecutionSpaceError::NoHome,
        project::ProjectError::Io(error) => ExecutionSpaceError::Io(error),
        project::ProjectError::Json(error) => ExecutionSpaceError::Json(error),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_home(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "harness-space-{tag}-{}-{}",
            std::process::id(),
            crate::generated_id("test")
        ))
    }

    #[test]
    fn registry_keeps_execution_store_independent_from_project_binding() {
        let home = temp_home("registry");
        let context = register_and_activate(
            &home,
            "company-dev",
            "Company Development",
            Some("multi-agent-harness".into()),
            None,
            "unix-ms:1",
        )
        .expect("register");
        assert_eq!(
            context.store_root,
            home.join("execution-spaces/company-dev")
        );
        assert_eq!(
            context.default_project_binding_id.as_deref(),
            Some("multi-agent-harness")
        );
        assert_eq!(
            active_space_id(&home).expect("active").as_deref(),
            Some("company-dev")
        );
        let _ = std::fs::remove_dir_all(home);
    }
}
