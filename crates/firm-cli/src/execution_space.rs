//! Execution Space identity and registry (ADR 0042).
//!
//! An Execution Space owns coordination ledgers. It does not own provider cwd,
//! Git state, project instructions, Skills, or Company OS truth.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use harness_core::ExecutionSpace;
use serde::{Deserialize, Serialize};

use crate::project;

const REGISTRY_FORMAT_VERSION: u32 = 1;
static ATOMIC_WRITE_SEQUENCE: AtomicU64 = AtomicU64::new(1);

#[derive(Debug)]
pub enum ExecutionSpaceError {
    Io(std::io::Error),
    Json(serde_json::Error),
    InvalidId(String),
    NoHome,
    /// A recorded registry `store_root` lies outside the current FIRM_HOME and
    /// the explicit override was not given; the message names the registry
    /// file, the recorded path and FIRM_HOME.
    ExternalStoreRoot(String),
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
            Self::ExternalStoreRoot(message) => write!(f, "{message}"),
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

fn registry_lock_path(firm_home: &Path) -> PathBuf {
    spaces_dir(firm_home).join(".registry.lock")
}

/// Exclusive advisory lock for execution-space registry and ACTIVE_SPACE
/// mutations. The standard-library lock maps to `flock` on Unix and
/// `LockFileEx` on Windows and is released on drop, including process exit.
pub struct ExecutionSpaceRegistryLock {
    file: File,
    lock_path: PathBuf,
}

impl Drop for ExecutionSpaceRegistryLock {
    fn drop(&mut self) {
        let _ = self.file.unlock();
    }
}

pub fn acquire_registry_lock(firm_home: &Path) -> ExecutionSpaceResult<ExecutionSpaceRegistryLock> {
    std::fs::create_dir_all(spaces_dir(firm_home))?;
    let lock_path = registry_lock_path(firm_home);
    let file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(&lock_path)?;
    file.lock()?;
    Ok(ExecutionSpaceRegistryLock { file, lock_path })
}

/// Replace a small control-plane file without exposing a truncated destination.
/// The temporary file is created beside the destination, flushed, and then
/// atomically renamed over it. Callers still own any higher-level serialization
/// (for example [`ExecutionSpaceRegistryLock`]).
pub(crate) fn atomic_write_bytes(path: &Path, bytes: &[u8]) -> ExecutionSpaceResult<()> {
    atomic_write_bytes_with_hooks(path, bytes, |_| Ok(()), sync_parent_directory)
}

#[cfg(test)]
fn atomic_write_bytes_with_hook<F>(
    path: &Path,
    bytes: &[u8],
    before_publish: F,
) -> ExecutionSpaceResult<()>
where
    F: FnOnce(&Path) -> std::io::Result<()>,
{
    atomic_write_bytes_with_hooks(path, bytes, before_publish, sync_parent_directory)
}

fn atomic_write_bytes_with_hooks<F, G>(
    path: &Path,
    bytes: &[u8],
    before_publish: F,
    sync_after_publish: G,
) -> ExecutionSpaceResult<()>
where
    F: FnOnce(&Path) -> std::io::Result<()>,
    G: FnOnce(&Path) -> std::io::Result<()>,
{
    let parent = path.parent().ok_or_else(|| {
        ExecutionSpaceError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("control-plane path has no parent: {}", path.display()),
        ))
    })?;
    std::fs::create_dir_all(parent)?;
    let file_name = path.file_name().ok_or_else(|| {
        ExecutionSpaceError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("control-plane path has no file name: {}", path.display()),
        ))
    })?;

    let mut temporary = None;
    for _ in 0..32 {
        let sequence = ATOMIC_WRITE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let candidate = parent.join(format!(
            ".{}.{}.{}.tmp",
            file_name.to_string_lossy(),
            std::process::id(),
            sequence
        ));
        match OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&candidate)
        {
            Ok(file) => {
                temporary = Some((candidate, file));
                break;
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.into()),
        }
    }
    let (temporary_path, mut file) = temporary.ok_or_else(|| {
        ExecutionSpaceError::Io(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            format!(
                "could not reserve a temporary control-plane file beside {}",
                path.display()
            ),
        ))
    })?;

    let result = (|| -> std::io::Result<()> {
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        before_publish(&temporary_path)?;
        atomic_replace(&temporary_path, path)?;
        // The new complete file is already the only visible destination. A
        // directory-sync failure cannot be reported as "not published" without
        // lying to callers, and this layer does not promise crash durability.
        // Still attempt the sync on platforms that support directory handles.
        let _ = sync_after_publish(parent);
        Ok(())
    })();
    if let Err(error) = result {
        let _ = std::fs::remove_file(&temporary_path);
        return Err(error.into());
    }
    Ok(())
}

#[cfg(not(windows))]
fn atomic_replace(source: &Path, destination: &Path) -> std::io::Result<()> {
    std::fs::rename(source, destination)
}

#[cfg(windows)]
fn atomic_replace(source: &Path, destination: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;

    const MOVEFILE_REPLACE_EXISTING: u32 = 0x1;
    const MOVEFILE_WRITE_THROUGH: u32 = 0x8;

    #[link(name = "Kernel32")]
    unsafe extern "system" {
        fn MoveFileExW(existing: *const u16, new: *const u16, flags: u32) -> i32;
    }

    fn wide(path: &Path) -> std::io::Result<Vec<u16>> {
        let mut value = path.as_os_str().encode_wide().collect::<Vec<_>>();
        if value.contains(&0) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Windows path contains an embedded NUL",
            ));
        }
        value.push(0);
        Ok(value)
    }

    let source = wide(source)?;
    let destination = wide(destination)?;
    // MoveFileExW performs the same-directory replace in one filesystem
    // operation and, unlike the removed ReplaceFileW path, has no backup-file
    // side effects to reconcile after a partial failure.
    let moved = unsafe {
        MoveFileExW(
            source.as_ptr(),
            destination.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if moved == 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(unix)]
fn sync_parent_directory(parent: &Path) -> std::io::Result<()> {
    File::open(parent)?.sync_all()
}

#[cfg(not(unix))]
fn sync_parent_directory(_parent: &Path) -> std::io::Result<()> {
    Ok(())
}

/// Publish a fully prepared directory only if the destination is still absent.
/// Unlike POSIX `rename`, this never replaces an empty directory that appeared
/// after a caller's final preflight check.
pub(crate) fn publish_directory_no_replace(
    source: &Path,
    destination: &Path,
) -> ExecutionSpaceResult<()> {
    if source.parent() != destination.parent() {
        return Err(ExecutionSpaceError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "execution-space publication requires source and destination in the same directory",
        )));
    }
    publish_directory_no_replace_platform(source, destination).map_err(Into::into)
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn publish_directory_no_replace_platform(source: &Path, destination: &Path) -> std::io::Result<()> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    let source = CString::new(source.as_os_str().as_bytes()).map_err(|_| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "source path contains an embedded NUL",
        )
    })?;
    let destination = CString::new(destination.as_os_str().as_bytes()).map_err(|_| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "destination path contains an embedded NUL",
        )
    })?;
    let result = unsafe {
        libc::renameat2(
            libc::AT_FDCWD,
            source.as_ptr(),
            libc::AT_FDCWD,
            destination.as_ptr(),
            libc::RENAME_NOREPLACE,
        )
    };
    if result == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
fn publish_directory_no_replace_platform(source: &Path, destination: &Path) -> std::io::Result<()> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    let source = CString::new(source.as_os_str().as_bytes()).map_err(|_| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "source path contains an embedded NUL",
        )
    })?;
    let destination = CString::new(destination.as_os_str().as_bytes()).map_err(|_| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "destination path contains an embedded NUL",
        )
    })?;
    let result =
        unsafe { libc::renamex_np(source.as_ptr(), destination.as_ptr(), libc::RENAME_EXCL) };
    if result == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

#[cfg(windows)]
fn publish_directory_no_replace_platform(source: &Path, destination: &Path) -> std::io::Result<()> {
    // Rust's Windows rename does not replace an existing destination.
    std::fs::rename(source, destination)
}

#[cfg(not(any(
    target_os = "linux",
    target_os = "android",
    target_os = "macos",
    target_os = "ios",
    windows
)))]
fn publish_directory_no_replace_platform(
    _source: &Path,
    _destination: &Path,
) -> std::io::Result<()> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "no-replace execution-space publication is unsupported on this platform",
    ))
}

fn validate_registry_lock(
    firm_home: &Path,
    lock: &ExecutionSpaceRegistryLock,
) -> ExecutionSpaceResult<()> {
    if lock.lock_path == registry_lock_path(firm_home) {
        Ok(())
    } else {
        Err(ExecutionSpaceError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "execution-space registry lock belongs to a different firm home",
        )))
    }
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
    /// Load the registry. Recorded `store_root` paths resolve against the
    /// current FIRM_HOME (relative paths join it; absolute in-home paths load
    /// unchanged); an external root is refused unless
    /// FIRM_ALLOW_EXTERNAL_STORE_ROOT=1.
    pub fn load(firm_home: &Path) -> ExecutionSpaceResult<Self> {
        match std::fs::read_to_string(registry_path(firm_home)) {
            Ok(text) if text.trim().is_empty() => Ok(Self::default()),
            Ok(text) => {
                let mut registry: Self = serde_json::from_str(&text)?;
                for entry in &mut registry.spaces {
                    entry.store_root = project::resolve_recorded_store_root(
                        firm_home,
                        &registry_path(firm_home),
                        &entry.store_root,
                    )
                    .map_err(ExecutionSpaceError::ExternalStoreRoot)?;
                }
                Ok(registry)
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(error.into()),
        }
    }

    pub fn save(&mut self, firm_home: &Path) -> ExecutionSpaceResult<()> {
        self.format_version = REGISTRY_FORMAT_VERSION;
        for entry in &mut self.spaces {
            entry.store_root = project::recorded_store_root_for_write(firm_home, &entry.store_root);
        }
        atomic_write_bytes(
            &registry_path(firm_home),
            serde_json::to_string_pretty(self)?.as_bytes(),
        )
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

pub fn context_for_id(firm_home: &Path, id: &str) -> ExecutionSpaceResult<Option<ExecutionSpace>> {
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
    let lock = acquire_registry_lock(firm_home)?;
    register_and_activate_locked(
        firm_home,
        &lock,
        id,
        name,
        default_project_binding_id,
        company_id,
        now,
    )
}

pub fn register_and_activate_locked(
    firm_home: &Path,
    lock: &ExecutionSpaceRegistryLock,
    id: &str,
    name: &str,
    default_project_binding_id: Option<String>,
    company_id: Option<String>,
    now: &str,
) -> ExecutionSpaceResult<ExecutionSpace> {
    validate_registry_lock(firm_home, lock)?;
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
    complete_pending_migration_registration(&context);
    Ok(context)
}

pub fn switch_current_space(
    firm_home: &Path,
    id: &str,
    now: &str,
) -> ExecutionSpaceResult<ExecutionSpace> {
    let lock = acquire_registry_lock(firm_home)?;
    switch_current_space_locked(firm_home, &lock, id, now)
}

pub fn switch_current_space_locked(
    firm_home: &Path,
    lock: &ExecutionSpaceRegistryLock,
    id: &str,
    now: &str,
) -> ExecutionSpaceResult<ExecutionSpace> {
    validate_registry_lock(firm_home, lock)?;
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
    complete_pending_migration_registration(&context);
    Ok(context)
}

/// Best-effort reconciliation for the independently published migration
/// receipt. Registry/ACTIVE_SPACE have already been successfully written by
/// this process when this runs, so a receipt write failure is warning-only and
/// must not turn a successful register or switch into a reported failure.
fn complete_pending_migration_registration(context: &ExecutionSpace) {
    complete_pending_migration_registration_with_writer(context, atomic_write_bytes);
}

fn complete_pending_migration_registration_with_writer<F>(context: &ExecutionSpace, write: F)
where
    F: FnOnce(&Path, &[u8]) -> ExecutionSpaceResult<()>,
{
    let path = context.store_root.join("execution_space_migration.json");
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
        Err(error) => {
            eprintln!(
                "warning: execution space is active, but migration manifest could not be read at {}: {error}",
                path.display()
            );
            return;
        }
    };
    let mut manifest: serde_json::Value = match serde_json::from_slice(&bytes) {
        Ok(value) => value,
        Err(error) => {
            eprintln!(
                "warning: execution space is active, but migration manifest is invalid at {}: {error}",
                path.display()
            );
            return;
        }
    };
    let expected_recovery = format!("harness space switch {}", context.id);
    if manifest["registration"]["status"] != "pending"
        || manifest["registration"]["recovery_command"] != expected_recovery
    {
        return;
    }
    manifest["registration"]["status"] = serde_json::Value::String("complete".into());
    let result = serde_json::to_vec_pretty(&manifest)
        .map_err(ExecutionSpaceError::from)
        .and_then(|bytes| write(&path, &bytes));
    if let Err(error) = result {
        eprintln!(
            "warning: execution space is active, but migration manifest remains registration pending at {}: {error}",
            path.display()
        );
    }
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
    atomic_write_bytes(
        &context.store_root.join("metadata.json"),
        serde_json::to_string_pretty(&metadata)?.as_bytes(),
    )
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
        project::ProjectError::ExternalStoreRoot(message) => {
            ExecutionSpaceError::ExternalStoreRoot(message)
        }
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

    #[test]
    fn registry_lock_serializes_mutations_across_file_handles() {
        let home = temp_home("registry-lock");
        let first = acquire_registry_lock(&home).expect("first registry lock");
        let (started_tx, started_rx) = std::sync::mpsc::sync_channel(0);
        let (acquired_tx, acquired_rx) = std::sync::mpsc::sync_channel(0);
        let other_home = home.clone();
        let waiter = std::thread::spawn(move || {
            started_tx.send(()).expect("announce lock attempt");
            let second = acquire_registry_lock(&other_home).expect("second registry lock");
            acquired_tx.send(()).expect("announce acquired lock");
            drop(second);
        });
        started_rx.recv().expect("waiter started");
        assert!(
            acquired_rx
                .recv_timeout(std::time::Duration::from_millis(100))
                .is_err(),
            "a second registry writer acquired the lock before the first released it"
        );
        drop(first);
        acquired_rx
            .recv_timeout(std::time::Duration::from_secs(2))
            .expect("waiter acquires after release");
        waiter.join().expect("lock waiter");
        let _ = std::fs::remove_dir_all(home);
    }

    #[test]
    fn failed_atomic_registry_publication_preserves_previous_json() {
        let home = temp_home("registry-atomic-failure");
        let mut original = ExecutionSpaceRegistry {
            current_space_id: Some("original".into()),
            ..ExecutionSpaceRegistry::default()
        };
        original.save(&home).expect("initial registry");
        let path = registry_path(&home);
        let original_bytes = std::fs::read(&path).expect("initial bytes");

        let mut replacement = original.clone();
        replacement.current_space_id = Some("replacement".into());
        let replacement_bytes = serde_json::to_vec_pretty(&replacement).expect("serialize");
        let error = atomic_write_bytes_with_hook(&path, &replacement_bytes, |_| {
            Err(std::io::Error::other(
                "injected failure before atomic rename",
            ))
        })
        .expect_err("publication failure");

        assert!(error.to_string().contains("injected failure"));
        assert_eq!(
            std::fs::read(&path).expect("registry bytes"),
            original_bytes
        );
        assert_eq!(
            ExecutionSpaceRegistry::load(&home)
                .expect("registry remains parseable")
                .current_space_id
                .as_deref(),
            Some("original")
        );
        assert!(
            std::fs::read_dir(spaces_dir(&home))
                .expect("spaces directory")
                .all(|entry| !entry
                    .expect("directory entry")
                    .file_name()
                    .to_string_lossy()
                    .ends_with(".tmp")),
            "failed publication must clean its temporary file"
        );
        let _ = std::fs::remove_dir_all(home);
    }

    #[test]
    fn failed_atomic_metadata_publication_preserves_recoverable_previous_json() {
        let home = temp_home("metadata-atomic-failure");
        let context = ExecutionSpace {
            id: "recoverable-space".into(),
            name: "Recoverable Space".into(),
            store_root: space_store_root(&home, "recoverable-space"),
            default_project_binding_id: Some("project-original".into()),
            company_id: None,
        };
        write_metadata(&context).expect("initial metadata");
        let path = context.store_root.join("metadata.json");
        let original_bytes = std::fs::read(&path).expect("initial metadata bytes");
        let replacement = ExecutionSpaceMetadata {
            space_id: context.id.clone(),
            name: "Replacement".into(),
            default_project_binding_id: Some("project-replacement".into()),
            company_id: None,
        };
        let replacement_bytes = serde_json::to_vec_pretty(&replacement).expect("serialize");

        atomic_write_bytes_with_hook(&path, &replacement_bytes, |_| {
            Err(std::io::Error::other(
                "injected metadata publication failure",
            ))
        })
        .expect_err("metadata publication failure");
        assert_eq!(
            std::fs::read(&path).expect("metadata after failure"),
            original_bytes
        );
        assert_eq!(
            read_metadata(&context.store_root)
                .expect("metadata remains parseable")
                .expect("metadata remains present")
                .default_project_binding_id
                .as_deref(),
            Some("project-original")
        );
        let _ = std::fs::remove_dir_all(home);
    }

    #[test]
    fn directory_sync_failure_after_atomic_replace_does_not_report_unpublished() {
        let home = temp_home("atomic-post-publish-sync");
        let path = spaces_dir(&home).join("receipt.json");
        atomic_write_bytes(&path, br#"{"status":"old"}"#).expect("initial receipt");

        atomic_write_bytes_with_hooks(
            &path,
            br#"{"status":"new"}"#,
            |_| Ok(()),
            |_| Err(std::io::Error::other("injected directory sync failure")),
        )
        .expect("atomic replacement is already published despite directory sync failure");
        let receipt: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).expect("published receipt"))
                .expect("published receipt remains parseable");
        assert_eq!(receipt["status"], "new");
        let _ = std::fs::remove_dir_all(home);
    }

    #[test]
    fn failed_manifest_completion_preserves_pending_recovery_receipt() {
        let home = temp_home("manifest-atomic-failure");
        let context = ExecutionSpace {
            id: "pending-space".into(),
            name: "Pending Space".into(),
            store_root: space_store_root(&home, "pending-space"),
            default_project_binding_id: Some("project-1".into()),
            company_id: None,
        };
        std::fs::create_dir_all(&context.store_root).expect("store root");
        let path = context.store_root.join("execution_space_migration.json");
        let pending = serde_json::json!({
            "registration": {
                "status": "pending",
                "recovery_command": "harness space switch pending-space"
            }
        });
        let pending_bytes = serde_json::to_vec_pretty(&pending).expect("serialize pending");
        atomic_write_bytes(&path, &pending_bytes).expect("initial manifest");

        complete_pending_migration_registration_with_writer(&context, |path, bytes| {
            atomic_write_bytes_with_hook(path, bytes, |_| {
                Err(std::io::Error::other(
                    "injected manifest publication failure",
                ))
            })
        });
        let after_failure: serde_json::Value = serde_json::from_slice(
            &std::fs::read(&path).expect("manifest after failed completion"),
        )
        .expect("pending manifest remains parseable");
        assert_eq!(after_failure["registration"]["status"], "pending");

        complete_pending_migration_registration(&context);
        let completed: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).expect("completed manifest"))
                .expect("completed manifest remains parseable");
        assert_eq!(completed["registration"]["status"], "complete");
        let _ = std::fs::remove_dir_all(home);
    }
}
