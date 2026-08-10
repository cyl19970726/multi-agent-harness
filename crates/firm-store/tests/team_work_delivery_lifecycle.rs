//! Regression coverage for the TeamRun WorkDelivery lifecycle contract in
//! `docs/product/agent-team-works.md` and the runtime/mailbox table of
//! `specs/nested-agent-team-organization/design.md` (PR #302):
//!
//! - one execution slot per MemberRun: a member with active Work cannot start
//!   or claim a second Work, and the Supervisor claim fence does not hand a
//!   second delivery to an occupied member (dual-driver risk);
//! - busy members: Host assignment queues a durable delivery instead of
//!   interrupting, and the queued delivery becomes claimable at the next safe
//!   boundary;
//! - closed members: queued deliveries freeze and new deliveries are rejected;
//!   reopen delivers the frozen Work version exactly once
//!   (duplicate-delivery risk);
//! - retired members: no new WorkDelivery and no member-driven transitions;
//!   ordinary delivery cannot revive the member.
//!
//! These tests pin behavior of the current TeamRun-scoped Work store only;
//! they do not depend on the future persistent Team-scoped Work schema.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use firm_core::{
    AgentTeamRun, ExecutionNode, ExecutionNodeStatus, MemberCoordinationStatus, MemberRun,
    MemberRunStatus, NodeDaemonLeaseStatus, TeamActorKind, TeamActorRef, TeamRunStatus, Work,
    WorkClaimMode, WorkCommandContext, WorkCondition, WorkDelivery, WorkDeliveryStatus, WorkPhase,
    WorkPriority,
};
use firm_store::{HarnessStore, WorkDeliveryClaimResult};

static NEXT_TEMP: AtomicU64 = AtomicU64::new(1);

const SUPERVISOR: &str = "sup-delivery-test";
const NOW_MS: u64 = 1_000_000;
const LEASE_TTL_MS: u64 = 600_000;

struct TestStore {
    root: PathBuf,
    store: HarnessStore,
}

impl TestStore {
    fn new(label: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "harness-work-delivery-{label}-{}-{}",
            std::process::id(),
            NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
        ));
        let store = HarnessStore::new(&root);
        store.init().expect("initialize test store");
        Self { root, store }
    }
}

impl Drop for TestStore {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn team_fixture(label: &str) -> (TestStore, AgentTeamRun, MemberRun, MemberRun) {
    let harness = TestStore::new(label);
    let run = AgentTeamRun {
        id: format!("tr-{label}"),
        agent_team_id: format!("team-{label}"),
        execution_node_id: "00000000-0000-4000-8000-000000000001".into(),
        project_binding_id: "project-test".into(),
        previous_run_id: None,
        host_surface: "codex-app".into(),
        host_thread_id: Some(format!("host-{label}")),
        host_actor: None,
        host_control_mode: Default::default(),
        objective: "prove WorkDelivery lifecycle".into(),
        execution_root: None,
        status: TeamRunStatus::Running,
        member_run_ids: vec![format!("mr-{label}-a"), format!("mr-{label}-b")],
        budget_limit_usd: None,
        created_at: "unix-ms:1".into(),
        updated_at: "unix-ms:1".into(),
        completed_at: None,
    };
    let member = |suffix: &str| MemberRun {
        id: format!("mr-{label}-{suffix}"),
        team_run_id: run.id.clone(),
        slot_id: Some(format!("slot-{suffix}")),
        agent_member_id: format!("agent-{suffix}"),
        name: format!("Member {suffix}"),
        role: "builder".into(),
        provider: "codex".into(),
        model: None,
        provider_controls: Default::default(),
        provider_profile: None,
        provider_capacity: None,
        provider_compatibility_block_cause: None,
        coordination_status: MemberCoordinationStatus::Active,
        runtime_generation: 1,
        status: MemberRunStatus::Idle,
        native_session: None,
        worktree_ref: None,
        workspace_snapshot: None,
        owned_paths: Vec::new(),
        started_at: "unix-ms:1".into(),
        last_event_at: None,
        finished_at: None,
        zero_output_streak: 0,
        last_consumed_work_version: None,
    };
    let member_a = member("a");
    let member_b = member("b");
    harness
        .store
        .append_team_run(&run)
        .expect("append team run");
    harness
        .store
        .append_member_run(&member_a)
        .expect("append member a");
    harness
        .store
        .append_member_run(&member_b)
        .expect("append member b");
    (harness, run, member_a, member_b)
}

fn host_context(event_id: &str, key: &str, at: &str) -> WorkCommandContext {
    WorkCommandContext {
        event_id: event_id.into(),
        performed_by_actor: TeamActorRef {
            kind: TeamActorKind::Host,
            id: "host".into(),
            display_name: Some("Host".into()),
            authn_source: Some("test".into()),
        },
        authority_actor: None,
        causation_ref: None,
        idempotency_key: key.into(),
        created_at: at.into(),
        duplicate_ok: false,
    }
}

fn member_context(member_run_id: &str, event_id: &str, key: &str, at: &str) -> WorkCommandContext {
    WorkCommandContext {
        event_id: event_id.into(),
        performed_by_actor: TeamActorRef {
            kind: TeamActorKind::MemberRun,
            id: member_run_id.into(),
            display_name: None,
            authn_source: Some("bound-runtime:test".into()),
        },
        authority_actor: None,
        causation_ref: None,
        idempotency_key: key.into(),
        created_at: at.into(),
        duplicate_ok: false,
    }
}

fn base_work(run_id: &str, id: &str) -> Work {
    Work {
        id: id.into(),
        team_run_id: run_id.into(),
        team_id: None,
        created_by_member_id: None,
        parent_work_id: None,
        title: format!("Work {id}"),
        context_markdown: "context".into(),
        completion_criteria_markdown: "criteria".into(),
        phase: WorkPhase::Open,
        condition: WorkCondition::Normal,
        resolution: None,
        owner_member_id: None,
        active_member_run_id: None,
        claim_mode: WorkClaimMode::TeamClaim,
        eligible_member_ids: Vec::new(),
        prerequisite_work_ids: Vec::new(),
        priority: WorkPriority::High,
        created_by_actor: host_context("ignored", "ignored", "unix-ms:1").performed_by_actor,
        result_summary: None,
        blocker_reason: None,
        artifact_refs: Vec::new(),
        check_refs: Vec::new(),
        github_links: Vec::new(),
        version: 0,
        created_at: String::new(),
        updated_at: String::new(),
    }
}

fn owned_work(run_id: &str, id: &str, member: &MemberRun) -> Work {
    let mut work = base_work(run_id, id);
    work.claim_mode = WorkClaimMode::HostAssign;
    work.owner_member_id = Some(member.agent_member_id.clone());
    work.active_member_run_id = Some(member.id.clone());
    work
}

/// Persist a member coordination/runtime transition exactly the way the
/// Supervisor driver and the reopen command do: one new MemberRun row that
/// keeps identity, workspace, and native-session binding untouched.
fn set_member_state(
    harness: &TestStore,
    member: &MemberRun,
    coordination: MemberCoordinationStatus,
    status: MemberRunStatus,
    generation: u64,
) {
    let current = harness
        .store
        .member_runs()
        .expect("read MemberRun history")
        .into_iter()
        .rev()
        .find(|candidate| candidate.id == member.id)
        .expect("current MemberRun");
    let mut next = current.clone();
    next.coordination_status = coordination;
    next.status = status;
    next.runtime_generation = generation;
    harness
        .store
        .compare_and_append_member_run(&current, &next)
        .expect("append member state");
}

fn deliveries_for(harness: &TestStore, work_id: &str) -> Vec<WorkDelivery> {
    harness
        .store
        .latest_work_deliveries()
        .expect("read deliveries")
        .into_iter()
        .filter(|delivery| delivery.work_id == work_id)
        .collect()
}

fn acquire_lease(harness: &TestStore, run_id: &str) {
    let node = ExecutionNode {
        id: "00000000-0000-4000-8000-000000000001".into(),
        display_name: "test-node".into(),
        status: ExecutionNodeStatus::Active,
        created_at: "unix-ms:1".into(),
        updated_at: "unix-ms:1".into(),
    };
    if harness.store.latest_execution_nodes().unwrap().is_empty() {
        harness.store.insert_execution_node(&node).unwrap();
    }
    let parent = harness
        .store
        .acquire_node_daemon_lease(
            &node.id,
            "daemon-test",
            "instance-test",
            NOW_MS,
            LEASE_TTL_MS,
        )
        .expect("acquire node daemon lease");
    assert_eq!(parent.status, NodeDaemonLeaseStatus::Active);
    harness
        .store
        .acquire_team_supervisor_under_node_lease(
            run_id,
            &node.id,
            &parent.daemon_id,
            parent.generation,
            "space-test",
            "project-test",
            SUPERVISOR,
            std::process::id(),
            "loopback://delivery-test",
            NOW_MS,
            LEASE_TTL_MS,
        )
        .expect("acquire supervisor lease");
}

fn claim_delivery(
    harness: &TestStore,
    run_id: &str,
    delivery_id: &str,
    member_run_id: &str,
    claim_id: &str,
) -> WorkDeliveryClaimResult {
    harness
        .store
        .claim_work_delivery(
            run_id,
            delivery_id,
            member_run_id,
            SUPERVISOR,
            1,
            claim_id,
            NOW_MS + 1,
            "unix-ms:1000001",
        )
        .expect("claim work delivery")
}

fn complete_delivery(
    harness: &TestStore,
    run_id: &str,
    delivery_id: &str,
    member_run_id: &str,
    claim_id: &str,
    provider_receipt_id: &str,
) -> WorkDelivery {
    harness
        .store
        .complete_work_delivery_claim(
            run_id,
            delivery_id,
            member_run_id,
            SUPERVISOR,
            1,
            claim_id,
            provider_receipt_id,
            NOW_MS + 2,
            "unix-ms:1000002",
        )
        .expect("complete work delivery")
}

#[test]
fn member_with_active_work_cannot_start_or_claim_a_second_work() {
    let (harness, run, member_a, member_b) = team_fixture("one-execution-slot");
    let store = &harness.store;

    let work_one = store
        .insert_work(
            owned_work(&run.id, "work-1", &member_a),
            host_context("we-w1", "create-w1", "unix-ms:2"),
        )
        .expect("create owned Work");
    assert_eq!(work_one.version, 1);

    let started = store
        .start_work(
            "work-1",
            1,
            &member_a.id,
            member_context(&member_a.id, "we-start-1", "start-w1", "unix-ms:3"),
        )
        .expect("start first Work");
    assert_eq!(started.phase, WorkPhase::Active);

    // Host assignment to a busy member is allowed; it queues a durable
    // delivery instead of interrupting the active turn.
    store
        .insert_work(
            owned_work(&run.id, "work-2", &member_a),
            host_context("we-w2", "create-w2", "unix-ms:4"),
        )
        .expect("assignment to a busy member queues");
    let queued = deliveries_for(&harness, "work-2");
    assert_eq!(queued.len(), 1);
    assert_eq!(queued[0].status, WorkDeliveryStatus::Queued);

    let busy_start = store
        .start_work(
            "work-2",
            1,
            &member_a.id,
            member_context(&member_a.id, "we-start-2", "start-w2", "unix-ms:5"),
        )
        .expect_err("a second active Work on one MemberRun must be rejected");
    assert!(
        busy_start.to_string().contains("MEMBER_BUSY"),
        "unexpected error: {busy_start}"
    );

    // The shared-pool claim path applies the same single-slot rule.
    store
        .insert_work(
            base_work(&run.id, "work-3"),
            host_context("we-w3", "create-w3", "unix-ms:6"),
        )
        .expect("create unassigned Work");
    let busy_claim = store
        .claim_work(
            "work-3",
            1,
            &member_a.id,
            member_context(&member_a.id, "we-claim-3", "claim-w3", "unix-ms:7"),
        )
        .expect_err("claiming a second Work while one is active must be rejected");
    assert!(
        busy_claim.to_string().contains("MEMBER_BUSY"),
        "unexpected error: {busy_claim}"
    );

    // The fence is per MemberRun, not global: an idle peer claims the pool.
    let claimed = store
        .claim_work(
            "work-3",
            1,
            &member_b.id,
            member_context(&member_b.id, "we-claim-3b", "claim-w3-b", "unix-ms:8"),
        )
        .expect("idle peer claims the pooled Work");
    assert_eq!(
        claimed.active_member_run_id.as_deref(),
        Some(member_b.id.as_str())
    );

    // Neither rejected command mutated the board.
    let works = store.latest_works().expect("read works");
    let work_two = works
        .iter()
        .find(|work| work.id == "work-2")
        .expect("work-2");
    assert_eq!(work_two.phase, WorkPhase::Open);
    assert_eq!(work_two.version, 1);
}

#[test]
fn busy_member_assignment_queues_and_fence_releases_at_safe_boundary() {
    let (harness, run, member_a, _) = team_fixture("busy-delivery-fence");
    let store = &harness.store;
    acquire_lease(&harness, &run.id);

    store
        .insert_work(
            owned_work(&run.id, "work-1", &member_a),
            host_context("we-w1", "create-w1", "unix-ms:2"),
        )
        .expect("create owned Work");
    let first_id = deliveries_for(&harness, "work-1")[0].id.clone();

    // The Supervisor claims the delivery and the native runtime receipts it;
    // during this hand-off window the Work itself is still open.
    match claim_delivery(&harness, &run.id, &first_id, &member_a.id, "claim-1") {
        WorkDeliveryClaimResult::Claimed(delivery) => assert_eq!(delivery.attempt, 1),
        WorkDeliveryClaimResult::NotQueued => panic!("fresh delivery must be claimable"),
    }
    complete_delivery(
        &harness,
        &run.id,
        &first_id,
        &member_a.id,
        "claim-1",
        "receipt-native-w1",
    );

    // Assignment to the busy member queues durably instead of interrupting.
    store
        .insert_work(
            owned_work(&run.id, "work-2", &member_a),
            host_context("we-w2", "create-w2", "unix-ms:5"),
        )
        .expect("assignment to a busy member queues");
    let second_id = deliveries_for(&harness, "work-2")[0].id.clone();
    assert!(
        matches!(
            claim_delivery(&harness, &run.id, &second_id, &member_a.id, "claim-2"),
            WorkDeliveryClaimResult::NotQueued
        ),
        "a provider-received first Work occupies the single execution slot"
    );

    // An explicitly started in-progress Work fences the same way.
    store
        .start_work(
            "work-1",
            1,
            &member_a.id,
            member_context(&member_a.id, "we-start-1", "start-w1", "unix-ms:6"),
        )
        .expect("start first Work");
    assert!(
        matches!(
            claim_delivery(&harness, &run.id, &second_id, &member_a.id, "claim-3"),
            WorkDeliveryClaimResult::NotQueued
        ),
        "an in-progress Work fences deliveries of other Works to the same member"
    );

    // Submission frees the slot: the queued delivery becomes claimable at the
    // safe boundary, without any synthetic event on the waiting Work.
    store
        .submit_work(
            "work-1",
            2,
            &member_a.id,
            "first result",
            Vec::new(),
            Vec::new(),
            member_context(&member_a.id, "we-submit-1", "submit-w1", "unix-ms:7"),
        )
        .expect("submit first Work");
    match claim_delivery(&harness, &run.id, &second_id, &member_a.id, "claim-4") {
        WorkDeliveryClaimResult::Claimed(delivery) => assert_eq!(delivery.attempt, 1),
        WorkDeliveryClaimResult::NotQueued => {
            panic!("queued delivery must be claimable once the member is idle")
        }
    }

    // A claimed row is never handed out twice.
    assert!(matches!(
        claim_delivery(&harness, &run.id, &second_id, &member_a.id, "claim-5"),
        WorkDeliveryClaimResult::NotQueued
    ));
}

#[test]
fn close_freezes_and_reopen_delivers_a_queued_work_version_exactly_once() {
    let (harness, run, member_a, _) = team_fixture("close-reopen-deliver-once");
    let store = &harness.store;
    acquire_lease(&harness, &run.id);

    store
        .insert_work(
            owned_work(&run.id, "work-1", &member_a),
            host_context("we-w1", "create-w1", "unix-ms:2"),
        )
        .expect("create owned Work");
    let delivery_id = deliveries_for(&harness, "work-1")[0].id.clone();

    // Close: the Supervisor driver persists Closed + Stopped and retains the
    // native-session binding on the same MemberRun identity.
    set_member_state(
        &harness,
        &member_a,
        MemberCoordinationStatus::Closed,
        MemberRunStatus::Stopped,
        1,
    );

    // Frozen: the queued delivery stays durable but is not claimable.
    assert!(matches!(
        claim_delivery(&harness, &run.id, &delivery_id, &member_a.id, "claim-1"),
        WorkDeliveryClaimResult::NotQueued
    ));
    assert_eq!(
        deliveries_for(&harness, "work-1")[0].status,
        WorkDeliveryStatus::Queued,
        "close must not drop or invalidate the durable delivery"
    );

    // New deliveries to a closed member are rejected at the store.
    store
        .insert_work(
            base_work(&run.id, "work-2"),
            host_context("we-w2", "create-w2", "unix-ms:4"),
        )
        .expect("create unassigned Work");
    let closed_assign = store
        .assign_work(
            "work-2",
            1,
            &member_a.id,
            host_context("we-assign-2", "assign-w2", "unix-ms:5"),
        )
        .expect_err("assigning to a closed member must fail");
    assert!(
        closed_assign.to_string().contains("MEMBER_UNAVAILABLE"),
        "unexpected error: {closed_assign}"
    );

    // Reopen: same member identity and native-session binding, next runtime
    // generation. The frozen delivery becomes claimable again.
    set_member_state(
        &harness,
        &member_a,
        MemberCoordinationStatus::Active,
        MemberRunStatus::Queued,
        2,
    );
    match claim_delivery(&harness, &run.id, &delivery_id, &member_a.id, "claim-2") {
        WorkDeliveryClaimResult::Claimed(delivery) => {
            assert_eq!(delivery.attempt, 1);
            assert_eq!(delivery.work_version, 1);
        }
        WorkDeliveryClaimResult::NotQueued => {
            panic!("reopen must unfreeze the queued delivery")
        }
    }
    assert!(
        matches!(
            claim_delivery(&harness, &run.id, &delivery_id, &member_a.id, "claim-3"),
            WorkDeliveryClaimResult::NotQueued
        ),
        "a claimed delivery is never handed out twice"
    );

    complete_delivery(
        &harness,
        &run.id,
        &delivery_id,
        &member_a.id,
        "claim-2",
        "receipt-native-session-w1",
    );
    assert!(
        matches!(
            claim_delivery(&harness, &run.id, &delivery_id, &member_a.id, "claim-4"),
            WorkDeliveryClaimResult::NotQueued
        ),
        "a provider-received delivery is never redelivered"
    );

    let deliveries = deliveries_for(&harness, "work-1");
    assert_eq!(
        deliveries.len(),
        1,
        "one accepted Work version produces one durable delivery"
    );
    assert_eq!(deliveries[0].status, WorkDeliveryStatus::ProviderReceived);
    assert_eq!(
        deliveries[0].attempt, 1,
        "the Work version was delivered exactly once across close/reopen"
    );
    assert_eq!(
        deliveries[0].provider_receipt_id.as_deref(),
        Some("receipt-native-session-w1")
    );
}

#[test]
fn retired_member_rejects_new_deliveries_and_member_driven_transitions() {
    let (harness, run, member_a, _) = team_fixture("retired-member-rejects");
    let store = &harness.store;
    acquire_lease(&harness, &run.id);

    store
        .insert_work(
            owned_work(&run.id, "work-1", &member_a),
            host_context("we-w1", "create-w1", "unix-ms:2"),
        )
        .expect("create owned Work");
    let delivery_id = deliveries_for(&harness, "work-1")[0].id.clone();

    set_member_state(
        &harness,
        &member_a,
        MemberCoordinationStatus::Retired,
        MemberRunStatus::Stopped,
        1,
    );

    // Retired members receive no new WorkDelivery; the Host must reassign.
    let retired_create = store
        .insert_work(
            owned_work(&run.id, "work-2", &member_a),
            host_context("we-w2", "create-w2", "unix-ms:3"),
        )
        .expect_err("creating owned Work for a retired member must fail");
    assert!(
        retired_create.to_string().contains("MEMBER_UNAVAILABLE"),
        "unexpected error: {retired_create}"
    );
    store
        .insert_work(
            base_work(&run.id, "work-3"),
            host_context("we-w3", "create-w3", "unix-ms:4"),
        )
        .expect("create unassigned Work");
    let retired_assign = store
        .assign_work(
            "work-3",
            1,
            &member_a.id,
            host_context("we-assign-3", "assign-w3", "unix-ms:5"),
        )
        .expect_err("assigning to a retired member must fail");
    assert!(
        retired_assign.to_string().contains("MEMBER_UNAVAILABLE"),
        "unexpected error: {retired_assign}"
    );

    // The previously queued delivery stays durable but ordinary delivery
    // cannot revive the member.
    assert!(matches!(
        claim_delivery(&harness, &run.id, &delivery_id, &member_a.id, "claim-1"),
        WorkDeliveryClaimResult::NotQueued
    ));
    assert_eq!(
        deliveries_for(&harness, "work-1")[0].status,
        WorkDeliveryStatus::Queued
    );

    // Member-driven transitions are refused for the same reason.
    let retired_start = store
        .start_work(
            "work-1",
            1,
            &member_a.id,
            member_context(&member_a.id, "we-start-1", "start-w1", "unix-ms:6"),
        )
        .expect_err("a retired member must not start Work");
    assert!(
        retired_start.to_string().contains("MEMBER_BUSY"),
        "unexpected error: {retired_start}"
    );
}
