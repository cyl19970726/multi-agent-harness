//! Embeds server build provenance as compile-time env vars so `GET /v1/meta`
//! (issue #307) never has to shell out per-request. Both values are
//! best-effort: a build in an environment without `git` (a source tarball, a
//! container image with git stripped) still compiles — `main.rs` falls back to
//! "unknown" / `null` via `option_env!` when the var was never set here.
//!
//! Cargo's default build-script re-run heuristic (re-run when any file in this
//! package changes) is good enough: any commit that touches `harness-cli`
//! source picks up the new HEAD on the next build. A code-free commit leaving a
//! stale embedded rev is an accepted, low-stakes edge case for a diagnostic
//! field, not a correctness requirement.

use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn main() {
    if let Some(rev) = git_short_rev() {
        println!("cargo:rustc-env=HARNESS_BUILD_GIT_REV={rev}");
    }
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0);
    println!("cargo:rustc-env=HARNESS_BUILD_AT_MS={millis}");
}

/// Best-effort `git rev-parse --short HEAD`, run once at compile time (never at
/// request time). `Command::current_dir` defaults to this build script's own
/// working directory (the crate root); git resolves the real gitdir itself
/// even from inside a linked worktree, so no special-casing is needed here.
/// Returns `None` — never panics/fails the build — when git is missing, this
/// tree is not a git checkout, or HEAD cannot be resolved.
fn git_short_rev() -> Option<String> {
    let output = Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let rev = String::from_utf8(output.stdout).ok()?;
    let rev = rev.trim();
    (!rev.is_empty()).then(|| rev.to_string())
}
