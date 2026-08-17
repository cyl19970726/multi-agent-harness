//! Real-CLI coverage for the exact-session headless Host consumer (#387).

use std::process::Command;
use std::{fs::OpenOptions, io::Write};

use harness_core::{
    AgentTeamRun, HostAttention, HostAttentionKind, HostAttentionStatus, HostBindingLeaseStatus,
    HostControlMode, TeamActorKind, TeamActorRef, TeamRunStatus, Work, WorkClaimMode,
    WorkCommandContext, WorkCondition, WorkPhase, WorkPriority,
};
use harness_store::HarnessStore;

mod fake_provider;
mod firm_env;

use firm_env::{current_project_id, run_firm, TempHome};

fn append_legacy_jsonl<T: serde::Serialize>(path: &std::path::Path, value: &T) {
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .expect("open Legacy fixture ledger");
    serde_json::to_writer(&mut file, value).expect("serialize Legacy fixture row");
    file.write_all(b"\n").expect("terminate Legacy fixture row");
    file.sync_all().expect("persist Legacy fixture row");
}

#[test]
fn dispatch_host_resumes_exact_kimi_session_and_releases_lease() {
    let home = TempHome::new("host-dispatch-exact-kimi");
    let project_root = home.base().join("project");
    std::fs::create_dir_all(&project_root).unwrap();
    let init = run_firm(&home, &project_root, &["init"]);
    assert!(init.status.success(), "init failed: {init:?}");
    let project_id = current_project_id(&home);
    let store_root = home.spaces_dir().join(&project_id);
    let store = HarnessStore::new(&store_root);
    store.init().unwrap();

    let run = AgentTeamRun {
        id: "team-run-host-dispatch".into(),
        agent_team_id: "team-host-dispatch".into(),
        execution_node_id: "00000000-0000-4000-8000-000000000001".into(),
        previous_run_id: None,
        project_binding_id: project_id.clone(),
        host_surface: "kimi".into(),
        host_thread_id: Some("session_exact_host".into()),
        host_actor: None,
        host_control_mode: HostControlMode::External,
        objective: "Triage one submitted Work".into(),
        execution_root: None,
        budget_limit_usd: None,
        status: TeamRunStatus::Running,
        member_run_ids: Vec::new(),
        created_at: "unix-ms:1".into(),
        updated_at: "unix-ms:1".into(),
        completed_at: None,
    };
    store
        .insert_execution_node(&harness_core::ExecutionNode {
            id: run.execution_node_id.clone(),
            display_name: "test-node".into(),
            status: harness_core::ExecutionNodeStatus::Active,
            created_at: "unix-ms:1".into(),
            updated_at: "unix-ms:1".into(),
        })
        .unwrap();
    store
        .register_node_project(
            &harness_core::NodeProjectRegistration {
                node_id: run.execution_node_id.clone(),
                execution_space_id: project_id.clone(),
                project_binding_id: run.project_binding_id.clone(),
                status: harness_core::NodeProjectRegistrationStatus::Active,
                created_at: "unix-ms:1".into(),
                updated_at: "unix-ms:1".into(),
            },
            &project_id,
        )
        .unwrap();
    append_legacy_jsonl(&store.root().join("team_runs.jsonl"), &run);
    let host_actor = TeamActorRef {
        kind: TeamActorKind::Host,
        id: "host".into(),
        display_name: None,
        authn_source: None,
    };
    let work = store
        .insert_work(
            Work {
                id: "work-review".into(),
                team_run_id: run.id.clone(),
                accountable_team_id: None,
                assignee_membership_id: None,
                parent_work_id: None,
                title: "Review exact Host dispatch".into(),
                context_markdown: String::new(),
                completion_criteria_markdown: "exact bound Host receives triage".into(),
                phase: WorkPhase::Open,
                condition: WorkCondition::Normal,
                resolution: None,
                owner_member_id: None,
                active_member_run_id: None,
                claim_mode: WorkClaimMode::HostAssign,
                eligible_member_ids: Vec::new(),
                prerequisite_work_ids: Vec::new(),
                priority: WorkPriority::Normal,
                created_by_actor: host_actor.clone(),
                created_by_member_id: None,
                result_summary: None,
                blocker_reason: None,
                artifact_refs: Vec::new(),
                check_refs: Vec::new(),
                github_links: Vec::new(),
                version: 0,
                created_at: String::new(),
                updated_at: String::new(),
            },
            WorkCommandContext {
                event_id: "work-event:create".into(),
                performed_by_actor: host_actor,
                authority_actor: None,
                causation_ref: None,
                idempotency_key: "create-work-review".into(),
                created_at: "unix-ms:1".into(),
                duplicate_ok: false,
            },
        )
        .unwrap();
    store
        .ensure_host_attention(&HostAttention {
            id: "attention-exact-host".into(),
            team_run_id: run.id.clone(),
            kind: HostAttentionKind::WorkReviewRequested,
            work_id: work.id.clone(),
            work_version: work.version,
            source_event_ref: "work-event:submitted:1".into(),
            member_run_id: None,
            status: HostAttentionStatus::Actionable,
            attempt: 0,
            claim_id: None,
            claimed_host_surface: None,
            claimed_host_thread_id: None,
            claimed_host_lease_id: None,
            claimed_host_lease_generation: None,
            claimed_host_lease_owner_id: None,
            provider_receipt_id: None,
            last_failure_reason: None,
            created_at: "unix-ms:1".into(),
            updated_at: "unix-ms:1".into(),
        })
        .unwrap();

    let fake_bin = fake_provider::install_kimi_acp_shim(home.base());
    let attach_marker = home.base().join("host-attach.log");
    let prompt_marker = home.base().join("host-prompt.log");
    let output = Command::new(env!("CARGO_BIN_EXE_firm"))
        .args([
            "--project",
            &project_id,
            "team-run",
            "dispatch-host",
            "--id",
            &run.id,
            "--min-age-s",
            "0",
            "--timeout-ms",
            "5000",
        ])
        .current_dir(&project_root)
        .envs(home.envs())
        .env_remove("FIRM_ROOT")
        .env_remove("FIRM_PROJECT")
        .env_remove("FIRM_SPACE")
        .env("KIMI_CODE_BIN", fake_bin.join("kimi"))
        .env("FAKE_KIMI_VERSION", "0.36.1")
        .env("FAKE_KIMI_ATTACH_MARKER", &attach_marker)
        .env("FAKE_KIMI_PROMPT_MARKER", &prompt_marker)
        .output()
        .expect("dispatch Host");
    assert!(
        output.status.success(),
        "dispatch failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let result: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["dispatched"], true);
    assert_eq!(result["host_thread_id"], "session_exact_host");

    let attach = std::fs::read_to_string(&attach_marker).unwrap();
    assert!(attach.contains("session_exact_host"));
    let prompt = std::fs::read_to_string(&prompt_marker).unwrap();
    assert!(prompt.contains("READ-ONLY TRIAGE"));
    assert!(prompt.contains("MUST NOT accept"));

    let attention = store
        .host_attentions()
        .unwrap()
        .into_iter()
        .find(|row| row.id == "attention-exact-host")
        .unwrap();
    assert_eq!(attention.status, HostAttentionStatus::Delivered);
    assert!(attention
        .provider_receipt_id
        .as_deref()
        .is_some_and(|receipt| receipt.starts_with("kimi-acp-prompt:")));
    let lease = store.latest_host_binding_lease(&run.id).unwrap().unwrap();
    assert_eq!(lease.status, HostBindingLeaseStatus::Released);
}
