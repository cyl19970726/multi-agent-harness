//! Company Store identity + registry layer (ADR 0040, Phase 2).
//!
//! This is deliberately separate from `project.rs`: a Company Store owns Company
//! OS truth, while a Project Binding owns repository/worktree/runtime selection.
//! The current implementation is additive. Existing project-derived stores keep
//! working until explicit Company Store selection is used.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::project;

const REGISTRY_FORMAT_VERSION: u32 = 1;

#[derive(Debug)]
pub enum CompanyStoreError {
    Io(std::io::Error),
    Json(serde_json::Error),
    InvalidId(String),
    NoHome,
}

impl std::fmt::Display for CompanyStoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CompanyStoreError::Io(e) => write!(f, "company store io error: {e}"),
            CompanyStoreError::Json(e) => write!(f, "company store json error: {e}"),
            CompanyStoreError::InvalidId(id) => write!(
                f,
                "invalid company id `{id}`; use letters, digits, '.', '_' or '-'"
            ),
            CompanyStoreError::NoHome => write!(f, "could not determine harness home"),
        }
    }
}

impl std::error::Error for CompanyStoreError {}

impl From<std::io::Error> for CompanyStoreError {
    fn from(e: std::io::Error) -> Self {
        CompanyStoreError::Io(e)
    }
}

impl From<serde_json::Error> for CompanyStoreError {
    fn from(e: serde_json::Error) -> Self {
        CompanyStoreError::Json(e)
    }
}

pub type CompanyStoreResult<T> = Result<T, CompanyStoreError>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompanyContext {
    pub id: String,
    pub name: String,
    pub store_root: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompanyRegistryEntry {
    pub id: String,
    pub name: String,
    pub store_root: PathBuf,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub last_opened_at: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CompanyRegistry {
    #[serde(default)]
    pub format_version: u32,
    #[serde(default)]
    pub current_company_id: Option<String>,
    #[serde(default)]
    pub companies: Vec<CompanyRegistryEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompanyMetadata {
    pub company_id: String,
    pub name: String,
}

pub fn companies_dir(harness_home: &Path) -> PathBuf {
    harness_home.join("companies")
}

pub fn company_store_root(harness_home: &Path, id: &str) -> PathBuf {
    companies_dir(harness_home).join(id)
}

pub fn registry_path(harness_home: &Path) -> PathBuf {
    companies_dir(harness_home).join("registry.json")
}

pub fn active_company_path(harness_home: &Path) -> PathBuf {
    harness_home.join("ACTIVE_COMPANY")
}

pub fn validate_company_id(id: &str) -> CompanyStoreResult<()> {
    if id.is_empty() || id == "." || id == ".." {
        return Err(CompanyStoreError::InvalidId(id.to_string()));
    }
    let valid = id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-');
    if valid {
        Ok(())
    } else {
        Err(CompanyStoreError::InvalidId(id.to_string()))
    }
}

impl CompanyRegistry {
    pub fn load(harness_home: &Path) -> CompanyStoreResult<Self> {
        let path = registry_path(harness_home);
        match std::fs::read_to_string(&path) {
            Ok(text) if text.trim().is_empty() => Ok(Self::default()),
            Ok(text) => Ok(serde_json::from_str(&text)?),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(CompanyStoreError::Io(e)),
        }
    }

    pub fn save(&mut self, harness_home: &Path) -> CompanyStoreResult<()> {
        self.format_version = REGISTRY_FORMAT_VERSION;
        std::fs::create_dir_all(companies_dir(harness_home))?;
        std::fs::write(
            registry_path(harness_home),
            serde_json::to_string_pretty(self)?,
        )?;
        Ok(())
    }

    pub fn find(&self, id: &str) -> Option<&CompanyRegistryEntry> {
        self.companies.iter().find(|c| c.id == id)
    }

    pub fn upsert(&mut self, mut entry: CompanyRegistryEntry, now: &str) {
        if let Some(existing) = self.companies.iter_mut().find(|c| c.id == entry.id) {
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
            self.companies.push(entry);
        }
    }
}

pub fn context_for_id(harness_home: &Path, id: &str) -> CompanyStoreResult<Option<CompanyContext>> {
    validate_company_id(id)?;
    let registry = CompanyRegistry::load(harness_home)?;
    if let Some(entry) = registry.find(id) {
        return Ok(Some(CompanyContext {
            id: entry.id.clone(),
            name: entry.name.clone(),
            store_root: entry.store_root.clone(),
        }));
    }
    let store_root = company_store_root(harness_home, id);
    match read_metadata(&store_root)? {
        Some(meta) => Ok(Some(CompanyContext {
            id: meta.company_id,
            name: meta.name,
            store_root,
        })),
        None => Ok(None),
    }
}

pub fn register_and_activate(
    harness_home: &Path,
    id: &str,
    name: &str,
    now: &str,
) -> CompanyStoreResult<CompanyContext> {
    validate_company_id(id)?;
    let name = if name.trim().is_empty() { id } else { name };
    let ctx = CompanyContext {
        id: id.to_string(),
        name: name.to_string(),
        store_root: company_store_root(harness_home, id),
    };
    write_metadata(&ctx)?;
    let mut registry = CompanyRegistry::load(harness_home)?;
    registry.upsert(
        CompanyRegistryEntry {
            id: ctx.id.clone(),
            name: ctx.name.clone(),
            store_root: ctx.store_root.clone(),
            created_at: String::new(),
            last_opened_at: String::new(),
        },
        now,
    );
    registry.current_company_id = Some(ctx.id.clone());
    registry.save(harness_home)?;
    write_active_company(harness_home, &ctx.id)?;
    Ok(ctx)
}

pub fn switch_current_company(
    harness_home: &Path,
    id: &str,
    now: &str,
) -> CompanyStoreResult<CompanyContext> {
    let ctx = context_for_id(harness_home, id)?.ok_or_else(|| {
        CompanyStoreError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("unknown company id: {id}"),
        ))
    })?;
    let mut registry = CompanyRegistry::load(harness_home)?;
    registry.upsert(
        CompanyRegistryEntry {
            id: ctx.id.clone(),
            name: ctx.name.clone(),
            store_root: ctx.store_root.clone(),
            created_at: String::new(),
            last_opened_at: String::new(),
        },
        now,
    );
    registry.current_company_id = Some(ctx.id.clone());
    registry.save(harness_home)?;
    write_active_company(harness_home, &ctx.id)?;
    Ok(ctx)
}

pub fn active_company_id(harness_home: &Path) -> CompanyStoreResult<Option<String>> {
    let registry = CompanyRegistry::load(harness_home)?;
    if let Some(id) = registry.current_company_id {
        return Ok(Some(id));
    }
    read_active_company(harness_home)
}

pub fn list_companies(harness_home: &Path) -> CompanyStoreResult<Vec<CompanyContext>> {
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let registry = CompanyRegistry::load(harness_home)?;
    for entry in &registry.companies {
        if seen.insert(entry.id.clone()) {
            out.push(CompanyContext {
                id: entry.id.clone(),
                name: entry.name.clone(),
                store_root: entry.store_root.clone(),
            });
        }
    }

    let companies = companies_dir(harness_home);
    if let Ok(read_dir) = std::fs::read_dir(&companies) {
        for dir_entry in read_dir.flatten() {
            if !dir_entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                continue;
            }
            let id = match dir_entry.file_name().into_string() {
                Ok(name) => name,
                Err(_) => continue,
            };
            if seen.contains(&id) {
                continue;
            }
            let store_root = dir_entry.path();
            if let Ok(Some(meta)) = read_metadata(&store_root) {
                seen.insert(id.clone());
                out.push(CompanyContext {
                    id: meta.company_id,
                    name: meta.name,
                    store_root,
                });
            }
        }
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(out)
}

pub fn write_metadata(ctx: &CompanyContext) -> CompanyStoreResult<()> {
    std::fs::create_dir_all(&ctx.store_root)?;
    let metadata = CompanyMetadata {
        company_id: ctx.id.clone(),
        name: ctx.name.clone(),
    };
    std::fs::write(
        ctx.store_root.join("metadata.json"),
        serde_json::to_string_pretty(&metadata)?,
    )?;
    Ok(())
}

pub fn read_metadata(store_root: &Path) -> CompanyStoreResult<Option<CompanyMetadata>> {
    let path = store_root.join("metadata.json");
    match std::fs::read_to_string(&path) {
        Ok(text) => Ok(Some(serde_json::from_str(&text)?)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(CompanyStoreError::Io(e)),
    }
}

pub fn write_active_company(harness_home: &Path, id: &str) -> CompanyStoreResult<()> {
    std::fs::create_dir_all(harness_home)?;
    std::fs::write(active_company_path(harness_home), format!("{id}\n"))?;
    Ok(())
}

pub fn read_active_company(harness_home: &Path) -> CompanyStoreResult<Option<String>> {
    match std::fs::read_to_string(active_company_path(harness_home)) {
        Ok(text) => {
            let id = text.trim().to_string();
            Ok(if id.is_empty() { None } else { Some(id) })
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(CompanyStoreError::Io(e)),
    }
}

pub fn harness_home() -> CompanyStoreResult<PathBuf> {
    project::harness_home().map_err(|e| match e {
        project::ProjectError::NoHome => CompanyStoreError::NoHome,
        project::ProjectError::Io(io) => CompanyStoreError::Io(io),
        project::ProjectError::Json(json) => CompanyStoreError::Json(json),
    })
}
