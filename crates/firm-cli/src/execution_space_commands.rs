//! CLI commands for Execution Space selection and project-store migration.

use super::project_commands::directory_file_index;
use super::*;

pub(super) fn execution_space_json(space: &ExecutionSpace, current: &str) -> serde_json::Value {
    serde_json::json!({
        "id": space.id,
        "name": space.name,
        "store_root": space.store_root.display().to_string(),
        "default_project_binding_id": space.default_project_binding_id,
        "company_id": space.company_id,
        "is_current": space.id == current,
        "identity_boundary": "execution_space",
        "owns": ["mission", "mission_log", "agent_team", "team_run", "member_run", "team_message", "workflow"],
    })
}

pub(super) fn execution_space_command(args: &[String]) -> CliResult<()> {
    require_subcommand(
        args,
        "space init|list|current|switch|show|migrate-from-project",
    )?;
    let firm_home = execution_space::firm_home().map_err(execution_space_err)?;
    match args[0].as_str() {
        "init" => {
            let id = required(args, "--id")?;
            let name = value(args, "--name").unwrap_or_else(|| id.clone());
            let default_project_binding_id = value(args, "--project-binding");
            if let Some(binding) = default_project_binding_id.as_deref() {
                if project::binding_for_id(&firm_home, binding)
                    .map_err(project_err)?
                    .is_none()
                {
                    return Err(CliError::Usage(format!(
                        "unknown project binding: {binding}"
                    )));
                }
            }
            let company_id = value(args, "--company");
            // DOC-108: the Company registry is retired, so `--company` stays
            // only as an inert free-text compatibility label on the Space —
            // it is no longer validated against a registry.
            let context = execution_space::register_and_activate(
                &firm_home,
                &id,
                &name,
                default_project_binding_id,
                company_id,
                &now_string(),
            )
            .map_err(execution_space_err)?;
            HarnessStore::new(context.store_root.clone()).init()?;
            print_json(&execution_space_json(&context, &context.id))
        }
        "list" => {
            let current = execution_space::active_space_id(&firm_home)
                .map_err(execution_space_err)?
                .unwrap_or_default();
            let spaces = execution_space::list_spaces(&firm_home).map_err(execution_space_err)?;
            print_json(
                &spaces
                    .iter()
                    .map(|space| execution_space_json(space, &current))
                    .collect::<Vec<_>>(),
            )
        }
        "current" => {
            let current =
                execution_space::active_space_id(&firm_home).map_err(execution_space_err)?;
            match current {
                Some(id) => match execution_space::context_for_id(&firm_home, &id)
                    .map_err(execution_space_err)?
                {
                    Some(space) => print_json(&execution_space_json(&space, &id)),
                    None => print_json(&serde_json::json!({"id": id, "is_current": true})),
                },
                None => print_json(
                    &serde_json::json!({"id": serde_json::Value::Null, "is_current": false}),
                ),
            }
        }
        "switch" => {
            let id = args
                .iter()
                .skip(1)
                .find(|value| !value.starts_with("--"))
                .ok_or_else(|| CliError::Usage("usage: harness space switch <id>".into()))?;
            let space = execution_space::switch_current_space(&firm_home, id, &now_string())
                .map_err(execution_space_err)?;
            print_json(&execution_space_json(&space, &space.id))
        }
        "show" => {
            let selector = args
                .iter()
                .skip(1)
                .find(|value| !value.starts_with("--"))
                .cloned()
                .or(execution_space::active_space_id(&firm_home).map_err(execution_space_err)?)
                .ok_or_else(|| CliError::Usage("no active execution space".into()))?;
            let current = execution_space::active_space_id(&firm_home)
                .map_err(execution_space_err)?
                .unwrap_or_default();
            let space = execution_space::context_for_id(&firm_home, &selector)
                .map_err(execution_space_err)?
                .ok_or_else(|| CliError::Usage(format!("unknown execution space: {selector}")))?;
            print_json(&execution_space_json(&space, &current))
        }
        "migrate-from-project" => execution_space_migrate_from_project(&firm_home, &args[1..]),
        other => Err(CliError::Usage(format!("unknown space command: {other}"))),
    }
}

pub(super) const EXECUTION_LEDGER_NAMES: &[&str] = &[
    "missions.jsonl",
    "waves.jsonl",
    "teams.jsonl",
    "proposals.jsonl",
    "evidence.jsonl",
    "decisions.jsonl",
    "gaps.jsonl",
    "provider_child_threads.jsonl",
    "team_runs.jsonl",
    "work_operations.jsonl",
    "host_attentions.jsonl",
    "team_supervisor_leases.jsonl",
    "delegation_runs.jsonl",
    "team_run_events.jsonl",
    "agentfirm_trust_operations.jsonl",
    "workflow_runs.jsonl",
    "workflow_steps.jsonl",
    "workflow_patches.jsonl",
    "workflow_artifact_manifests.jsonl",
    "provider_compatibility_admissions.jsonl",
];

pub(super) fn execution_space_migrate_from_project(
    firm_home: &Path,
    args: &[String],
) -> CliResult<()> {
    execution_space_migrate_from_project_with_activate(
        firm_home,
        args,
        |firm_home, lock, id, name, project_binding_id, now| {
            execution_space::register_and_activate_locked(
                firm_home,
                lock,
                id,
                name,
                Some(project_binding_id.to_string()),
                None,
                now,
            )
        },
    )
}

#[derive(Debug)]
struct ExecutionLedgerMigrationPlan {
    name: &'static str,
    source: PathBuf,
    source_bytes: Vec<u8>,
    migration_bytes: Vec<u8>,
    record_count: u64,
    downgraded_bound_reviews: u64,
}

#[derive(Debug)]
struct ExecutionDirectoryMigrationPlan {
    name: &'static str,
    source: PathBuf,
    source_snapshot: Vec<(PathBuf, Vec<u8>)>,
}

fn snapshot_directory_files(root: &Path) -> CliResult<Vec<(PathBuf, Vec<u8>)>> {
    match std::fs::symlink_metadata(root) {
        Ok(metadata) if metadata.file_type().is_dir() => {}
        Ok(_) => {
            return Err(CliError::Usage(format!(
                "execution migration refuses symbolic links or special files: {}",
                root.display()
            )))
        }
        Err(error) => return Err(error.into()),
    }
    directory_file_index(root)?
        .into_iter()
        .map(|relative| {
            let bytes = std::fs::read(root.join(&relative))?;
            Ok((relative, bytes))
        })
        .collect()
}

fn ensure_real_migration_source_ancestors(store_root: &Path, source: &Path) -> CliResult<()> {
    let relative = source.strip_prefix(store_root).map_err(|_| {
        CliError::Usage(format!(
            "execution migration source escapes its store root: {}",
            source.display()
        ))
    })?;
    match std::fs::symlink_metadata(store_root) {
        Ok(metadata) if metadata.file_type().is_dir() => {}
        Ok(_) => {
            return Err(CliError::Usage(format!(
                "execution migration refuses symbolic links or special source ancestors: {}",
                store_root.display()
            )))
        }
        Err(error) => return Err(error.into()),
    }
    let mut current = store_root.to_path_buf();
    let parent = relative.parent().unwrap_or_else(|| Path::new(""));
    for component in parent.components() {
        current.push(component.as_os_str());
        match std::fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_dir() => {}
            Ok(_) => {
                return Err(CliError::Usage(format!(
                    "execution migration refuses symbolic links or special source ancestors: {}",
                    current.display()
                )))
            }
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

fn copy_dir_recursive_strict(src: &Path, dst: &Path) -> CliResult<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let path = entry.path();
        let target = dst.join(entry.file_name());
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            copy_dir_recursive_strict(&path, &target)?;
        } else if file_type.is_file() {
            std::fs::copy(&path, &target)?;
        } else {
            return Err(CliError::Usage(format!(
                "execution migration refuses symbolic links or special files: {}",
                path.display()
            )));
        }
    }
    Ok(())
}

fn verify_directory_snapshot(root: &Path, expected: &[(PathBuf, Vec<u8>)]) -> CliResult<bool> {
    Ok(snapshot_directory_files(root)? == expected)
}

fn migration_error_after_staging_cleanup(
    error: impl std::fmt::Display,
    staging: &Path,
) -> CliError {
    let cleanup_error = if staging.exists() {
        std::fs::remove_dir_all(staging).err()
    } else {
        None
    };
    CliError::Usage(match cleanup_error {
        Some(cleanup_error) => format!(
            "{error}; staging cleanup also failed at {}: {cleanup_error}",
            staging.display()
        ),
        None => error.to_string(),
    })
}

pub(super) fn execution_space_migrate_from_project_with_activate<F>(
    firm_home: &Path,
    args: &[String],
    activate: F,
) -> CliResult<()>
where
    F: FnOnce(
        &Path,
        &execution_space::ExecutionSpaceRegistryLock,
        &str,
        &str,
        &str,
        &str,
    ) -> execution_space::ExecutionSpaceResult<ExecutionSpace>,
{
    execution_space_migrate_from_project_with_hooks(firm_home, args, || Ok(()), activate)
}

pub(super) fn execution_space_migrate_from_project_with_hooks<F, G>(
    firm_home: &Path,
    args: &[String],
    before_source_verification: G,
    activate: F,
) -> CliResult<()>
where
    F: FnOnce(
        &Path,
        &execution_space::ExecutionSpaceRegistryLock,
        &str,
        &str,
        &str,
        &str,
    ) -> execution_space::ExecutionSpaceResult<ExecutionSpace>,
    G: FnOnce() -> CliResult<()>,
{
    execution_space_migrate_from_project_with_publish_hook(
        firm_home,
        args,
        before_source_verification,
        || Ok(()),
        activate,
    )
}

pub(super) fn execution_space_migrate_from_project_with_publish_hook<F, G, H>(
    firm_home: &Path,
    args: &[String],
    before_source_verification: G,
    before_publish: H,
    activate: F,
) -> CliResult<()>
where
    F: FnOnce(
        &Path,
        &execution_space::ExecutionSpaceRegistryLock,
        &str,
        &str,
        &str,
        &str,
    ) -> execution_space::ExecutionSpaceResult<ExecutionSpace>,
    G: FnOnce() -> CliResult<()>,
    H: FnOnce() -> CliResult<()>,
{
    if has_flag(args, "--force") {
        return Err(CliError::Usage(
            "--force is retired for execution-space migration; choose a new --id because an existing target is never replaced".into(),
        ));
    }
    let project_selector = required(args, "--from-project")?;
    let id = required(args, "--id")?;
    execution_space::validate_space_id(&id).map_err(execution_space_err)?;
    let name = value(args, "--name").unwrap_or_else(|| id.clone());
    let project_context = resolve_project_selector(firm_home, &project_selector)
        .ok_or_else(|| CliError::Usage(format!("unknown project binding: {project_selector}")))?;
    let target = execution_space::space_store_root(firm_home, &id);
    match std::fs::symlink_metadata(&target) {
        Ok(_) => {
            return Err(CliError::Usage(format!(
                "target execution space already exists: {}; choose a new --id",
                target.display()
            )))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }

    ensure_real_migration_source_ancestors(
        &project_context.store_root,
        &project_context.store_root,
    )?;
    let source_store = HarnessStore::new(project_context.store_root.clone());
    // Keep the ordinary source-store writer lock from the first source read
    // through publication. Do not call any source Store writer while this
    // guard is alive: those APIs acquire this same lock themselves.
    let source_guard = source_store.acquire_exclusive_migration_guard()?;

    // Complete every source read and typed transformation before staging.
    let mut ledger_plans = Vec::new();
    for ledger in EXECUTION_LEDGER_NAMES {
        let source = project_context.store_root.join(ledger);
        ensure_real_migration_source_ancestors(&project_context.store_root, &source)?;
        match std::fs::symlink_metadata(&source) {
            Ok(metadata) if metadata.file_type().is_file() => {}
            Ok(_) => {
                return Err(CliError::Usage(format!(
                    "execution migration refuses symbolic links or special files: {}",
                    source.display()
                )))
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error.into()),
        }
        let source_bytes = std::fs::read(&source)?;
        let (migration_bytes, downgraded_bound_reviews) =
            prepare_execution_ledger_for_migration(ledger, &source_bytes)?;
        ledger_plans.push(ExecutionLedgerMigrationPlan {
            name: ledger,
            source,
            record_count: source_bytes
                .split(|byte| *byte == b'\n')
                .filter(|line| line.iter().any(|byte| !byte.is_ascii_whitespace()))
                .count() as u64,
            source_bytes,
            migration_bytes,
            downgraded_bound_reviews,
        });
    }

    let mut directory_plans = Vec::new();
    for directory in ["checks", "compiled", "workflow-patches"] {
        let source = project_context.store_root.join(directory);
        ensure_real_migration_source_ancestors(&project_context.store_root, &source)?;
        match std::fs::symlink_metadata(&source) {
            Ok(metadata) if metadata.file_type().is_dir() => {
                let source_snapshot = snapshot_directory_files(&source)?;
                directory_plans.push(ExecutionDirectoryMigrationPlan {
                    name: directory,
                    source,
                    source_snapshot,
                });
            }
            Ok(_) => {
                return Err(CliError::Usage(format!(
                    "execution migration refuses symbolic links or special files: {}",
                    source.display()
                )))
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }

    let spaces_parent = execution_space::spaces_dir(firm_home);
    std::fs::create_dir_all(&spaces_parent)?;
    let transaction_id = generated_id("migration");
    let staging = spaces_parent.join(format!(".{id}.{transaction_id}.staging"));
    if let Err(error) = std::fs::create_dir(&staging) {
        return Err(migration_error_after_staging_cleanup(error, &staging));
    }

    let stage_result = (|| -> CliResult<(ExecutionSpace, serde_json::Value)> {
        for plan in &ledger_plans {
            std::fs::write(staging.join(plan.name), &plan.migration_bytes)?;
        }
        for plan in &directory_plans {
            ensure_real_migration_source_ancestors(&project_context.store_root, &plan.source)?;
            copy_dir_recursive_strict(&plan.source, &staging.join(plan.name))?;
        }

        let staged_context = ExecutionSpace {
            id: id.clone(),
            name: name.clone(),
            store_root: staging.clone(),
            default_project_binding_id: Some(project_context.id.clone()),
            company_id: None,
        };
        execution_space::write_metadata(&staged_context).map_err(execution_space_err)?;
        HarnessStore::new(staging.clone()).init()?;
        before_source_verification()?;
        ensure_real_migration_source_ancestors(
            &project_context.store_root,
            &project_context.store_root,
        )?;

        // Re-read source bytes after staging. This detects a source that changed
        // during the migration instead of verifying a stale in-memory plan.
        for plan in &ledger_plans {
            ensure_real_migration_source_ancestors(&project_context.store_root, &plan.source)?;
            match std::fs::symlink_metadata(&plan.source) {
                Ok(metadata) if metadata.file_type().is_file() => {}
                Ok(_) => {
                    return Err(CliError::Usage(format!(
                    "execution migration refuses symbolic links or special files after staging: {}",
                    plan.source.display()
                )))
                }
                Err(error) => return Err(error.into()),
            }
            let current_source = std::fs::read(&plan.source)?;
            if current_source != plan.source_bytes {
                return Err(CliError::Usage(format!(
                    "execution migration source changed while staging: {}",
                    plan.source.display()
                )));
            }
            let (expected_bytes, _) =
                prepare_execution_ledger_for_migration(plan.name, &current_source)?;
            if expected_bytes != std::fs::read(staging.join(plan.name))? {
                return Err(CliError::Usage(format!(
                    "execution migration verification failed for {}",
                    plan.name
                )));
            }
        }
        for plan in &directory_plans {
            ensure_real_migration_source_ancestors(&project_context.store_root, &plan.source)?;
            if !verify_directory_snapshot(&plan.source, &plan.source_snapshot)? {
                return Err(CliError::Usage(format!(
                    "execution migration source changed while staging: {}/",
                    plan.source.display()
                )));
            }
            if !verify_directory_snapshot(&staging.join(plan.name), &plan.source_snapshot)? {
                return Err(CliError::Usage(format!(
                    "execution migration verification failed for {}/",
                    plan.name
                )));
            }
        }

        let copied_files = ledger_plans.len() as u64
            + directory_plans
                .iter()
                .map(|plan| plan.source_snapshot.len() as u64)
                .sum::<u64>();
        let copied_records = ledger_plans
            .iter()
            .map(|plan| plan.record_count)
            .sum::<u64>();
        let downgraded_bound_reviews = ledger_plans
            .iter()
            .map(|plan| plan.downgraded_bound_reviews)
            .sum::<u64>();
        let final_context = ExecutionSpace {
            store_root: target.clone(),
            ..staged_context
        };
        let manifest = serde_json::json!({
            "kind": "project_execution_store_to_execution_space",
            "source_project_binding_id": project_context.id,
            "source_store_root": project_context.store_root.display().to_string(),
            "target_space_id": final_context.id,
            "target_store_root": final_context.store_root.display().to_string(),
            "copied_files": copied_files,
            "copied_records": copied_records,
            "verified_records": copied_records,
            "downgraded_bound_reviews": downgraded_bound_reviews,
            "excluded_prefixes": ["company_os_", "provider-sessions", "runtimes"],
            "source_retained": true,
            "registration": {
                "status": "pending",
                "recovery_command": format!("harness space switch {id}"),
            },
            "created_at": now_string(),
        });
        std::fs::write(
            staging.join("execution_space_migration.json"),
            serde_json::to_string_pretty(&manifest)?,
        )?;
        Ok((final_context, manifest))
    })();

    let (planned_context, manifest) = match stage_result {
        Ok(result) => result,
        Err(error) => {
            return Err(migration_error_after_staging_cleanup(error, &staging));
        }
    };

    // Serialize publication and registration/activation as one same-id critical
    // section. The published target remains independently recoverable if a later
    // registry or ACTIVE_SPACE write fails, but no competing creator can claim
    // the id between rename and registration.
    let publish_lock = execution_space::acquire_registry_lock(firm_home)
        .map_err(execution_space_err)
        .map_err(|error| migration_error_after_staging_cleanup(error, &staging))?;
    let registry = execution_space::ExecutionSpaceRegistry::load(firm_home)
        .map_err(execution_space_err)
        .map_err(|error| migration_error_after_staging_cleanup(error, &staging))?;
    if registry.find(&id).is_some() {
        return Err(migration_error_after_staging_cleanup(
            format!("execution space id is already registered: {id}; choose a new --id"),
            &staging,
        ));
    }
    match std::fs::symlink_metadata(&target) {
        Ok(_) => {
            return Err(migration_error_after_staging_cleanup(
                format!(
                    "target execution space appeared while staging: {}; choose a new --id",
                    target.display()
                ),
                &staging,
            ))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(migration_error_after_staging_cleanup(error, &staging));
        }
    }
    if let Err(error) = before_publish() {
        return Err(migration_error_after_staging_cleanup(error, &staging));
    }
    if let Err(error) = execution_space::publish_directory_no_replace(&staging, &target) {
        let message = if target.exists() {
            format!(
                "target execution space appeared while publishing: {}; choose a new --id",
                target.display()
            )
        } else {
            format!("could not publish staged execution space: {error}")
        };
        return Err(migration_error_after_staging_cleanup(message, &staging));
    }
    drop(source_guard);

    let activation_now = now_string();
    let context = match activate(
        firm_home,
        &publish_lock,
        &id,
        &name,
        &project_context.id,
        &activation_now,
    ) {
        Ok(context) => context,
        Err(error) => {
            return Err(CliError::Usage(format!(
                "execution-space migration was published and verified at {}, but registration/activation failed: {error}; the target was retained; recover with `harness space switch {id}`",
                target.display()
            )));
        }
    };
    drop(publish_lock);

    debug_assert_eq!(context.id, planned_context.id);
    debug_assert_eq!(context.store_root, planned_context.store_root);
    let manifest_path = target.join("execution_space_migration.json");
    let manifest = std::fs::read(&manifest_path)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or(manifest);
    print_json(&serde_json::json!({
        "space": execution_space_json(&context, &context.id),
        "migration": manifest,
    }))
}

/// Historical project Review rows did not pass through the trusted Work-review
/// writer. Preserve them as readable evidence, but strip Work binding before
/// they enter an active execution space so raw ledger bytes cannot satisfy a
/// code-review gate.
fn prepare_execution_ledger_for_migration(
    ledger: &str,
    source_bytes: &[u8],
) -> CliResult<(Vec<u8>, u64)> {
    if ledger != "reviews.jsonl" {
        return Ok((source_bytes.to_vec(), 0));
    }

    let source = std::str::from_utf8(source_bytes)
        .map_err(|error| CliError::Usage(format!("reviews.jsonl is not valid UTF-8: {error}")))?;
    let mut output = Vec::with_capacity(source_bytes.len());
    let mut downgraded = 0u64;
    let mut review_ids = HashSet::new();
    for segment in source.split_inclusive('\n') {
        let (line, newline) = segment
            .strip_suffix('\n')
            .map_or((segment, ""), |line| (line, "\n"));
        if line.trim().is_empty() {
            output.extend_from_slice(segment.as_bytes());
            continue;
        }
        let mut value: serde_json::Value = serde_json::from_str(line.trim())?;
        let source_review: Review = serde_json::from_value(value.clone()).map_err(|error| {
            CliError::Usage(format!("reviews.jsonl row is not a valid Review: {error}"))
        })?;
        source_review.validate().map_err(|error| {
            CliError::Usage(format!(
                "reviews.jsonl row fails Review validation: {error}"
            ))
        })?;
        if !review_ids.insert(source_review.id.clone()) {
            return Err(CliError::Usage(format!(
                "reviews.jsonl contains duplicate Review id `{}`; Review ids must be globally unique",
                source_review.id
            )));
        }
        let object = value
            .as_object_mut()
            .ok_or_else(|| CliError::Usage("reviews.jsonl rows must be JSON objects".into()))?;
        let mut had_binding = false;
        for field in [
            "reviewed_work_id",
            "reviewed_work_version",
            "review_strategy",
            "command_idempotency_key",
        ] {
            had_binding |= object.remove(field).is_some();
        }
        let review: Review = serde_json::from_value(value.clone()).map_err(|error| {
            CliError::Usage(format!(
                "reviews.jsonl row is not a valid Review after migration downgrade: {error}"
            ))
        })?;
        review.validate().map_err(|error| {
            CliError::Usage(format!(
                "reviews.jsonl row fails Review validation after migration downgrade: {error}"
            ))
        })?;
        if had_binding {
            downgraded += 1;
            serde_json::to_writer(&mut output, &value)?;
            output.extend_from_slice(newline.as_bytes());
        } else {
            output.extend_from_slice(segment.as_bytes());
        }
    }
    Ok((output, downgraded))
}
