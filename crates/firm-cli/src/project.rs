//! Project Binding identity + registry layer.
//!
//! This module turns the pure path→id functions in `firm-core`
//! (`project_id_for_path`, `project_store_root`, `GLOBAL_PROJECT_ID`,
//! `ProjectContext`) into a persistent, on-disk control plane:
//!
//! - a **registry** at `<firm_home>/projects/registry.json` tracking every
//!   known Project Binding + the single default `current_project_id`,
//! - an `ACTIVE_PROJECT` marker file (mirror of `current_project_id`) read by
//!   `serve`/CLI-spawned workers without parsing the whole registry,
//! - compatibility `metadata.json` that pins path, kind, and Git identity.
//!
//! Project selection controls provider cwd/config/Skill boundaries. Native
//! coordination storage is selected independently by Execution Space.
//! `firm_home()` honors `FIRM_HOME` so tests never touch the developer's
//! real `~/.firm`.

use std::path::{Path, PathBuf};

use harness_core::{
    project_id_for_path, project_store_root, ProjectBinding, ProjectContext, ProjectKind,
    GLOBAL_PROJECT_ID,
};
use serde::{Deserialize, Serialize};

/// Current on-disk schema version for the registry file.
const REGISTRY_FORMAT_VERSION: u32 = 1;

/// Errors from the project identity / registry layer. Kept local and converted to
/// `CliError::Io`/`CliError::Json` at the `main.rs` boundary.
#[derive(Debug)]
pub enum ProjectError {
    Io(std::io::Error),
    Json(serde_json::Error),
    NoHome,
}

impl std::fmt::Display for ProjectError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProjectError::Io(e) => write!(f, "project io error: {e}"),
            ProjectError::Json(e) => write!(f, "project json error: {e}"),
            ProjectError::NoHome => write!(f, "could not determine home directory"),
        }
    }
}

impl std::error::Error for ProjectError {}

impl From<std::io::Error> for ProjectError {
    fn from(e: std::io::Error) -> Self {
        ProjectError::Io(e)
    }
}

impl From<serde_json::Error> for ProjectError {
    fn from(e: serde_json::Error) -> Self {
        ProjectError::Json(e)
    }
}

pub type ProjectResult<T> = Result<T, ProjectError>;

/// Resolve the user's HOME directory, canonicalized when possible so the slug rule
/// matches `project_id_for_path` (which expects canonical paths). Honors `HOME`
/// (always set on unix/macOS in tests too).
pub fn home_dir() -> ProjectResult<PathBuf> {
    let raw = std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or(ProjectError::NoHome)?;
    Ok(canonicalize_best_effort(&raw))
}

/// Resolve the firm HOME (`~/.firm` by default).
///
/// Honors the `FIRM_HOME` env var as a **test hook** so the registry layer can
/// be exercised against a temp dir without polluting the developer's real
/// `~/.firm` (per the spec's "Test isolation" risk). This is independent of
/// `FIRM_ROOT`, which overrides a single *store* root, not the home.
///
/// Backward compatibility: if `~/.firm` does not exist but the legacy
/// `~/.harness` does, the legacy path is returned (with a deprecation warning
/// on stderr). `HARNESS_HOME` is also checked as a fallback for `FIRM_HOME`.
pub fn firm_home() -> ProjectResult<PathBuf> {
    if let Some(home) = std::env::var_os("FIRM_HOME") {
        if !home.is_empty() {
            return Ok(PathBuf::from(home));
        }
    }
    // Backward compat: HARNESS_HOME as fallback
    if let Some(home) = std::env::var_os("HARNESS_HOME") {
        if !home.is_empty() {
            eprintln!("warning: HARNESS_HOME is deprecated; use FIRM_HOME instead");
            return Ok(PathBuf::from(home));
        }
    }
    let new_home = home_dir()?.join(".firm");
    let old_home = home_dir()?.join(".harness");
    if !new_home.exists() && old_home.exists() {
        eprintln!("warning: using legacy ~/.harness store; migrate to ~/.firm with `firm project migrate`");
        return Ok(old_home);
    }
    Ok(new_home)
}

/// `<firm_home>/projects` — Project Binding registry plus legacy
/// project-derived compatibility stores.
pub fn projects_dir(firm_home: &Path) -> PathBuf {
    firm_home.join("projects")
}

/// `<firm_home>/projects/registry.json`.
pub fn registry_path(firm_home: &Path) -> PathBuf {
    projects_dir(firm_home).join("registry.json")
}

/// `<firm_home>/ACTIVE_PROJECT` — single-line marker mirroring the registry's
/// `current_project_id`, read by `serve`/workers without parsing the registry.
pub fn active_project_path(firm_home: &Path) -> PathBuf {
    firm_home.join("ACTIVE_PROJECT")
}

/// Canonicalize a path, falling back to a lexical absolutization when the path
/// does not yet exist (e.g. a project we are about to `init`). This keeps id
/// derivation stable whether or not the dir is materialized.
pub fn canonicalize_best_effort(path: &Path) -> PathBuf {
    if let Ok(canon) = std::fs::canonicalize(path) {
        return canon;
    }
    if path.is_absolute() {
        return path.to_path_buf();
    }
    match std::env::current_dir() {
        Ok(cwd) => cwd.join(path),
        Err(_) => path.to_path_buf(),
    }
}

/// One registered project. `path` is the canonical project root; `store_root` is
/// the centralized store under `<firm_home>/projects/<id>/`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegistryEntry {
    pub id: String,
    pub path: PathBuf,
    pub store_root: PathBuf,
    pub kind: ProjectKind,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub last_opened_at: String,
}

/// The persisted registry: a schema version, the single current project id, and
/// the known projects. Every field is defaulted so an older/forward file loads.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProjectRegistry {
    #[serde(default)]
    pub format_version: u32,
    #[serde(default)]
    pub current_project_id: Option<String>,
    #[serde(default)]
    pub projects: Vec<RegistryEntry>,
}

/// Per-store identity, written as `<store_root>/metadata.json` so a project's
/// identity survives a fresh clone of the *registry* (or a re-pin after a move).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectMetadata {
    pub project_id: String,
    pub canonical_path: PathBuf,
    pub kind: ProjectKind,
    #[serde(default)]
    pub is_git_repo: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git_remote: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub migrated_from: Option<PathBuf>,
}

impl ProjectRegistry {
    /// Load the registry from `<firm_home>/projects/registry.json`. A missing
    /// file yields an empty registry (first run); a corrupt file is a hard error
    /// rather than silently discarding known projects.
    pub fn load(firm_home: &Path) -> ProjectResult<Self> {
        let path = registry_path(firm_home);
        match std::fs::read_to_string(&path) {
            Ok(text) if text.trim().is_empty() => Ok(Self::default()),
            Ok(text) => Ok(serde_json::from_str(&text)?),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(ProjectError::Io(e)),
        }
    }

    /// Persist the registry, creating `<firm_home>/projects/` as needed. The
    /// `format_version` is stamped on save.
    pub fn save(&mut self, firm_home: &Path) -> ProjectResult<()> {
        self.format_version = REGISTRY_FORMAT_VERSION;
        std::fs::create_dir_all(projects_dir(firm_home))?;
        let text = serde_json::to_string_pretty(self)?;
        std::fs::write(registry_path(firm_home), text)?;
        Ok(())
    }

    /// Find a registered project by id.
    pub fn find(&self, id: &str) -> Option<&RegistryEntry> {
        self.projects.iter().find(|p| p.id == id)
    }

    /// Find a registered project by its (canonical) path.
    pub fn find_by_path(&self, path: &Path) -> Option<&RegistryEntry> {
        self.projects.iter().find(|p| p.path == path)
    }

    /// Insert or update an entry (keyed by id), preserving `created_at` on update
    /// and refreshing `last_opened_at`.
    pub fn upsert(&mut self, mut entry: RegistryEntry, now: &str) {
        if let Some(existing) = self.projects.iter_mut().find(|p| p.id == entry.id) {
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
            self.projects.push(entry);
        }
    }
}

/// Build a [`ProjectContext`] for a project root, deriving the id from the
/// canonical path relative to HOME. Does NOT touch the filesystem registry — it
/// only computes identity plus the compatibility-store locator.
pub fn context_for_root(project_root: &Path, firm_home: &Path) -> ProjectResult<ProjectContext> {
    let home = home_dir()?;
    let canonical = canonicalize_best_effort(project_root);
    let id = project_id_for_path(&canonical, &home);
    let kind = if id == GLOBAL_PROJECT_ID {
        ProjectKind::Global
    } else {
        ProjectKind::Repo
    };
    let store_root = project_store_root(firm_home, &id);
    let is_git_repo = path_is_git_repo(&canonical);
    Ok(ProjectContext {
        id,
        project_root: canonical,
        store_root,
        kind,
        is_git_repo,
    })
}

fn git_output(root: &Path, args: &[&str]) -> Option<String> {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let value = String::from_utf8(output.stdout).ok()?.trim().to_string();
    (!value.is_empty()).then_some(value)
}

/// Resolve a native Project Binding for a local directory.
///
/// This deliberately contains no execution store root. The existing
/// [`ProjectContext`] remains a compatibility adapter for legacy stores.
pub fn binding_for_root(project_root: &Path, firm_home: &Path) -> ProjectResult<ProjectBinding> {
    let context = context_for_root(project_root, firm_home)?;
    let canonical_root = context.project_root.clone();
    let git_common_dir = git_output(&canonical_root, &["rev-parse", "--git-common-dir"])
        .map(PathBuf::from)
        .map(|path| {
            if path.is_absolute() {
                canonicalize_best_effort(&path)
            } else {
                canonicalize_best_effort(&canonical_root.join(path))
            }
        });
    Ok(ProjectBinding {
        id: context.id,
        project_root: canonical_root.clone(),
        kind: context.kind,
        is_git_repo: context.is_git_repo,
        repository_url: git_output(&canonical_root, &["remote", "get-url", "origin"]),
        default_branch: git_output(
            &canonical_root,
            &["symbolic-ref", "--short", "refs/remotes/origin/HEAD"],
        )
        .and_then(|value| value.rsplit('/').next().map(ToString::to_string)),
        git_common_dir,
        instruction_boundary: canonical_root.clone(),
        skill_discovery_boundary: canonical_root,
        worktree_policy: if context.is_git_repo {
            "same_git_common_dir".to_string()
        } else {
            "within_project_root".to_string()
        },
        permission_policy: "project_workspace".to_string(),
    })
}

pub fn binding_for_id(firm_home: &Path, id: &str) -> ProjectResult<Option<ProjectBinding>> {
    context_for_id(firm_home, id)?
        .map(|context| binding_for_root(&context.project_root, firm_home))
        .transpose()
}

/// Build a [`ProjectContext`] for the reserved GLOBAL project (`$HOME`).
pub fn global_context(firm_home: &Path) -> ProjectResult<ProjectContext> {
    let home = home_dir()?;
    context_for_root(&home, firm_home)
}

/// Cheap git-ness check used at context-build time: a `.git` entry exists at the
/// path. This avoids spawning `git` for every resolution; the spawn-time
/// [`is_git_repo`](crate) gate in `main.rs` remains the authority for worktrees.
fn path_is_git_repo(path: &Path) -> bool {
    path.join(".git").exists()
}

/// Materialize a project's store + metadata and register it, marking it current.
///
/// Idempotent: re-running on the same root refreshes `last_opened_at` and rewrites
/// `metadata.json` without losing `created_at`.
pub fn register_and_activate(
    firm_home: &Path,
    project_root: &Path,
    now: &str,
) -> ProjectResult<ProjectContext> {
    let ctx = context_for_root(project_root, firm_home)?;
    write_metadata(&ctx, None)?;
    let mut registry = ProjectRegistry::load(firm_home)?;
    registry.upsert(
        RegistryEntry {
            id: ctx.id.clone(),
            path: ctx.project_root.clone(),
            store_root: ctx.store_root.clone(),
            kind: ctx.kind,
            created_at: String::new(),
            last_opened_at: String::new(),
        },
        now,
    );
    registry.current_project_id = Some(ctx.id.clone());
    registry.save(firm_home)?;
    write_active_project(firm_home, &ctx.id)?;
    Ok(ctx)
}

/// Switch the active project to `id`, updating BOTH the registry's
/// `current_project_id` and the `ACTIVE_PROJECT` marker so the next CLI invocation
/// (run from any cwd) and a live `serve` converge on the same central store
/// (goal-multi-project #89 invariant, serve-project-switch-convergence task).
///
/// The id must already resolve to a [`ProjectContext`] (registered, on-disk, or the
/// reserved `_global`); switching to an unknown id is rejected so we never strand
/// the active pointer on a non-existent store. `last_opened_at` is refreshed when
/// the project is in the registry. Returns the now-current context.
pub fn switch_current_project(
    firm_home: &Path,
    id: &str,
    now: &str,
) -> ProjectResult<ProjectContext> {
    let ctx = context_for_id(firm_home, id)?.ok_or_else(|| {
        ProjectError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("unknown project id: {id}"),
        ))
    })?;
    let mut registry = ProjectRegistry::load(firm_home)?;
    // Keep the registry self-consistent: ensure the project is known so a later
    // `list`/`current` reflects it, refreshing `last_opened_at`.
    registry.upsert(
        RegistryEntry {
            id: ctx.id.clone(),
            path: ctx.project_root.clone(),
            store_root: ctx.store_root.clone(),
            kind: ctx.kind,
            created_at: String::new(),
            last_opened_at: String::new(),
        },
        now,
    );
    registry.current_project_id = Some(ctx.id.clone());
    registry.save(firm_home)?;
    write_active_project(firm_home, &ctx.id)?;
    Ok(ctx)
}

/// Outcome of [`remove_project`]: whether the registry actually changed and whether
/// the removed project was the active one (so the caller can warn / re-point).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoveOutcome {
    pub removed: bool,
    pub was_current: bool,
}

/// Unregister a project from the registry by id (the centralized STORE on disk is
/// left untouched — removal is a registry-pointer operation, not a destructive
/// delete, so history is never silently lost).
///
/// Refuses to remove the reserved `_global` project (it is always derivable and
/// serves as the default fallback). If the removed project was current, the
/// `current_project_id` + `ACTIVE_PROJECT` marker are cleared so resolution falls
/// back to the legacy walk-up / `_global` rather than stranding on a now-unknown id.
pub fn remove_project(firm_home: &Path, id: &str) -> ProjectResult<RemoveOutcome> {
    if id == GLOBAL_PROJECT_ID {
        return Err(ProjectError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "the reserved `_global` project cannot be removed".to_string(),
        )));
    }
    let mut registry = ProjectRegistry::load(firm_home)?;
    let before = registry.projects.len();
    registry.projects.retain(|p| p.id != id);
    let removed = registry.projects.len() != before;
    let was_current = registry.current_project_id.as_deref() == Some(id);
    if was_current {
        registry.current_project_id = None;
    }
    registry.save(firm_home)?;
    if was_current {
        clear_active_project(firm_home)?;
    }
    Ok(RemoveOutcome {
        removed,
        was_current,
    })
}

/// Clear the `ACTIVE_PROJECT` marker (delete it). Used when the current project is
/// removed, so the next resolution falls through to the legacy walk-up / `_global`.
pub fn clear_active_project(firm_home: &Path) -> ProjectResult<()> {
    match std::fs::remove_file(active_project_path(firm_home)) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(ProjectError::Io(e)),
    }
}

/// Write `<store_root>/metadata.json` pinning the project's identity. Creates the
/// store dir if it does not exist yet.
pub fn write_metadata(ctx: &ProjectContext, migrated_from: Option<PathBuf>) -> ProjectResult<()> {
    std::fs::create_dir_all(&ctx.store_root)?;
    let metadata = ProjectMetadata {
        project_id: ctx.id.clone(),
        canonical_path: ctx.project_root.clone(),
        kind: ctx.kind,
        is_git_repo: ctx.is_git_repo,
        git_remote: None,
        migrated_from,
    };
    let text = serde_json::to_string_pretty(&metadata)?;
    std::fs::write(ctx.store_root.join("metadata.json"), text)?;
    Ok(())
}

/// Read `<store_root>/metadata.json` if present.
pub fn read_metadata(store_root: &Path) -> ProjectResult<Option<ProjectMetadata>> {
    let path = store_root.join("metadata.json");
    match std::fs::read_to_string(&path) {
        Ok(text) => Ok(Some(serde_json::from_str(&text)?)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(ProjectError::Io(e)),
    }
}

/// Filename of the marker dropped in an OLD repo-local `.harness/` store once it
/// has been migrated to the centralized store. Its content is the absolute central
/// `store_root` path. Resolution ignores any local store carrying this marker
/// (goal-multi-project P7 / dual-read-logging), so a migrated repo never serves
/// stale rows.
pub const MIGRATED_MARKER_FILE: &str = "MIGRATED_TO_CENTRAL";

/// Write the `MIGRATED_TO_CENTRAL` marker into an old local `.harness/` store,
/// recording the absolute central store root it was migrated into.
pub fn write_migrated_marker(local_store: &Path, central_store: &Path) -> ProjectResult<()> {
    std::fs::create_dir_all(local_store)?;
    std::fs::write(
        local_store.join(MIGRATED_MARKER_FILE),
        format!("{}\n", central_store.display()),
    )?;
    Ok(())
}

/// Read the `MIGRATED_TO_CENTRAL` marker from a local store, if present. Returns the
/// central store path the local store points to (trimmed). `None` means the store is
/// not marked migrated.
pub fn read_migrated_marker(local_store: &Path) -> ProjectResult<Option<PathBuf>> {
    match std::fs::read_to_string(local_store.join(MIGRATED_MARKER_FILE)) {
        Ok(text) => {
            let trimmed = text.trim();
            Ok(if trimmed.is_empty() {
                // Marked but pointer-less: still "migrated", just no target known.
                Some(PathBuf::new())
            } else {
                Some(PathBuf::from(trimmed))
            })
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(ProjectError::Io(e)),
    }
}

/// Write the `ACTIVE_PROJECT` marker (mirror of `current_project_id`).
pub fn write_active_project(firm_home: &Path, id: &str) -> ProjectResult<()> {
    std::fs::create_dir_all(firm_home)?;
    std::fs::write(active_project_path(firm_home), format!("{id}\n"))?;
    Ok(())
}

/// Read the `ACTIVE_PROJECT` marker, if present (trimmed).
pub fn read_active_project(firm_home: &Path) -> ProjectResult<Option<String>> {
    match std::fs::read_to_string(active_project_path(firm_home)) {
        Ok(text) => {
            let id = text.trim().to_string();
            Ok(if id.is_empty() { None } else { Some(id) })
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(ProjectError::Io(e)),
    }
}

/// Resolve the currently-active project id: the registry's `current_project_id`
/// first, then the `ACTIVE_PROJECT` marker as a fallback. Returns `None` if no
/// project has been selected yet.
pub fn active_project_id(firm_home: &Path) -> ProjectResult<Option<String>> {
    let registry = ProjectRegistry::load(firm_home)?;
    if let Some(id) = registry.current_project_id {
        return Ok(Some(id));
    }
    read_active_project(firm_home)
}

/// Enumerate every known project as a [`ProjectContext`], for the serve
/// `GET /v1/projects` endpoint (goal-multi-project P6 / project-api task).
///
/// Sources, merged and de-duplicated by id (first writer wins):
/// 1. registry entries (the authoritative pinned Project Binding path),
/// 2. compatibility metadata not already in the registry,
/// 3. the reserved `_global` project, always present even if never explicitly
///    registered.
///
/// A missing `projects/` dir yields just `_global`. Corrupt compatibility
/// metadata is skipped rather than hiding every Project Binding from the picker.
pub fn list_projects(firm_home: &Path) -> ProjectResult<Vec<ProjectContext>> {
    let mut out: Vec<ProjectContext> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();

    // 1. Registry entries (authoritative).
    let registry = ProjectRegistry::load(firm_home)?;
    for entry in &registry.projects {
        if seen.insert(entry.id.clone()) {
            out.push(ProjectContext {
                id: entry.id.clone(),
                project_root: entry.path.clone(),
                store_root: entry.store_root.clone(),
                kind: entry.kind,
                is_git_repo: path_is_git_repo(&entry.path),
            });
        }
    }

    // 2. On-disk stores with metadata.json that the registry does not know about.
    let projects = projects_dir(firm_home);
    if let Ok(read_dir) = std::fs::read_dir(&projects) {
        for dir_entry in read_dir.flatten() {
            if !dir_entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                continue;
            }
            let store_root = dir_entry.path();
            let id = match dir_entry.file_name().into_string() {
                Ok(name) => name,
                Err(_) => continue,
            };
            if seen.contains(&id) {
                continue;
            }
            if let Ok(Some(meta)) = read_metadata(&store_root) {
                seen.insert(id.clone());
                out.push(ProjectContext {
                    id: meta.project_id,
                    project_root: meta.canonical_path.clone(),
                    store_root,
                    kind: meta.kind,
                    is_git_repo: path_is_git_repo(&meta.canonical_path),
                });
            }
        }
    }

    // 3. The reserved global project is always available.
    if !seen.contains(GLOBAL_PROJECT_ID) {
        out.push(global_context(firm_home)?);
    }

    Ok(out)
}

/// Resolve a [`ProjectContext`] for an id, preferring a registered entry (which
/// pins `path`/`store_root`) and falling back to `metadata.json` in the store, and
/// finally — for the reserved `_global` id — to a freshly computed global context.
pub fn context_for_id(firm_home: &Path, id: &str) -> ProjectResult<Option<ProjectContext>> {
    let registry = ProjectRegistry::load(firm_home)?;
    if let Some(entry) = registry.find(id) {
        let is_git_repo = path_is_git_repo(&entry.path);
        return Ok(Some(ProjectContext {
            id: entry.id.clone(),
            project_root: entry.path.clone(),
            store_root: entry.store_root.clone(),
            kind: entry.kind,
            is_git_repo,
        }));
    }
    // Not registered: try metadata.json under the derived store root.
    let store_root = project_store_root(firm_home, id);
    if let Some(meta) = read_metadata(&store_root)? {
        return Ok(Some(ProjectContext {
            id: meta.project_id,
            project_root: meta.canonical_path.clone(),
            store_root,
            kind: meta.kind,
            is_git_repo: path_is_git_repo(&meta.canonical_path),
        }));
    }
    // The reserved global project is always derivable on demand.
    if id == GLOBAL_PROJECT_ID {
        return Ok(Some(global_context(firm_home)?));
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, MutexGuard, OnceLock};

    /// Process-global lock serializing every test that mutates the shared `HOME` /
    /// `FIRM_HOME` env vars, so parallel test threads don't clobber each other.
    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    /// A temp harness home that also pins `HOME` so `project_id_for_path` derives
    /// stable slugs against a known root. Holds the env lock for the whole test so
    /// env mutation is exclusive across parallel threads.
    struct EnvGuard {
        _home: tempdir::TempDir,
        prev_home: Option<std::ffi::OsString>,
        prev_firm_home: Option<std::ffi::OsString>,
        // Dropped LAST (declared last) so env is restored before the lock releases.
        _lock: MutexGuard<'static, ()>,
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            restore("HOME", &self.prev_home);
            restore("FIRM_HOME", &self.prev_firm_home);
        }
    }

    fn restore(key: &str, prev: &Option<std::ffi::OsString>) {
        match prev {
            Some(v) => std::env::set_var(key, v),
            None => std::env::remove_var(key),
        }
    }

    // Minimal inline tempdir (no extra dependency): a uniquely-named dir under the
    // system temp that is removed on drop.
    mod tempdir {
        use std::path::{Path, PathBuf};
        use std::sync::atomic::{AtomicU64, Ordering};

        static COUNTER: AtomicU64 = AtomicU64::new(0);

        pub struct TempDir {
            path: PathBuf,
        }

        impl TempDir {
            pub fn new(tag: &str) -> std::io::Result<Self> {
                let n = COUNTER.fetch_add(1, Ordering::SeqCst);
                let pid = std::process::id();
                let nanos = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_nanos())
                    .unwrap_or(0);
                let path =
                    std::env::temp_dir().join(format!("harness-proj-test-{tag}-{pid}-{nanos}-{n}"));
                std::fs::create_dir_all(&path)?;
                Ok(Self { path })
            }

            pub fn path(&self) -> &Path {
                &self.path
            }
        }

        impl Drop for TempDir {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.path);
            }
        }
    }

    /// Set up an isolated HOME + FIRM_HOME and return (guard, home, firm_home).
    /// Acquires the process-global env lock first so parallel tests don't race.
    fn isolated() -> (EnvGuard, PathBuf, PathBuf) {
        let lock = env_lock().lock().unwrap_or_else(|e| e.into_inner());
        let home = tempdir::TempDir::new("home").expect("temp home");
        // Canonicalize so derived ids match `home_dir()` (which canonicalizes).
        let home_path = std::fs::canonicalize(home.path()).expect("canon home");
        let prev_home = std::env::var_os("HOME");
        let prev_firm_home = std::env::var_os("FIRM_HOME");
        std::env::set_var("HOME", &home_path);
        let firm_home = home_path.join(".firm");
        std::env::set_var("FIRM_HOME", &firm_home);
        (
            EnvGuard {
                _home: home,
                prev_home,
                prev_firm_home,
                _lock: lock,
            },
            home_path,
            firm_home,
        )
    }

    #[test]
    fn global_id_for_home() {
        let (_g, home, hh) = isolated();
        let ctx = context_for_root(&home, &hh).expect("ctx");
        assert_eq!(ctx.id, GLOBAL_PROJECT_ID);
        assert_eq!(ctx.kind, ProjectKind::Global);
        assert_eq!(ctx.store_root, hh.join("projects").join(GLOBAL_PROJECT_ID));
    }

    #[test]
    fn home_relative_slug() {
        let (_g, home, hh) = isolated();
        let root = home.join("ai-luodi").join("jyx3d");
        std::fs::create_dir_all(&root).unwrap();
        let ctx = context_for_root(&root, &hh).expect("ctx");
        assert_eq!(ctx.id, "ai-luodi-jyx3d");
        assert_eq!(ctx.kind, ProjectKind::Repo);
        assert_eq!(ctx.store_root, hh.join("projects").join("ai-luodi-jyx3d"));
    }

    #[test]
    fn external_path_is_content_hash() {
        let (_g, _home, hh) = isolated();
        // A path that is NOT under HOME → proj-<hash>. Use the system temp root,
        // which is outside the test HOME.
        let outside = std::env::temp_dir().join("harness-outside-xyz");
        std::fs::create_dir_all(&outside).unwrap();
        let ctx = context_for_root(&outside, &hh).expect("ctx");
        assert!(ctx.id.starts_with("proj-"), "got {}", ctx.id);
        // Stable across calls.
        let ctx2 = context_for_root(&outside, &hh).expect("ctx2");
        assert_eq!(ctx.id, ctx2.id);
        let _ = std::fs::remove_dir_all(&outside);
    }

    #[cfg(unix)]
    #[test]
    fn symlink_canonicalizes_to_same_id() {
        let (_g, home, hh) = isolated();
        let real = home.join("real-proj");
        std::fs::create_dir_all(&real).unwrap();
        let link = home.join("link-proj");
        let _ = std::fs::remove_file(&link);
        std::os::unix::fs::symlink(&real, &link).unwrap();
        let via_real = context_for_root(&real, &hh).expect("real");
        let via_link = context_for_root(&link, &hh).expect("link");
        assert_eq!(via_real.id, via_link.id);
        assert_eq!(via_real.id, "real-proj");
    }

    #[test]
    fn registry_round_trip() {
        let (_g, home, hh) = isolated();
        let root = home.join("proj-a");
        std::fs::create_dir_all(&root).unwrap();
        let now = "2026-06-17T00:00:00Z";
        let ctx = register_and_activate(&hh, &root, now).expect("register");

        let loaded = ProjectRegistry::load(&hh).expect("load");
        assert_eq!(loaded.current_project_id.as_deref(), Some(ctx.id.as_str()));
        let entry = loaded.find(&ctx.id).expect("entry");
        assert_eq!(entry.path, ctx.project_root);
        assert_eq!(entry.store_root, ctx.store_root);
        assert_eq!(entry.created_at, now);
        assert_eq!(entry.last_opened_at, now);

        // metadata.json pins identity in the store.
        let meta = read_metadata(&ctx.store_root).expect("meta").expect("some");
        assert_eq!(meta.project_id, ctx.id);
        assert_eq!(meta.canonical_path, ctx.project_root);

        // ACTIVE_PROJECT marker mirrors current.
        assert_eq!(
            read_active_project(&hh).expect("active").as_deref(),
            Some(ctx.id.as_str())
        );
        assert_eq!(
            active_project_id(&hh).expect("aid").as_deref(),
            Some(ctx.id.as_str())
        );
    }

    #[test]
    fn upsert_preserves_created_at_and_refreshes_last_opened() {
        let (_g, home, hh) = isolated();
        let root = home.join("proj-b");
        std::fs::create_dir_all(&root).unwrap();
        register_and_activate(&hh, &root, "2026-01-01T00:00:00Z").expect("first");
        let ctx = register_and_activate(&hh, &root, "2026-02-02T00:00:00Z").expect("second");
        let loaded = ProjectRegistry::load(&hh).expect("load");
        let entry = loaded.find(&ctx.id).expect("entry");
        assert_eq!(entry.created_at, "2026-01-01T00:00:00Z");
        assert_eq!(entry.last_opened_at, "2026-02-02T00:00:00Z");
        assert_eq!(loaded.projects.len(), 1);
    }

    #[test]
    fn context_for_id_from_registry_and_global_on_demand() {
        let (_g, home, hh) = isolated();
        let root = home.join("proj-c");
        std::fs::create_dir_all(&root).unwrap();
        let ctx = register_and_activate(&hh, &root, "now").expect("reg");
        let by_id = context_for_id(&hh, &ctx.id).expect("ok").expect("some");
        assert_eq!(by_id.project_root, ctx.project_root);

        // _global resolves even when never explicitly registered.
        let g = context_for_id(&hh, GLOBAL_PROJECT_ID)
            .expect("ok")
            .expect("some");
        assert_eq!(g.id, GLOBAL_PROJECT_ID);
        assert_eq!(g.kind, ProjectKind::Global);

        // Unknown id → None.
        assert!(context_for_id(&hh, "nope-does-not-exist")
            .expect("ok")
            .is_none());
    }

    #[test]
    fn missing_registry_loads_empty() {
        let (_g, _home, hh) = isolated();
        let reg = ProjectRegistry::load(&hh).expect("load");
        assert!(reg.current_project_id.is_none());
        assert!(reg.projects.is_empty());
    }

    #[test]
    fn list_projects_includes_registry_entries_and_global() {
        let (_g, home, hh) = isolated();
        // Empty registry on first run → just the reserved global.
        let only_global = list_projects(&hh).expect("list");
        assert_eq!(only_global.len(), 1);
        assert_eq!(only_global[0].id, GLOBAL_PROJECT_ID);

        // Register two projects; both plus _global are enumerated.
        let a = home.join("proj-a");
        let b = home.join("proj-b");
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(&b).unwrap();
        let ctx_a = register_and_activate(&hh, &a, "now").expect("a");
        let ctx_b = register_and_activate(&hh, &b, "now").expect("b");

        let ids: std::collections::HashSet<String> = list_projects(&hh)
            .expect("list")
            .into_iter()
            .map(|c| c.id)
            .collect();
        assert!(ids.contains(&ctx_a.id), "missing {} in {ids:?}", ctx_a.id);
        assert!(ids.contains(&ctx_b.id), "missing {} in {ids:?}", ctx_b.id);
        assert!(
            ids.contains(GLOBAL_PROJECT_ID),
            "missing _global in {ids:?}"
        );
    }

    #[test]
    fn list_projects_picks_up_on_disk_store_without_registry_entry() {
        let (_g, home, hh) = isolated();
        // Materialize a store + metadata WITHOUT a registry entry (e.g. a store
        // copied in by `migrate` before a registry write landed).
        let orphan_root = home.join("orphan");
        std::fs::create_dir_all(&orphan_root).unwrap();
        let ctx = context_for_root(&orphan_root, &hh).expect("ctx");
        write_metadata(&ctx, None).expect("meta"); // creates the store dir + metadata.json
                                                   // Registry stays empty; list still finds it via the on-disk metadata scan.
        let ids: std::collections::HashSet<String> = list_projects(&hh)
            .expect("list")
            .into_iter()
            .map(|c| c.id)
            .collect();
        assert!(
            ids.contains(&ctx.id),
            "on-disk store not enumerated: {ids:?}"
        );
    }

    #[test]
    fn switch_current_project_updates_registry_and_marker() {
        let (_g, home, hh) = isolated();
        let a = home.join("sw-a");
        let b = home.join("sw-b");
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(&b).unwrap();
        let ctx_a = register_and_activate(&hh, &a, "t1").expect("a");
        let ctx_b = register_and_activate(&hh, &b, "t2").expect("b");
        // b is current after the second register_and_activate.
        assert_eq!(
            active_project_id(&hh).expect("aid").as_deref(),
            Some(ctx_b.id.as_str())
        );

        // Switch back to a.
        let switched = switch_current_project(&hh, &ctx_a.id, "t3").expect("switch");
        assert_eq!(switched.id, ctx_a.id);
        assert_eq!(
            active_project_id(&hh).expect("aid").as_deref(),
            Some(ctx_a.id.as_str())
        );
        assert_eq!(
            read_active_project(&hh).expect("marker").as_deref(),
            Some(ctx_a.id.as_str())
        );

        // Switching to an unknown id is rejected (never strands the pointer).
        assert!(switch_current_project(&hh, "no-such-project", "t4").is_err());
        // Active project is unchanged after the rejected switch.
        assert_eq!(
            active_project_id(&hh).expect("aid").as_deref(),
            Some(ctx_a.id.as_str())
        );
    }
}
