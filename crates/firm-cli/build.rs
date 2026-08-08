//! Embeds server build provenance as compile-time env vars so `GET /v1/meta`
//! (issue #307) never has to shell out per-request. Both values are
//! best-effort: a build in an environment without `git` (a source archive, a
//! container image with git stripped) still compiles — `main.rs` falls back to
//! "unknown" / `null` via `option_env!` when the revision cannot be proven.
//! Release/archive builds may supply `FIRM_BUILD_GIT_REV`; only a full 40-hex
//! object id is accepted.

use std::env;
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn main() {
    println!("cargo:rerun-if-env-changed=FIRM_BUILD_GIT_REV");
    emit_git_rerun_paths();
    if let Some(rev) = exact_git_rev() {
        println!("cargo:rustc-env=FIRM_BUILD_GIT_REV={rev}");
    }
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0);
    println!("cargo:rustc-env=FIRM_BUILD_AT_MS={millis}");
}

/// Resolve one exact revision, preferring an explicit archive-build value and
/// otherwise asking git for the full `HEAD` object id. `Command::current_dir`
/// defaults to this build script's own
/// working directory (the crate root); git resolves the real gitdir itself
/// even from inside a linked worktree, so no special-casing is needed here.
/// Returns `None` — never panics/fails the build — when git is missing, this
/// tree is not a git checkout, or the value is not exactly 40 hexadecimal
/// characters.
fn exact_git_rev() -> Option<String> {
    if let Ok(rev) = env::var("FIRM_BUILD_GIT_REV") {
        return normalize_exact_rev(&rev);
    }
    let output = Command::new("git")
        .args(["rev-parse", "--verify", "HEAD^{commit}"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    normalize_exact_rev(&String::from_utf8(output.stdout).ok()?)
}

fn normalize_exact_rev(raw: &str) -> Option<String> {
    let rev = raw.trim();
    (rev.len() == 40 && rev.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .then(|| rev.to_ascii_lowercase())
}

/// Make a branch/HEAD-only commit invalidate Cargo's cached build-script
/// output, including from a linked worktree. Failure is harmless for source
/// archives, where `FIRM_BUILD_GIT_REV` is the exact-revision input.
fn emit_git_rerun_paths() {
    for git_path in ["HEAD", "refs/heads"] {
        let Ok(output) = Command::new("git")
            .args(["rev-parse", "--git-path", git_path])
            .output()
        else {
            continue;
        };
        if !output.status.success() {
            continue;
        }
        let Ok(path) = String::from_utf8(output.stdout) else {
            continue;
        };
        let path = PathBuf::from(path.trim());
        if !path.as_os_str().is_empty() {
            println!("cargo:rerun-if-changed={}", path.display());
        }
    }
}
