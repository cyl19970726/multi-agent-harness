use std::env;
use std::path::{Path, PathBuf};

use harness_core::ExecutionSpace;

use super::{
    execution_space, execution_space_err, project, project_err, CliError, CliResult,
    HARNESS_WORKFLOW_CHILD_STORE_ROOT_ENV,
};

/// How the active store root was chosen — surfaced via the `--store-source` debug
/// flag and used to keep back-compat behavior auditable (goal-multi-project P1/P7).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum StoreSource {
    /// `--store <path>` override (deprecated, kept for tests/back-compat).
    StoreFlag,
    /// Internal guard for workflow child processes. A workflow leaf may run with
    /// full provider permissions, so nested `harness ...` commands default to a
    /// session-local store unless the operator explicitly opts out.
    WorkflowChildEnv,
    /// `FIRM_ROOT` env override, or deprecated `HARNESS_ROOT` alias.
    FirmRootEnv,
    /// Deprecated `HARNESS_ROOT` compatibility alias.
    HarnessRootEnv,
    /// `--space <id>` explicit Execution Space selector.
    SpaceFlag,
    /// `FIRM_SPACE` Execution Space selector, with `HARNESS_SPACE` alias.
    SpaceEnv,
    /// Active Execution Space marker / registry current.
    SpaceCurrent,
    /// `--project <id|path>` explicit selector.
    ProjectFlag,
    /// `FIRM_PROJECT` Project Binding selector, with `HARNESS_PROJECT` alias.
    ProjectEnv,
    /// Registry `current_project_id` / `ACTIVE_PROJECT` marker.
    RegistryCurrent,
    /// Legacy cwd walk-up to the nearest existing `.harness/` (deprecation-warned).
    CwdWalkUp,
    /// Reserved GLOBAL project (`$HOME`), auto-created on first use.
    GlobalDefault,
}

/// The resolved coordination store plus its independent execution bindings.
///
/// `execution_space_context` owns Mission/Mission Log/Agent Team/Workflow rows.
/// `context` is the selected Project Binding compatibility adapter and owns
/// provider cwd, repository instructions, Skills, Git/worktree and permission
/// boundaries. Neither identity implies the other.
pub(crate) struct ResolvedStore {
    pub(super) root: PathBuf,
    pub(super) source: StoreSource,
    /// True only when this invocation selected its Project Binding with the
    /// global `--project` flag. Environment/current/default selections remain
    /// ambient context and are not operator authorization for scoped writes.
    pub(super) project_selection_explicit: bool,
    pub(super) context: Option<harness_core::ProjectContext>,
    pub(super) execution_space_context: Option<ExecutionSpace>,
}

impl ResolvedStore {
    /// Stable authority scope for operational provider admissions. Identity is
    /// taken from the selected Project Binding / Execution Space metadata,
    /// never from a path hash. Unbound raw/legacy stores intentionally have no
    /// scope and therefore cannot grant or consume admissions.
    pub(super) fn provider_compatibility_scope(&self) -> Option<(String, String)> {
        let project_id = self
            .context
            .as_ref()
            .map(|context| context.id.clone())
            .or_else(|| {
                self.execution_space_context.is_none().then(|| {
                    project::read_metadata(&self.root)
                        .ok()
                        .flatten()
                        .map(|metadata| metadata.project_id)
                })?
            })?;
        let store_id = self
            .execution_space_context
            .as_ref()
            .map(|space| format!("execution-space:{}", space.id))
            .unwrap_or_else(|| format!("project-store:{project_id}"));
        Some((project_id, store_id))
    }
}

/// Resolve the Harness coordination store and Project Binding.
///
/// Store precedence:
/// 1. `--store` / workflow-child store / `FIRM_ROOT` (`HARNESS_ROOT` alias).
/// 2. Company Store selector for `harness company ...`.
/// 3. `--space` / `FIRM_SPACE` (`HARNESS_SPACE` alias) / active Execution Space.
/// 4. project-derived compatibility store only when no Execution Space exists.
/// 5. legacy repo-local `.harness`, then active/global compatibility project.
///
/// Project Binding precedence is independent:
/// `--project` / `FIRM_PROJECT` (`HARNESS_PROJECT` alias), then the selected
/// space's default binding,
/// then the active Project Binding. Selecting it never switches the store.
///
/// `init` is special-cased so it never adopts an ancestor's `.harness` via the
/// walk-up; its routing lives in [`init_routed`].
///
/// IMPORTANT back-compat: when NONE of the project signals (3/4/5) and NO
/// override (1/2) apply, the result is the SAME directory today's code would have
/// used (walk-up → otherwise the GLOBAL store), so existing serve + run-script
/// flows keep converging on one store.
pub(super) fn resolve_store(
    args: &mut Vec<String>,
    command: Option<&str>,
) -> CliResult<ResolvedStore> {
    // Raw store overrides remain the highest-precedence compatibility path.
    if let Some(path) = take_flag_value(args, "--store") {
        warn_deprecated_override("--store", "harness space switch");
        return Ok(ResolvedStore {
            root: PathBuf::from(path),
            source: StoreSource::StoreFlag,
            project_selection_explicit: false,
            context: None,
            execution_space_context: None,
        });
    }
    if let Ok(root) = env::var(HARNESS_WORKFLOW_CHILD_STORE_ROOT_ENV) {
        if !root.is_empty() {
            return Ok(ResolvedStore {
                root: PathBuf::from(root),
                source: StoreSource::WorkflowChildEnv,
                project_selection_explicit: false,
                context: None,
                execution_space_context: None,
            });
        }
    }
    if let Some((root, used_legacy_alias)) = canonical_or_legacy_env("FIRM_ROOT", "HARNESS_ROOT") {
        return Ok(ResolvedStore {
            root: PathBuf::from(root),
            source: if used_legacy_alias {
                StoreSource::HarnessRootEnv
            } else {
                StoreSource::FirmRootEnv
            },
            project_selection_explicit: false,
            context: None,
            execution_space_context: None,
        });
    }

    let firm_home = match project::firm_home() {
        Ok(h) => h,
        // No HOME: fall back to the historical `./.harness` so we never panic.
        Err(_) => {
            return Ok(ResolvedStore {
                root: PathBuf::from(".harness"),
                source: StoreSource::CwdWalkUp,
                project_selection_explicit: false,
                context: None,
                execution_space_context: None,
            });
        }
    };

    // Project selection is now independent from execution-store selection. It
    // picks the provider workspace/config/permission binding only.
    let (project_selector, selector_source) = match take_flag_value(args, "--project") {
        Some(v) => (Some(v), StoreSource::ProjectFlag),
        None => match canonical_or_legacy_env("FIRM_PROJECT", "HARNESS_PROJECT") {
            Some((v, _)) => (Some(v), StoreSource::ProjectEnv),
            None => (None, StoreSource::ProjectFlag),
        },
    };
    // Only a command-line flag is an explicit authorization for an admission
    // scoped into an Execution Space. FIRM_PROJECT/HARNESS_PROJECT remain useful
    // ambient selectors for ordinary commands, but are intentionally insufficient
    // for this append-only trust decision.
    let project_selection_explicit =
        project_selector.is_some() && selector_source == StoreSource::ProjectFlag;
    let explicit_project_context = match project_selector.as_deref() {
        Some(selector) => Some(
            resolve_project_selector(&firm_home, selector)
                .ok_or_else(|| CliError::Usage(format!("unknown project binding: {selector}")))?,
        ),
        None => None,
    };

    // Company OS compatibility rows must never fall through into an Execution
    // Space. Until a native Company Store is selected, retain the historical
    // project-derived Company OS store as a narrow compatibility boundary.
    if command == Some("company") {
        let (context, source) = match explicit_project_context {
            Some(context) => (context, selector_source),
            None => match project::active_project_id(&firm_home).map_err(project_err)? {
                Some(id) => (
                    project::context_for_id(&firm_home, &id)
                        .map_err(project_err)?
                        .ok_or_else(|| {
                            CliError::Usage(format!("active project binding is unknown: {id}"))
                        })?,
                    StoreSource::RegistryCurrent,
                ),
                None => (
                    project::global_context(&firm_home).map_err(project_err)?,
                    StoreSource::GlobalDefault,
                ),
            },
        };
        return Ok(ResolvedStore {
            root: context.store_root.clone(),
            source,
            project_selection_explicit,
            context: Some(context),
            execution_space_context: None,
        });
    }

    // Execution Space owns Mission/Mission Log/Agent Team/Workflow coordination.
    // Selecting a Project Binding never changes this store.
    let (space_selector, space_source) = match take_flag_value(args, "--space") {
        Some(value) => (Some(value), StoreSource::SpaceFlag),
        None => match canonical_or_legacy_env("FIRM_SPACE", "HARNESS_SPACE") {
            Some((value, _)) => (Some(value), StoreSource::SpaceEnv),
            None => (
                execution_space::active_space_id(&firm_home).map_err(execution_space_err)?,
                StoreSource::SpaceCurrent,
            ),
        },
    };
    if let Some(space_id) = space_selector {
        let space = execution_space::context_for_id(&firm_home, &space_id)
            .map_err(execution_space_err)?
            .ok_or_else(|| CliError::Usage(format!("unknown execution space: {space_id}")))?;
        let project_context = match explicit_project_context {
            Some(context) => Some(context),
            None => match space.default_project_binding_id.as_deref() {
                Some(binding_id) => {
                    project::context_for_id(&firm_home, binding_id).map_err(project_err)?
                }
                None => project::active_project_id(&firm_home)
                    .map_err(project_err)?
                    .and_then(|id| project::context_for_id(&firm_home, &id).ok().flatten()),
            },
        };
        return Ok(ResolvedStore {
            root: space.store_root.clone(),
            source: space_source,
            project_selection_explicit,
            context: project_context,
            execution_space_context: Some(space),
        });
    }

    // No Execution Space was selected yet: preserve the old project-derived
    // compatibility store without silently migrating or dual-writing history.
    if let Some(context) = explicit_project_context {
        return Ok(ResolvedStore {
            root: context.store_root.clone(),
            source: selector_source,
            project_selection_explicit,
            context: Some(context),
            execution_space_context: None,
        });
    }

    // 5. Legacy cwd walk-up to the nearest existing `.harness/` (back-compat).
    // A PRESENT repo-local `.harness` WINS over the registry-current project
    // (rung 6): this restores the design's stated invariant that, absent an
    // explicit project signal, resolution lands on the SAME store today's code
    // would use — so standing inside a legacy repo never silently shadows its
    // local goals/tasks with an unrelated active project. (`init` never walks up
    // — it materializes a fresh store, see `init_routed`.)
    //
    // DUAL-READ (goal-multi-project P7): central (steps 3/4/5) was absent, so we may
    // fall back to a repo-local store — but ONLY if it has not been migrated. A local
    // store carrying a `MIGRATED_TO_CENTRAL` marker is redirected to the central
    // store it points to (never serving stale rows), and the choice is always logged.
    if command != Some("init") {
        if let Ok(cwd) = env::current_dir() {
            // A walked-up `.harness` that IS the central harness home (e.g.
            // `~/.harness`, which holds `projects/` + `registry.json`) is the
            // container for project stores, NOT a legacy repo-local store — skip it
            // so resolution falls through to the registry-current project (issue #89
            // convergence holds for cwds inside the home tree).
            let found = discover_harness_from(&cwd).filter(|p| {
                project::canonicalize_best_effort(p)
                    != project::canonicalize_best_effort(&firm_home)
            });
            if let Some(found) = found {
                match project::read_migrated_marker(&found) {
                    Ok(Some(target)) if !target.as_os_str().is_empty() => {
                        // Migrated: prefer the central store the marker points to.
                        eprintln!(
                            "store-source: local store {} is migrated; reading central store {}",
                            found.display(),
                            target.display()
                        );
                        let context = project::read_metadata(&target).ok().flatten().map(|meta| {
                            harness_core::ProjectContext {
                                id: meta.project_id,
                                project_root: meta.canonical_path,
                                store_root: target.clone(),
                                kind: meta.kind,
                                is_git_repo: meta.is_git_repo,
                            }
                        });
                        return Ok(ResolvedStore {
                            root: target,
                            source: StoreSource::RegistryCurrent,
                            project_selection_explicit: false,
                            context,
                            execution_space_context: None,
                        });
                    }
                    Ok(Some(_)) => {
                        // Marked migrated but pointer-less: ignore the local store and
                        // fall through to registry-current / the GLOBAL default
                        // rather than serve it.
                        eprintln!(
                            "store-source: local store {} is marked migrated (no target); \
                             skipping it for the active/global project",
                            found.display()
                        );
                    }
                    _ => {
                        // Unmigrated local store: keep working, but warn it is a
                        // back-compat fallback (no central project selected).
                        eprintln!(
                            "warning: using repo-local store {} (no central project selected); \
                             run `harness project migrate` to centralize it",
                            found.display()
                        );
                        warn_deprecated_override(
                            "cwd .harness walk-up",
                            "harness init / harness project switch",
                        );
                        return Ok(ResolvedStore {
                            root: found,
                            source: StoreSource::CwdWalkUp,
                            project_selection_explicit: false,
                            context: None,
                            execution_space_context: None,
                        });
                    }
                }
            }
        }
    }

    // 6. Registry current project (the cwd-independent convergence point) — the
    // resolver for project roots with NO repo-local `.harness` (e.g. a centrally
    // `init`ed project) and the cross-cwd convergence point (issue #89).
    if let Ok(Some(id)) = project::active_project_id(&firm_home) {
        if let Ok(Some(ctx)) = project::context_for_id(&firm_home, &id) {
            return Ok(ResolvedStore {
                root: ctx.store_root.clone(),
                source: StoreSource::RegistryCurrent,
                project_selection_explicit: false,
                context: Some(ctx),
                execution_space_context: None,
            });
        }
    }

    // 7. Reserved GLOBAL project, auto-created on first use.
    if let Ok(ctx) = project::global_context(&firm_home) {
        return Ok(ResolvedStore {
            root: ctx.store_root.clone(),
            source: StoreSource::GlobalDefault,
            project_selection_explicit: false,
            context: Some(ctx),
            execution_space_context: None,
        });
    }

    // Absolute last resort (no HOME / global failed): historical default.
    Ok(ResolvedStore {
        root: PathBuf::from(".harness"),
        source: StoreSource::CwdWalkUp,
        project_selection_explicit: false,
        context: None,
        execution_space_context: None,
    })
}

/// Resolve a `--project`/`FIRM_PROJECT` selector that may be a registered id OR
/// a path to a project root. `HARNESS_PROJECT` remains a deprecated alias.
/// Returns `None` if it cannot be resolved (caller then continues down the
/// precedence chain).
pub(super) fn resolve_project_selector(
    firm_home: &Path,
    selector: &str,
) -> Option<harness_core::ProjectContext> {
    // First: treat as a known id (registry / metadata / reserved `_global`).
    if let Ok(Some(ctx)) = project::context_for_id(firm_home, selector) {
        return Some(ctx);
    }
    // Otherwise: treat as a path to a project root and derive its identity.
    let candidate = Path::new(selector);
    if candidate.exists() {
        let canonical = project::canonicalize_best_effort(candidate);
        // Prefer a registered entry pinned to this canonical path (keeps a pinned
        // store_root even if path→id derivation later changes).
        if let Ok(registry) = project::ProjectRegistry::load(firm_home) {
            if let Some(entry) = registry.find_by_path(&canonical) {
                if let Ok(Some(ctx)) = project::context_for_id(firm_home, &entry.id) {
                    return Some(ctx);
                }
            }
        }
        if let Ok(ctx) = project::context_for_root(candidate, firm_home) {
            return Some(ctx);
        }
    }
    None
}

/// Emit a one-line deprecation warning for a legacy store-selection mechanism,
/// pointing at the supported replacement. Routed to stderr so it never corrupts
/// JSON stdout.
pub(super) fn warn_deprecated_override(what: &str, replacement: &str) {
    eprintln!("warning: {what} is deprecated for store selection; prefer `{replacement}`");
}

/// Read one canonical Firm selector with a deprecated Harness compatibility
/// alias. Empty values are treated as absent. The boolean reports whether the
/// legacy alias supplied the selected value, which lets callers retain precise
/// debug provenance without duplicating precedence logic.
fn canonical_or_legacy_env(canonical: &str, legacy: &str) -> Option<(String, bool)> {
    if let Ok(value) = env::var(canonical) {
        if !value.is_empty() {
            return Some((value, false));
        }
    }
    if let Ok(value) = env::var(legacy) {
        if !value.is_empty() {
            if legacy == "HARNESS_ROOT" {
                warn_deprecated_override(legacy, "harness space switch");
            } else {
                eprintln!("warning: {legacy} is deprecated; prefer `{canonical}`");
            }
            return Some((value, true));
        }
    }
    None
}

/// Back-compat shim: callers that only need the store root keep working. New code
/// should use [`resolve_store`] to also get the `StoreSource`/`ProjectContext`.
/// Only used by tests today; `run()` calls [`resolve_store`] directly.
#[cfg(test)]
pub(super) fn resolve_store_root(args: &mut Vec<String>) -> PathBuf {
    let command = args.first().cloned();
    resolve_store(args, command.as_deref())
        .expect("resolve store")
        .root
}

/// Walk up from `start` returning the first existing `<dir>/.harness` directory,
/// or `None` if none is found up to the filesystem root.
pub(super) fn discover_harness_from(start: &Path) -> Option<PathBuf> {
    let mut dir = start.to_path_buf();
    loop {
        let candidate = dir.join(".harness");
        if candidate.is_dir() {
            return Some(candidate);
        }
        if !dir.pop() {
            return None;
        }
    }
}

/// Remove the first `--flag <value>` pair from `args`, returning the value. The
/// flag is always removed; the value is returned only when present (a trailing
/// `--flag` with no value yields `None`).
pub(super) fn take_flag_value(args: &mut Vec<String>, flag: &str) -> Option<String> {
    let pos = args.iter().position(|a| a == flag)?;
    args.remove(pos);
    if pos < args.len() {
        Some(args.remove(pos))
    } else {
        None
    }
}

/// Remove a boolean `--flag` from `args`, returning whether it was present.
pub(super) fn take_flag(args: &mut Vec<String>, flag: &str) -> bool {
    if let Some(pos) = args.iter().position(|a| a == flag) {
        args.remove(pos);
        true
    } else {
        false
    }
}

pub(super) fn command_name_for_resolution(args: &[String]) -> Option<String> {
    let mut index = 0usize;
    while index < args.len() {
        match args[index].as_str() {
            "--store" | "--project" | "--company" | "--space" => {
                index += 2;
            }
            "--store-source" => {
                index += 1;
            }
            value if value.starts_with("--") => {
                index += 1;
            }
            value => return Some(value.to_string()),
        }
    }
    None
}
