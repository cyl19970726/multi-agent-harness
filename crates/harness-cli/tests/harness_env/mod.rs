//! Shared test helper: an isolated harness HOME so integration tests never touch
//! the developer's real `~/.harness` (goal-multi-project "Test isolation" risk).
//!
//! `TempHome` creates a unique temp dir, points `HOME` and `HARNESS_HOME` at it,
//! and exposes the registry/marker paths. It is passed to spawned `harness`
//! processes via `.envs(home.envs())`; we never mutate the test process's own env
//! (which would race across parallel tests).

#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static COUNTER: AtomicU64 = AtomicU64::new(0);

pub struct TempHome {
    base: PathBuf,
    home: PathBuf,
    harness_home: PathBuf,
}

impl TempHome {
    pub fn new(tag: &str) -> Self {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let pid = std::process::id();
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let base = std::env::temp_dir().join(format!("harness-it-{tag}-{pid}-{nanos}-{n}"));
        let home = base.join("home");
        let harness_home = home.join(".harness");
        std::fs::create_dir_all(&harness_home).expect("create temp harness home");
        // Canonicalize HOME so the binary's `project_id_for_path` (which
        // canonicalizes) derives slugs against the same root the tests assert on.
        let home = std::fs::canonicalize(&home).expect("canonicalize home");
        let harness_home = home.join(".harness");
        Self {
            base,
            home,
            harness_home,
        }
    }

    pub fn base(&self) -> &Path {
        &self.base
    }

    pub fn home(&self) -> &Path {
        &self.home
    }

    pub fn harness_home(&self) -> &Path {
        &self.harness_home
    }

    pub fn projects_dir(&self) -> PathBuf {
        self.harness_home.join("projects")
    }

    pub fn registry_path(&self) -> PathBuf {
        self.projects_dir().join("registry.json")
    }

    pub fn active_marker_path(&self) -> PathBuf {
        self.harness_home.join("ACTIVE_PROJECT")
    }

    /// Env pairs to pass to a spawned `harness` process.
    pub fn envs(&self) -> Vec<(String, String)> {
        vec![
            ("HOME".to_string(), self.home.display().to_string()),
            (
                "HARNESS_HOME".to_string(),
                self.harness_home.display().to_string(),
            ),
        ]
    }
}

impl Drop for TempHome {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.base);
    }
}
