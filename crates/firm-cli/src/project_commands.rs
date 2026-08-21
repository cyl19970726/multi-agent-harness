//! CLI commands for Project Binding selection and legacy-store migration.

use super::*;

/// `harness project <subcommand>` — inspect and manage Project Bindings.
///
/// A switch changes the default provider cwd/config/Skill boundary only.
/// Native coordination routing remains owned by `harness space`.
pub(super) fn project_command(args: &[String]) -> CliResult<()> {
    require_subcommand(args, "project add|list|current|switch|remove|show|migrate")?;
    let firm_home = project::firm_home().map_err(project_err)?;
    match args[0].as_str() {
        "add" => project_add(&firm_home, &args[1..]),
        "list" => project_list(&firm_home),
        "current" => project_current(&firm_home),
        "switch" => project_switch_cmd(&firm_home, &args[1..]),
        "remove" => project_remove(&firm_home, &args[1..]),
        "show" => project_show(&firm_home, &args[1..]),
        "migrate" => project_migrate(&firm_home, &args[1..]),
        other => Err(CliError::Usage(format!("unknown project command: {other}"))),
    }
}

/// `harness project add [<path>] [--switch]` — register a project root (defaulting
/// to the current directory) WITHOUT changing the active project, unless `--switch`
/// is passed. Materializes the central store + `metadata.json` and a registry entry.
fn project_add(firm_home: &Path, args: &[String]) -> CliResult<()> {
    let switch = has_flag(args, "--switch");
    // First non-flag positional is an optional explicit project root.
    let path = args.iter().find(|a| !a.starts_with("--")).cloned();
    let project_root = match path {
        Some(p) => PathBuf::from(p),
        None => env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
    };
    let now = now_string();
    // `register_and_activate` materializes store + metadata + registry entry and
    // marks current. When `--switch` is NOT requested we restore the previously
    // active project so `add` is non-disruptive (inspectable before a switch).
    let prev_active = project::active_project_id(firm_home).map_err(project_err)?;
    let ctx =
        project::register_and_activate(firm_home, &project_root, &now).map_err(project_err)?;
    if !switch {
        match prev_active {
            Some(prev) if prev != ctx.id => {
                project::switch_current_project(firm_home, &prev, &now).map_err(project_err)?;
            }
            None => {
                // There was no active project before; clear the pointer so `add`
                // alone never silently flips the default away from local/_global.
                let mut registry =
                    project::ProjectRegistry::load(firm_home).map_err(project_err)?;
                registry.current_project_id = None;
                registry.save(firm_home).map_err(project_err)?;
                project::clear_active_project(firm_home).map_err(project_err)?;
            }
            _ => {}
        }
    }
    let current = project::active_project_id(firm_home)
        .map_err(project_err)?
        .unwrap_or_default();
    print_json(&project_context_json(&ctx, &current))
}

/// `harness project list` — enumerate every known project (registry + on-disk
/// stores + the reserved `_global`), marking the current one.
fn project_list(firm_home: &Path) -> CliResult<()> {
    let current = project::active_project_id(firm_home)
        .map_err(project_err)?
        .unwrap_or_default();
    let projects = project::list_projects(firm_home).map_err(project_err)?;
    let json: Vec<serde_json::Value> = projects
        .iter()
        .map(|c| project_context_json(c, &current))
        .collect();
    print_json(&json)
}

/// `harness project current` — print the currently-active project context (the
/// convergence point `serve` + CLI workers resolve), or a `null`-id placeholder if
/// none has been selected yet.
fn project_current(firm_home: &Path) -> CliResult<()> {
    match project::active_project_id(firm_home).map_err(project_err)? {
        Some(id) => match project::context_for_id(firm_home, &id).map_err(project_err)? {
            Some(ctx) => print_json(&project_context_json(&ctx, &id)),
            None => print_json(&serde_json::json!({ "id": id, "is_current": true })),
        },
        None => print_json(&serde_json::json!({
            "id": serde_json::Value::Null,
            "is_current": false,
        })),
    }
}

/// `harness project switch <id|path>` — flip the active project, updating BOTH the
/// registry `current_project_id` and the `ACTIVE_PROJECT` marker so the next CLI
/// invocation and a live `serve` converge on the same central store.
fn project_switch_cmd(firm_home: &Path, args: &[String]) -> CliResult<()> {
    let selector = args
        .first()
        .filter(|a| !a.starts_with("--"))
        .cloned()
        .ok_or_else(|| CliError::Usage("usage: harness project switch <id|path>".to_string()))?;
    // Accept either a registered id / `_global`, or a path to a project root.
    let id = match project::context_for_id(firm_home, &selector).map_err(project_err)? {
        Some(ctx) => ctx.id,
        None => match resolve_project_selector(firm_home, &selector) {
            Some(ctx) => {
                // A path that is not yet registered: register it first so the switch
                // never strands the pointer on an unknown id.
                project::register_and_activate(firm_home, &ctx.project_root, &now_string())
                    .map_err(project_err)?;
                ctx.id
            }
            None => {
                return Err(CliError::Usage(format!(
                    "unknown project: {selector} (not a registered id, path, or `_global`)"
                )))
            }
        },
    };
    let ctx =
        project::switch_current_project(firm_home, &id, &now_string()).map_err(project_err)?;
    print_json(&project_context_json(&ctx, &ctx.id))
}

/// `harness project remove <id> [--force]` — unregister a project (the on-disk
/// central store is left intact; this is a pointer operation). The reserved
/// `_global` cannot be removed. Removing the CURRENT project requires `--force` and
/// clears the active pointer so resolution falls back safely.
fn project_remove(firm_home: &Path, args: &[String]) -> CliResult<()> {
    let force = has_flag(args, "--force");
    let id = args
        .iter()
        .find(|a| !a.starts_with("--"))
        .cloned()
        .ok_or_else(|| {
            CliError::Usage("usage: harness project remove <id> [--force]".to_string())
        })?;
    let current = project::active_project_id(firm_home).map_err(project_err)?;
    if current.as_deref() == Some(id.as_str()) && !force {
        return Err(CliError::Usage(format!(
            "`{id}` is the current project; switch away first or pass --force to remove it"
        )));
    }
    let outcome = project::remove_project(firm_home, &id).map_err(project_err)?;
    if !outcome.removed {
        return Err(CliError::Usage(format!(
            "no registered project with id `{id}`"
        )));
    }
    if outcome.was_current {
        eprintln!(
            "note: removed the active project `{id}`; no project is selected now \
             (resolution falls back to the legacy walk-up / `_global`)"
        );
    }
    print_json(&serde_json::json!({
        "removed": id,
        "was_current": outcome.was_current,
    }))
}

/// `harness project show <id|path>` — print one project's resolved context. With no
/// argument, shows the current project (alias for `current`).
fn project_show(firm_home: &Path, args: &[String]) -> CliResult<()> {
    let selector = args.iter().find(|a| !a.starts_with("--")).cloned();
    let current = project::active_project_id(firm_home)
        .map_err(project_err)?
        .unwrap_or_default();
    let ctx = match selector {
        None => return project_current(firm_home),
        Some(sel) => match project::context_for_id(firm_home, &sel).map_err(project_err)? {
            Some(ctx) => ctx,
            None => resolve_project_selector(firm_home, &sel)
                .ok_or_else(|| CliError::Usage(format!("unknown project: {sel}")))?,
        },
    };
    print_json(&project_context_json(&ctx, &current))
}

/// The clean-cutover project migration copies only canonical ledgers. Provider
/// native sessions, launch payloads and runtime files remain provider truth and
/// are never imported into an Execution Space.
const STORE_PAYLOAD_DIRS: &[&str] = &[];

/// `harness project migrate [<local-store>] [--switch]` — move an existing
/// repo-local `.harness/` store into the centralized per-project store
/// (goal-multi-project P7 / project-migrate task).
///
/// Steps: compute the project's canonical id from the repo root (the local store's
/// PARENT dir), copy every `*.jsonl` ledger + the payload dirs into
/// `~/.harness/projects/<id>/`, write `metadata.json` with `migrated_from`, and drop
/// a `MIGRATED_TO_CENTRAL` marker in the old store pointing at the central one.
///
/// Idempotent / fail-safe: if the local store is ALREADY marked migrated it reports
/// success without recopying; if the central store already has ledger rows it
/// refuses (to avoid clobbering newer central data) unless `--force` is given.
fn project_migrate(firm_home: &Path, args: &[String]) -> CliResult<()> {
    let force = has_flag(args, "--force");
    let switch = has_flag(args, "--switch");

    // Resolve the local store dir: explicit positional, else the cwd's `.harness`.
    let positional = args.iter().find(|a| !a.starts_with("--")).cloned();
    let local_store = match positional {
        Some(p) => PathBuf::from(p),
        None => env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(".harness"),
    };
    if !local_store.is_dir() {
        return Err(CliError::Usage(format!(
            "no local store to migrate at {} (pass a path or run from a repo with ./.harness)",
            local_store.display()
        )));
    }

    // Already migrated? Report idempotently rather than recopying.
    if let Some(target) = project::read_migrated_marker(&local_store).map_err(project_err)? {
        println!(
            "already migrated: {} → {}",
            local_store.display(),
            target.display()
        );
        return Ok(());
    }

    // The project ROOT is the local store's parent (the repo dir), not the store
    // itself, so the id matches what `init`/`switch` would derive for that repo.
    let project_root = local_store
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| local_store.clone());
    let ctx = project::context_for_root(&project_root, firm_home).map_err(project_err)?;

    // Refuse to clobber a central store that already holds ledger data, unless
    // forced. A central store that only has metadata.json (freshly created) is fine.
    if central_store_has_ledger_rows(&ctx.store_root) && !force {
        return Err(CliError::Usage(format!(
            "central store {} already has data; refusing to overwrite (pass --force to merge-copy)",
            ctx.store_root.display()
        )));
    }

    let before = count_store_records(&local_store)?;
    std::fs::create_dir_all(&ctx.store_root)?;
    let copied = copy_store_contents(&local_store, &ctx.store_root)?;
    let after = count_store_records(&ctx.store_root)?;

    // Register the project, then pin identity in the central store with a
    // `migrated_from` breadcrumb. The metadata write comes AFTER
    // `register_and_activate` (which itself writes a `migrated_from`-less
    // metadata.json) so the breadcrumb is the one that survives.
    let now = now_string();
    let prev_active = project::active_project_id(firm_home).map_err(project_err)?;
    project::register_and_activate(firm_home, &project_root, &now).map_err(project_err)?;
    project::write_metadata(&ctx, Some(local_store.clone())).map_err(project_err)?;
    if !switch {
        // Non-disruptive by default: restore the previously active project (or clear
        // if none) so a bare `migrate` does not silently flip the active project.
        match prev_active {
            Some(prev) if prev != ctx.id => {
                project::switch_current_project(firm_home, &prev, &now).map_err(project_err)?;
            }
            None => {
                let mut registry =
                    project::ProjectRegistry::load(firm_home).map_err(project_err)?;
                registry.current_project_id = None;
                registry.save(firm_home).map_err(project_err)?;
                project::clear_active_project(firm_home).map_err(project_err)?;
            }
            _ => {}
        }
    }
    project::write_migrated_marker(&local_store, &ctx.store_root).map_err(project_err)?;

    print_json(&serde_json::json!({
        "migrated": true,
        "project_id": ctx.id,
        "from": local_store.display().to_string(),
        "to": ctx.store_root.display().to_string(),
        "files_copied": copied,
        "records_before": before,
        "records_after": after,
        "switched": switch,
    }))
}

/// Whether a central store already holds any `*.jsonl` ledger rows (used to guard
/// `migrate` against clobbering newer central data). A bare `metadata.json` does not
/// count as data.
fn central_store_has_ledger_rows(store_root: &Path) -> bool {
    count_store_records(store_root)
        .map(|n| n > 0)
        .unwrap_or(false)
}

/// Count total non-empty lines across every `*.jsonl` file in a store dir (the
/// record-count metric `migrate` reports before/after). Missing dir → 0.
fn count_store_records(store_root: &Path) -> CliResult<u64> {
    let mut total = 0u64;
    let read_dir = match std::fs::read_dir(store_root) {
        Ok(rd) => rd,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(e) => return Err(CliError::Io(e)),
    };
    for entry in read_dir.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("jsonl") {
            let text = std::fs::read_to_string(&path)?;
            total += text.lines().filter(|l| !l.trim().is_empty()).count() as u64;
        }
    }
    Ok(total)
}

/// Copy allowlisted canonical `*.jsonl` ledgers from `src` into `dst`, preserving filenames. Returns the
/// number of top-level entries copied. Existing destination files are overwritten
/// (merge-copy under `--force`); missing source payload dirs are skipped.
fn copy_store_contents(src: &Path, dst: &Path) -> CliResult<u64> {
    let mut copied = 0u64;
    // 1. JSONL ledgers (flat files at the store root).
    for entry in std::fs::read_dir(src)?.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("jsonl") {
            if let Some(name) = path.file_name() {
                let name = name.to_string_lossy();
                if !project_migration_ledger_allowed(&name) {
                    continue;
                }
                std::fs::copy(&path, dst.join(name.as_ref()))?;
                copied += 1;
            }
        }
    }
    debug_assert!(STORE_PAYLOAD_DIRS.is_empty());
    Ok(copied)
}

fn project_migration_ledger_allowed(name: &str) -> bool {
    matches!(
        name,
        "missions.jsonl"
            | "waves.jsonl"
            | "teams.jsonl"
            | "team_runs.jsonl"
            | "work_operations.jsonl"
            | "evidence.jsonl"
            | "decisions.jsonl"
            | "gaps.jsonl"
            | "host_attentions.jsonl"
            | "team_supervisor_leases.jsonl"
            | "delegation_runs.jsonl"
            | "team_run_events.jsonl"
            | "agentfirm_trust_operations.jsonl"
            | "workflow_runs.jsonl"
            | "workflow_steps.jsonl"
            | "workflow_patches.jsonl"
            | "workflow_artifact_manifests.jsonl"
    )
}

/// Return a stable relative-file index without following symlinks.
pub(super) fn directory_file_index(root: &Path) -> CliResult<Vec<PathBuf>> {
    fn visit(root: &Path, relative: &Path, out: &mut Vec<PathBuf>) -> CliResult<()> {
        let current = root.join(relative);
        for entry in std::fs::read_dir(current)? {
            let entry = entry?;
            let file_type = entry.file_type()?;
            let next = relative.join(entry.file_name());
            if file_type.is_dir() {
                visit(root, &next, out)?;
            } else if file_type.is_file() {
                out.push(next);
            } else {
                return Err(CliError::Usage(format!(
                    "execution migration refuses symbolic links or special files: {}",
                    root.join(next).display()
                )));
            }
        }
        Ok(())
    }

    let metadata = std::fs::symlink_metadata(root)?;
    if !metadata.file_type().is_dir() {
        return Err(CliError::Usage(format!(
            "execution migration refuses symbolic links or special files: {}",
            root.display()
        )));
    }
    let mut files = Vec::new();
    visit(root, Path::new(""), &mut files)?;
    files.sort();
    Ok(files)
}
