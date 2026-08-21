use super::*;
use harness_core::{LaunchMcpServer, WorkflowStepStatus};

fn temp_store(tag: &str) -> HarnessStore {
    let root = std::env::temp_dir().join(format!("harness-wf-test-{}", generated_id(tag)));
    let store = HarnessStore::new(&root);
    store.init().expect("init store");
    store
}

fn new_file_diff_str(path: &str, content: &str) -> String {
    format!(
            "diff --git a/{path} b/{path}\nnew file mode 100644\nindex 0000000..1111111\n--- /dev/null\n+++ b/{path}\n@@ -0,0 +1 @@\n+{content}\n"
        )
}

fn init_gc_git_project(tag: &str, store: &HarnessStore) -> PathBuf {
    let project_root =
        std::env::temp_dir().join(format!("harness-gc-project-{}", generated_id(tag)));
    std::fs::create_dir_all(&project_root).expect("mk gc project root");
    let git = |args: &[&str]| {
        Command::new("git")
            .arg("-C")
            .arg(&project_root)
            .args(args)
            .output()
            .expect("git")
    };
    assert!(git(&["init"]).status.success(), "git init");
    let _ = git(&["config", "user.email", "t@t"]);
    let _ = git(&["config", "user.name", "t"]);
    std::fs::write(project_root.join("README"), "x").expect("seed file");
    assert!(git(&["add", "-A"]).status.success(), "git add");
    assert!(
        git(&["commit", "-m", "init"]).status.success(),
        "git commit"
    );
    let ctx = ProjectContext {
        id: format!("gc-{}", generated_id(tag)),
        project_root: project_root.clone(),
        store_root: store.root().to_path_buf(),
        kind: ProjectKind::Repo,
        is_git_repo: true,
    };
    project::write_metadata(&ctx, None).expect("write gc project metadata");
    project_root
}

fn seed_gc_workflow_run(store: &HarnessStore, id: &str, status: WorkflowRunStatus) {
    store
        .append_workflow_run(&WorkflowRun {
            id: id.into(),
            workflow_name: "gc-demo".into(),
            project_binding_id: None,
            status,
            step_ids: Vec::new(),
            created_at: "unix-ms:1".into(),
            ended_at: if status == WorkflowRunStatus::Running {
                None
            } else {
                Some("unix-ms:2".into())
            },
            summary: None,
            args: None,
            agents_spawned: 0,
            final_output: None,
            initiated_by: Some("test".into()),
            design_intent: None,
            spec: None,
            host_pid: None,
            dry_run: false,
            terminal_reason: None,
            partial_output_available: false,
        })
        .expect("append gc run");
}

fn add_registered_gc_worktree(
    project_root: &Path,
    run_id: &str,
    label: &str,
    session_id: &str,
) -> PathBuf {
    let (rel, branch) = worktree_paths(run_id, label, session_id);
    let path = project_root.join(rel);
    let output = Command::new("git")
        .arg("-C")
        .arg(project_root)
        .args(["worktree", "add", "-B", &branch])
        .arg(&path)
        .arg("HEAD")
        .output()
        .expect("git worktree add");
    assert!(
        output.status.success(),
        "git worktree add failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    path
}

/// A throwaway [`ProjectContext`] for tests that build a `WorkflowDeliveryOptions`
/// directly (goal-multi-project): `project_root` is a fresh temp dir distinct
/// from the store, so worker-cwd assertions are unambiguous. Not a git repo by
/// default; pass `is_git_repo` per the case under test.
fn temp_project_context(tag: &str, is_git_repo: bool) -> ProjectContext {
    let project_root = std::env::temp_dir().join(format!("harness-wf-proj-{}", generated_id(tag)));
    std::fs::create_dir_all(&project_root).expect("mk project root");
    ProjectContext {
        id: "_global".to_string(),
        store_root: project_root.join(".store"),
        project_root,
        kind: ProjectKind::Repo,
        is_git_repo,
    }
}

fn launch_spec_with_model_effort(model: Option<&str>, effort: Option<&str>) -> LaunchSpec {
    LaunchSpec {
        prompt_ref: None,
        message_content: "hello".into(),
        model: model.map(str::to_string),
        effort: effort.map(str::to_string),
        output_schema: None,
        permission: LaunchPermission::WorkspaceWrite,
        writable_roots: Vec::new(),
        tools: Vec::new(),
        workspace: None,
        mcp: None,
        skill_refs: Vec::new(),
        resume: None,
        output: None,
    }
}

fn launch_spec_with_mcp(mcp: Option<LaunchMcp>) -> LaunchSpec {
    let mut spec = launch_spec_with_model_effort(None, None);
    spec.mcp = mcp;
    spec
}

fn mcp_stdio_server(id: &str, command: &[&str]) -> LaunchMcpServer {
    LaunchMcpServer {
        id: id.to_string(),
        transport: Some("stdio".to_string()),
        command: command.iter().map(|part| part.to_string()).collect(),
        url: None,
        allowed_tools: Vec::new(),
    }
}

fn mcp_http_server(id: &str, url: &str) -> LaunchMcpServer {
    LaunchMcpServer {
        id: id.to_string(),
        transport: Some("http".to_string()),
        command: Vec::new(),
        url: Some(url.to_string()),
        allowed_tools: Vec::new(),
    }
}

fn command_args(cmd: &Command) -> Vec<String> {
    cmd.get_args()
        .map(|arg| arg.to_string_lossy().to_string())
        .collect()
}

fn ok_step(spec: &workflow::AgentStepSpec) -> workflow::StepResult {
    workflow::StepResult {
        phase: spec.phase.clone(),
        label: spec.label.clone(),
        provider: spec.provider.clone(),
        isolation: spec.isolation.clone(),
        ok: true,
        output_summary: format!("mock ok: {}", spec.label),
        step_id: None,
        started_at: None,
        details: None,
        structured: None,
        ordinal: None,
    }
}

fn ndjson_values(lines: &[&str]) -> Vec<serde_json::Value> {
    lines
        .iter()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .collect()
}

fn delivery_outcome_for_test(
    tokens: Option<TokenUsage>,
    cost_usd: Option<f64>,
    model: Option<String>,
    structured: Option<serde_json::Value>,
) -> DeliveryOutcome {
    DeliveryOutcome {
        status: ProviderExecutionStatus::Succeeded,
        native_session: None,
        provider_thread_id: None,
        provider_turn_id: None,
        terminal_source: Some(MessageTerminalSource::Unknown),
        provider_request_id: None,
        exit_code: Some(0),
        tokens,
        cost_usd,
        model,
        structured,
        response_text: None,
        summary: "test delivery".into(),
    }
}

fn spawn_sleep_process_group() -> std::process::Child {
    let mut cmd = Command::new("sh");
    cmd.arg("-c").arg("sleep 30");
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    cmd.spawn().expect("spawn sleep worker")
}

fn kill_test_process_group(child: &mut std::process::Child) {
    #[cfg(unix)]
    unsafe {
        libc::kill(-(child.id() as libc::pid_t), libc::SIGKILL);
    }
    let _ = child.kill();
    let _ = child.wait();
}

fn wait_for_child_exit(child: &mut std::process::Child) {
    for _ in 0..50 {
        if child.try_wait().expect("try_wait").is_some() {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!("child did not exit after reaper kill");
}

fn write_test_worker_pidfile(
    store: &HarnessStore,
    run_id: &str,
    pid: u32,
    cmd_marker: &str,
) -> PathBuf {
    write_test_worker_pidfile_with_started_ms(store, run_id, pid, cmd_marker, current_unix_ms())
}

fn write_test_worker_pidfile_with_started_ms(
    store: &HarnessStore,
    run_id: &str,
    pid: u32,
    cmd_marker: &str,
    started_ms: u128,
) -> PathBuf {
    let dir = worker_pid_dir(store);
    fs::create_dir_all(&dir).expect("mkdir worker_pids");
    let path = dir.join(format!("{run_id}__{pid}.json"));
    let entry = OrphanPidfile {
        run_id: run_id.to_string(),
        pid,
        pgid: pid,
        cmd_marker: cmd_marker.to_string(),
        started_ms,
    };
    fs::write(
        &path,
        serde_json::to_vec(&entry).expect("serialize pidfile"),
    )
    .expect("write pidfile");
    path
}

fn append_test_workflow_run(
    store: &HarnessStore,
    id: &str,
    status: WorkflowRunStatus,
    host_pid: Option<u32>,
) {
    store
        .append_workflow_run(&WorkflowRun {
            id: id.into(),
            workflow_name: "demo".into(),
            project_binding_id: None,
            status,
            step_ids: vec![],
            created_at: now_string(),
            ended_at: None,
            summary: None,
            args: None,
            agents_spawned: 0,
            final_output: None,
            initiated_by: Some("op".into()),
            design_intent: None,
            spec: None,
            host_pid,
            dry_run: false,
            terminal_reason: None,
            partial_output_available: false,
        })
        .expect("append run");
}

// ---- goal-multi-project: workflow-cwd phase ---------------------------------

/// A throwaway spec for the cwd/worktree/policy tests below.
fn cwd_test_spec(label: &str, writable: bool, isolation: Option<&str>) -> workflow::AgentStepSpec {
    workflow::AgentStepSpec {
        phase: "p".into(),
        label: label.into(),
        provider: "claude".into(),
        model: None,
        effort: None,
        service_tier: None,
        fallback_model: None,
        timeout_s: None,
        image: Vec::new(),
        add_dir: Vec::new(),
        expected_artifacts: Vec::new(),
        persist_changes: None,
        write_mode: None,
        owned_paths: Vec::new(),
        artifact_root: None,
        write_roots: Vec::new(),
        auto_apply_on_verdict: false,
        isolation: isolation.map(str::to_string),
        prompt: "noop".into(),
        schema: None,
        schema_strict: false,
        writable,
        ordinal: Some(0),
    }
}

// D3a: a SUCCESSFUL writable standalone step still persists its patch (the
// positive control for the gate above).
// D4/D5 unit tests: the robust changed-path enumeration and binary-safe capture.
// D4b: owned_paths is enforced against `git apply --numstat` (the paths git
// actually touches), so a crafted `diff --git` header that names an in-bounds
// path but whose hunk edits an OUT-OF-BOUNDS file is caught.

#[path = "workflow_runtime_tests/apply_enforces_owned_paths_via_numstat_not_headers.rs"]
mod apply_enforces_owned_paths_via_numstat_not_headers;
#[path = "workflow_runtime_tests/artifact_manifest_marks_missing_and_stale_outputs.rs"]
mod artifact_manifest_marks_missing_and_stale_outputs;
#[path = "workflow_runtime_tests/build_step_details_caps_large_worktree_diff.rs"]
mod build_step_details_caps_large_worktree_diff;
#[path = "workflow_runtime_tests/build_step_details_failure_classifies_and_keeps_stderr.rs"]
mod build_step_details_failure_classifies_and_keeps_stderr;
#[path = "workflow_runtime_tests/build_step_details_success_has_tokens_and_no_failure.rs"]
mod build_step_details_success_has_tokens_and_no_failure;
#[path = "workflow_runtime_tests/classify_failure_reason_clean_exit_but_failed_is_delivery.rs"]
mod classify_failure_reason_clean_exit_but_failed_is_delivery;
#[path = "workflow_runtime_tests/classify_failure_reason_nonzero_exit_is_exit.rs"]
mod classify_failure_reason_nonzero_exit_is_exit;
#[path = "workflow_runtime_tests/classify_failure_reason_ok_is_none.rs"]
mod classify_failure_reason_ok_is_none;
#[path = "workflow_runtime_tests/classify_failure_reason_timeout_dominates.rs"]
mod classify_failure_reason_timeout_dominates;
#[path = "workflow_runtime_tests/count_unique_worktree_diff_files_counts_headers_once.rs"]
mod count_unique_worktree_diff_files_counts_headers_once;
#[path = "workflow_runtime_tests/dashboard_snapshot_hides_legacy_durable_thinking_rows.rs"]
mod dashboard_snapshot_hides_legacy_durable_thinking_rows;
#[path = "workflow_runtime_tests/dashboard_snapshot_includes_workflow_keys.rs"]
mod dashboard_snapshot_includes_workflow_keys;
#[path = "workflow_runtime_tests/dashboard_snapshot_still_fails_closed_on_unbacked_dangling_team_reference.rs"]
mod dashboard_snapshot_still_fails_closed_on_unbacked_dangling_team_reference;
#[path = "workflow_runtime_tests/dashboard_snapshot_tolerates_pre_cutover_dangling_team_reference.rs"]
mod dashboard_snapshot_tolerates_pre_cutover_dangling_team_reference;
#[path = "workflow_runtime_tests/delivery_outcome_defaults_have_no_telemetry_for_non_provider_paths.rs"]
mod delivery_outcome_defaults_have_no_telemetry_for_non_provider_paths;
#[path = "workflow_runtime_tests/direct_write_diff_captures_shared_repo_changes_without_index_side_effects.rs"]
mod direct_write_diff_captures_shared_repo_changes_without_index_side_effects;
#[path = "workflow_runtime_tests/direct_write_mode_requires_writable_clean_git_project.rs"]
mod direct_write_mode_requires_writable_clean_git_project;
#[path = "workflow_runtime_tests/discarded_worktree_diff_warning_names_run_step_and_recovery.rs"]
mod discarded_worktree_diff_warning_names_run_step_and_recovery;
#[path = "workflow_runtime_tests/discover_harness_from_finds_the_nearest_ancestor_dot_harness.rs"]
mod discover_harness_from_finds_the_nearest_ancestor_dot_harness;
#[path = "workflow_runtime_tests/driver_journaled_running_row_is_reused_for_terminal_row.rs"]
mod driver_journaled_running_row_is_reused_for_terminal_row;
#[path = "workflow_runtime_tests/ephemeral_codex_omits_service_tier_when_absent.rs"]
mod ephemeral_codex_omits_service_tier_when_absent;
#[path = "workflow_runtime_tests/ephemeral_codex_service_tier_arg_is_a_config_override.rs"]
mod ephemeral_codex_service_tier_arg_is_a_config_override;
#[path = "workflow_runtime_tests/expected_artifact_is_copied_from_worker_cwd_to_live_repo.rs"]
mod expected_artifact_is_copied_from_worker_cwd_to_live_repo;
#[path = "workflow_runtime_tests/extract_json_object_handles_bare_object.rs"]
mod extract_json_object_handles_bare_object;
#[path = "workflow_runtime_tests/extract_json_object_rejects_invalid_or_non_object.rs"]
mod extract_json_object_rejects_invalid_or_non_object;
#[path = "workflow_runtime_tests/extract_json_object_strips_a_json_code_fence.rs"]
mod extract_json_object_strips_a_json_code_fence;
#[path = "workflow_runtime_tests/extract_json_object_takes_first_balanced_object_amid_prose.rs"]
mod extract_json_object_takes_first_balanced_object_amid_prose;
#[path = "workflow_runtime_tests/gc_worktrees_keeps_registered_worktree_for_running_run.rs"]
mod gc_worktrees_keeps_registered_worktree_for_running_run;
#[path = "workflow_runtime_tests/gc_worktrees_removes_registered_worktrees_for_terminal_or_absent_runs.rs"]
mod gc_worktrees_removes_registered_worktrees_for_terminal_or_absent_runs;
#[path = "workflow_runtime_tests/kimi_parsers_match_the_real_v018_stream_shape.rs"]
mod kimi_parsers_match_the_real_v018_stream_shape;
#[path = "workflow_runtime_tests/kimi_reply_handles_array_content_and_multiple_assistant_frames.rs"]
mod kimi_reply_handles_array_content_and_multiple_assistant_frames;
#[path = "workflow_runtime_tests/kimi_status_failed_on_nonzero_exit_and_stale_when_empty.rs"]
mod kimi_status_failed_on_nonzero_exit_and_stale_when_empty;
#[path = "workflow_runtime_tests/missing_or_empty_expected_artifact_is_actionable_failure.rs"]
mod missing_or_empty_expected_artifact_is_actionable_failure;
#[path = "workflow_runtime_tests/object_has_required_keys_present_and_missing.rs"]
mod object_has_required_keys_present_and_missing;
#[path = "workflow_runtime_tests/parse_claude_result_extras_reads_structured_and_cost.rs"]
mod parse_claude_result_extras_reads_structured_and_cost;
#[path = "workflow_runtime_tests/parse_claude_usage_absent_is_none.rs"]
mod parse_claude_usage_absent_is_none;
#[path = "workflow_runtime_tests/parse_claude_usage_reads_result_usage.rs"]
mod parse_claude_usage_reads_result_usage;
#[path = "workflow_runtime_tests/parse_codex_usage_absent_is_none.rs"]
mod parse_codex_usage_absent_is_none;
#[path = "workflow_runtime_tests/parse_codex_usage_accepts_nested_turn_usage_and_legacy_name.rs"]
mod parse_codex_usage_accepts_nested_turn_usage_and_legacy_name;
#[path = "workflow_runtime_tests/parse_codex_usage_reads_turn_completed_usage.rs"]
mod parse_codex_usage_reads_turn_completed_usage;
#[path = "workflow_runtime_tests/parse_name_status_z_records_both_rename_sides_and_cjk_paths.rs"]
mod parse_name_status_z_records_both_rename_sides_and_cjk_paths;
#[path = "workflow_runtime_tests/parse_numstat_z_handles_counts_paths_and_cjk.rs"]
mod parse_numstat_z_handles_counts_paths_and_cjk;
#[path = "workflow_runtime_tests/parse_ps_etime_ms_accepts_common_ps_formats.rs"]
mod parse_ps_etime_ms_accepts_common_ps_formats;
#[path = "workflow_runtime_tests/parse_worker_model_reads_claude_init_and_ignores_codex.rs"]
mod parse_worker_model_reads_claude_init_and_ignores_codex;
#[path = "workflow_runtime_tests/persistent_claude_delivery_outcome_uses_raw_event_tokens_model_and_cost.rs"]
mod persistent_claude_delivery_outcome_uses_raw_event_tokens_model_and_cost;
#[path = "workflow_runtime_tests/persistent_claude_delivery_outcome_uses_result_structured_output.rs"]
mod persistent_claude_delivery_outcome_uses_result_structured_output;
#[path = "workflow_runtime_tests/persistent_claude_effort_arg_matches_ephemeral_mapping.rs"]
mod persistent_claude_effort_arg_matches_ephemeral_mapping;
#[path = "workflow_runtime_tests/persistent_claude_omits_effort_arg_when_absent.rs"]
mod persistent_claude_omits_effort_arg_when_absent;
#[path = "workflow_runtime_tests/persistent_claude_omits_schema_arg_when_absent.rs"]
mod persistent_claude_omits_schema_arg_when_absent;
#[path = "workflow_runtime_tests/persistent_claude_schema_arg_matches_ephemeral_mapping.rs"]
mod persistent_claude_schema_arg_matches_ephemeral_mapping;
#[path = "workflow_runtime_tests/persistent_codex_delivery_outcome_extracts_structured_only_with_schema.rs"]
mod persistent_codex_delivery_outcome_extracts_structured_only_with_schema;
#[path = "workflow_runtime_tests/persistent_codex_delivery_outcome_uses_raw_event_tokens_and_spec_model.rs"]
mod persistent_codex_delivery_outcome_uses_raw_event_tokens_and_spec_model;
#[path = "workflow_runtime_tests/persistent_codex_effort_arg_matches_ephemeral_mapping.rs"]
mod persistent_codex_effort_arg_matches_ephemeral_mapping;
#[path = "workflow_runtime_tests/persistent_codex_mcp_absent_or_empty_emits_no_config_flags.rs"]
mod persistent_codex_mcp_absent_or_empty_emits_no_config_flags;
#[path = "workflow_runtime_tests/persistent_codex_mcp_http_url_matches_config_schema.rs"]
mod persistent_codex_mcp_http_url_matches_config_schema;
#[path = "workflow_runtime_tests/persistent_codex_mcp_quotes_non_bare_id_key_path.rs"]
mod persistent_codex_mcp_quotes_non_bare_id_key_path;
#[path = "workflow_runtime_tests/persistent_codex_mcp_single_command_omits_args.rs"]
mod persistent_codex_mcp_single_command_omits_args;
#[path = "workflow_runtime_tests/persistent_codex_mcp_stdio_command_and_args_match_config_schema.rs"]
mod persistent_codex_mcp_stdio_command_and_args_match_config_schema;
#[path = "workflow_runtime_tests/persistent_codex_omits_effort_arg_when_absent.rs"]
mod persistent_codex_omits_effort_arg_when_absent;
#[path = "workflow_runtime_tests/persistent_codex_omits_schema_arg_when_absent.rs"]
mod persistent_codex_omits_schema_arg_when_absent;
#[path = "workflow_runtime_tests/persistent_codex_schema_arg_matches_ephemeral_mapping.rs"]
mod persistent_codex_schema_arg_matches_ephemeral_mapping;
#[path = "workflow_runtime_tests/provider_adapter_capabilities_return_codex_and_claude_presets.rs"]
mod provider_adapter_capabilities_return_codex_and_claude_presets;
#[path = "workflow_runtime_tests/read_only_leaf_stays_shared_cwd_regardless_of_provider_enforcement.rs"]
mod read_only_leaf_stays_shared_cwd_regardless_of_provider_enforcement;
#[path = "workflow_runtime_tests/reap_finalizes_runs_whose_host_process_is_dead_regardless_of_age.rs"]
mod reap_finalizes_runs_whose_host_process_is_dead_regardless_of_age;
#[path = "workflow_runtime_tests/reap_orphaned_workers_kills_live_process_for_absent_run.rs"]
mod reap_orphaned_workers_kills_live_process_for_absent_run;
#[path = "workflow_runtime_tests/reap_orphaned_workers_preserves_live_worker_owned_by_running_run.rs"]
mod reap_orphaned_workers_preserves_live_worker_owned_by_running_run;
#[path = "workflow_runtime_tests/reap_orphaned_workers_skips_pid_reuse_when_marker_does_not_match.rs"]
mod reap_orphaned_workers_skips_pid_reuse_when_marker_does_not_match;
#[path = "workflow_runtime_tests/reap_orphaned_workers_skips_same_marker_pid_reuse_when_process_started_later.rs"]
mod reap_orphaned_workers_skips_same_marker_pid_reuse_when_process_started_later;
#[path = "workflow_runtime_tests/reap_stale_workflow_runs_finalizes_old_running_rows.rs"]
mod reap_stale_workflow_runs_finalizes_old_running_rows;
#[path = "workflow_runtime_tests/resolve_store_root_prefers_explicit_store_flag.rs"]
mod resolve_store_root_prefers_explicit_store_flag;
#[path = "workflow_runtime_tests/run_ndjson_child_allows_worker_finishing_before_wall_clock_timeout.rs"]
mod run_ndjson_child_allows_worker_finishing_before_wall_clock_timeout;
#[path = "workflow_runtime_tests/run_ndjson_child_does_not_kill_a_slow_but_streaming_worker.rs"]
mod run_ndjson_child_does_not_kill_a_slow_but_streaming_worker;
#[path = "workflow_runtime_tests/run_ndjson_child_kills_a_hung_worker_via_timeout.rs"]
mod run_ndjson_child_kills_a_hung_worker_via_timeout;
#[path = "workflow_runtime_tests/run_ndjson_child_kills_streaming_worker_via_wall_clock_timeout.rs"]
mod run_ndjson_child_kills_streaming_worker_via_wall_clock_timeout;
#[path = "workflow_runtime_tests/run_ndjson_child_warns_and_keeps_valid_events_after_junk_stdout.rs"]
mod run_ndjson_child_warns_and_keeps_valid_events_after_junk_stdout;
#[path = "workflow_runtime_tests/run_ndjson_child_without_orphan_registration_writes_no_pidfile.rs"]
mod run_ndjson_child_without_orphan_registration_writes_no_pidfile;
#[path = "workflow_runtime_tests/running_step_carries_session_id_for_live_drill_in.rs"]
mod running_step_carries_session_id_for_live_drill_in;
#[path = "workflow_runtime_tests/schema_correction_retry_limits_are_short_and_never_expand_existing_caps.rs"]
mod schema_correction_retry_limits_are_short_and_never_expand_existing_caps;
#[path = "workflow_runtime_tests/schema_failure_detail_distinguishes_retry_timeout_from_plain_schema_miss.rs"]
mod schema_failure_detail_distinguishes_retry_timeout_from_plain_schema_miss;
#[path = "workflow_runtime_tests/schema_instruction_lists_keys_and_inlines_the_shape.rs"]
mod schema_instruction_lists_keys_and_inlines_the_shape;
#[path = "workflow_runtime_tests/schema_required_keys_reads_top_level_object_keys.rs"]
mod schema_required_keys_reads_top_level_object_keys;
#[path = "workflow_runtime_tests/schema_to_json_schema_coerces_known_type_hints.rs"]
mod schema_to_json_schema_coerces_known_type_hints;
#[path = "workflow_runtime_tests/schema_to_json_schema_wraps_flat_and_passes_real_through.rs"]
mod schema_to_json_schema_wraps_flat_and_passes_real_through;
#[path = "workflow_runtime_tests/spawn_isolation_worktree_node_in_non_git_project_also_fails_loud.rs"]
mod spawn_isolation_worktree_node_in_non_git_project_also_fails_loud;
#[path = "workflow_runtime_tests/spawn_writable_node_in_non_git_project_fails_loud.rs"]
mod spawn_writable_node_in_non_git_project_fails_loud;
#[path = "workflow_runtime_tests/standalone_run_does_not_persist_failed_or_readonly_isolated_diffs.rs"]
mod standalone_run_does_not_persist_failed_or_readonly_isolated_diffs;
#[path = "workflow_runtime_tests/standalone_run_persists_successful_writable_diff.rs"]
mod standalone_run_persists_successful_writable_diff;
#[path = "workflow_runtime_tests/step_result_json_merges_details_without_overriding_base.rs"]
mod step_result_json_merges_details_without_overriding_base;
#[path = "workflow_runtime_tests/structured_is_surfaced_only_on_succeeded_status.rs"]
mod structured_is_surfaced_only_on_succeeded_status;
#[path = "workflow_runtime_tests/take_flag_value_removes_the_pair_and_returns_the_value.rs"]
mod take_flag_value_removes_the_pair_and_returns_the_value;
#[path = "workflow_runtime_tests/truncate_on_char_boundary_never_splits_a_multibyte_char.rs"]
mod truncate_on_char_boundary_never_splits_a_multibyte_char;
#[path = "workflow_runtime_tests/workflow_child_store_guard_isolates_nested_harness_by_default.rs"]
mod workflow_child_store_guard_isolates_nested_harness_by_default;
#[path = "workflow_runtime_tests/workflow_child_store_guard_respects_explicit_store_mutation_opt_in.rs"]
mod workflow_child_store_guard_respects_explicit_store_mutation_opt_in;
#[path = "workflow_runtime_tests/workflow_journaling_persists_patch_apply_reject_and_artifact_manifest.rs"]
mod workflow_journaling_persists_patch_apply_reject_and_artifact_manifest;
#[path = "workflow_runtime_tests/workflow_journaling_records_direct_diff_without_creating_patch.rs"]
mod workflow_journaling_records_direct_diff_without_creating_patch;
#[path = "workflow_runtime_tests/workflow_journaling_skips_discard_and_auto_applies_on_verdict.rs"]
mod workflow_journaling_skips_discard_and_auto_applies_on_verdict;
#[path = "workflow_runtime_tests/workflow_patch_apply_reject_edge_guards_hold.rs"]
mod workflow_patch_apply_reject_edge_guards_hold;
#[path = "workflow_runtime_tests/workflow_project_context_falls_back_to_cwd_without_metadata.rs"]
mod workflow_project_context_falls_back_to_cwd_without_metadata;
#[path = "workflow_runtime_tests/workflow_project_context_reads_pinned_metadata.rs"]
mod workflow_project_context_reads_pinned_metadata;
#[path = "workflow_runtime_tests/workflow_repo_root_is_project_root_not_process_cwd.rs"]
mod workflow_repo_root_is_project_root_not_process_cwd;
#[path = "workflow_runtime_tests/workflow_run_defaults_do_not_override_leaf_model_or_effort.rs"]
mod workflow_run_defaults_do_not_override_leaf_model_or_effort;
#[path = "workflow_runtime_tests/workflow_run_journals_steps_and_completes_with_mock_driver.rs"]
mod workflow_run_journals_steps_and_completes_with_mock_driver;
#[path = "workflow_runtime_tests/workflow_run_rejects_a_different_explicit_project_binding.rs"]
mod workflow_run_rejects_a_different_explicit_project_binding;
#[path = "workflow_runtime_tests/workflow_run_script_journals_steps_and_snapshots_source.rs"]
mod workflow_run_script_journals_steps_and_snapshots_source;
#[path = "workflow_runtime_tests/workflow_run_script_rejects_bad_args_json.rs"]
mod workflow_run_script_rejects_bad_args_json;
#[path = "workflow_runtime_tests/workflow_run_script_rejects_missing_design_intent.rs"]
mod workflow_run_script_rejects_missing_design_intent;
#[path = "workflow_runtime_tests/workflow_run_script_resume_rejects_changed_script.rs"]
mod workflow_run_script_resume_rejects_changed_script;
#[path = "workflow_runtime_tests/workflow_run_script_resume_reuses_prior_steps.rs"]
mod workflow_run_script_resume_reuses_prior_steps;
#[path = "workflow_runtime_tests/workflow_run_transitions_running_to_failed_on_failed_required_step.rs"]
mod workflow_run_transitions_running_to_failed_on_failed_required_step;
#[path = "workflow_runtime_tests/worktree_create_in_non_git_dir_gives_actionable_error.rs"]
mod worktree_create_in_non_git_dir_gives_actionable_error;
#[path = "workflow_runtime_tests/worktree_diff_capture_is_binary_safe_and_enumerates_paths.rs"]
mod worktree_diff_capture_is_binary_safe_and_enumerates_paths;
#[path = "workflow_runtime_tests/worktree_paths_are_unique_per_leaf_even_with_duplicate_labels.rs"]
mod worktree_paths_are_unique_per_leaf_even_with_duplicate_labels;
#[path = "workflow_runtime_tests/writable_worktree_path_is_under_project_root.rs"]
mod writable_worktree_path_is_under_project_root;
