use super::*;

/// A member admitted into a live TeamRun (`team-run add-member`) misses the
/// adoption pass that materializes AgentSessions, so the Supervisor provisions
/// it on first drive — exactly once, and never again for a member that already
/// owns one (#749).
///
/// This pins the durable half of that seam. It calls no provider executable on
/// purpose: `ensure_joined_member_runtime_fabric` freezes the provider profile
/// first, and that version probe belongs to the daemon integration test where
/// a deterministic PATH shim exists.
#[test]
fn joined_member_runtime_fabric_is_provisioned_once_under_the_live_supervisor() {
    let (store, root) = temp_store("joined-member-runtime-fabric");
    let created = create_two_member_team_run(&store);
    let execution_space_id =
        team_run_execution_space_id(&store, &created.team_run).expect("TeamRun Execution Space");
    let lease = store
        .acquire_test_supervisor_lease(
            &created.team_run.id,
            "supervisor-joined-member-fabric",
            std::process::id(),
            "test://joined-member-fabric",
            current_unix_ms_u64(),
            600_000,
        )
        .expect("acquire the live Supervisor lease");
    let ledger = TeamRunLedger::new(
        &store,
        &created.team_run.id,
        &lease.supervisor_id,
        lease.generation,
        Arc::new(AtomicBool::new(true)),
    );

    let late = TeamMemberSpec {
        agent_member_id: "agent-joined-late".into(),
        name: "JoinedLate".into(),
        role: "reviewer".into(),
        provider: "codex".into(),
        execution_mode: Some("codex_app_server".into()),
        model: None,
        effort: None,
        service_tier: None,
        provider_cwd_hint: None,
        owned_paths: vec!["crates/review".into()],
        resume_native_session_id: None,
        initial_work: None,
    };
    ensure_unit_test_canonical_members(
        &store,
        &execution_space_id,
        &created.team_run.agent_team_id,
        std::slice::from_ref(&late),
    )
    .expect("the joining AgentMember earns its durable TeamMembership first");
    let (run, joined, _) = add_team_run_member(&store, None, &created.team_run.id, &late, None)
        .expect("admit the member into the live run");
    assert!(
        member_needs_agent_session(&store, &execution_space_id, &joined)
            .expect("session inventory"),
        "the defect precondition is that add-member leaves the joined member sessionless"
    );

    let session_id =
        match provision_member_agent_session(&ledger, &lease, &run, &execution_space_id, &joined)
            .expect("the live Supervisor provisions the joined member's AgentSession")
        {
            JoinedMemberRuntimeFabric::Provisioned { session_id } => session_id,
            JoinedMemberRuntimeFabric::AlreadyProvisioned => {
                panic!("a sessionless member must be provisioned, not skipped")
            }
        };
    let sessions = current_sessions(&store, &execution_space_id, &joined.agent_member_id);
    let [session] = sessions.as_slice() else {
        panic!(
            "one AgentMember owns exactly one current AgentSession, found {}",
            sessions.len()
        );
    };
    assert_eq!(session.id, session_id);
    assert_eq!(session.node_id, created.team_run.execution_node_id);
    assert_eq!(session.execution_space_id, execution_space_id);
    assert_eq!(session.node_daemon_id, lease.node_daemon_id);
    assert_eq!(session.node_daemon_generation, lease.node_daemon_generation);
    assert_eq!(session.provider_kind, joined.provider);
    assert_eq!(
        session.control_state.driver_ref,
        harness_core::agentfirm_api::RuntimeDriverRef::TeamSupervisor {
            team_run_id: created.team_run.id.clone(),
            team_supervisor_id: lease.supervisor_id.clone(),
            team_supervisor_generation: lease.generation,
        },
        "the joined member's session must be bound to the live Supervisor generation"
    );
    let provisioned = session.clone();

    // The guard is internal, so a direct second call is a no-op too: re-binding
    // a live member would reset residency/activity and lie about an attached
    // provider handle.
    assert!(
        !member_needs_agent_session(&store, &execution_space_id, &joined)
            .expect("session inventory"),
        "a provisioned member must not be provisioned again"
    );
    assert!(
        matches!(
            provision_member_agent_session(&ledger, &lease, &run, &execution_space_id, &joined)
                .expect("re-provisioning the same generation is idempotent"),
            JoinedMemberRuntimeFabric::AlreadyProvisioned
        ),
        "the provisioning function must refuse to re-bind a member that owns a session"
    );
    assert_eq!(
        current_sessions(&store, &execution_space_id, &joined.agent_member_id).as_slice(),
        [provisioned],
        "an idempotent re-run must not mint a second session or rewrite control state"
    );

    std::fs::remove_dir_all(root).expect("cleanup");
}

/// A member that already owns its session never consults an authority, so a
/// Supervisor generation that no longer owns the run cannot fail it. This is
/// the founding-member case: the whole roster is pending on every adoption,
/// and none of it may be touched by a fence it does not use.
#[test]
fn member_with_a_session_is_untouched_by_a_stale_supervisor_generation() {
    let fixture = JoinedMemberFixture::new("joined-member-fabric-untouched");
    let mut joined = fixture.joined.clone();
    fixture
        .provision(&mut joined)
        .expect("the live Supervisor provisions the joined member's AgentSession");

    let stale = fixture.stale_ledger();
    let before = durable_store_file_bytes(&fixture.store);
    match ensure_joined_member_runtime_fabric(&stale, &mut joined)
        .expect("a member that owns its session answers before any authority is read")
    {
        JoinedMemberRuntimeFabric::AlreadyProvisioned => {}
        JoinedMemberRuntimeFabric::Provisioned { session_id } => {
            panic!("a member that owns a session must not be re-provisioned as {session_id}")
        }
    }
    assert_eq!(
        durable_store_file_bytes(&fixture.store),
        before,
        "answering `already provisioned` must have byte-zero durable side effects"
    );
    fixture.cleanup();
}

/// Losing the Supervisor lease mid-provisioning is this generation's problem,
/// not the member's. It must surface as the typed lease loss the drive loop
/// already latches, never as a terminal MemberRunStatus::Failed.
#[test]
fn lease_lost_while_provisioning_never_journals_the_member_as_failed() {
    let fixture = JoinedMemberFixture::new("joined-member-fabric-lease-lost");
    let mut joined = fixture.joined.clone();
    let stale = fixture.stale_ledger();

    let before = durable_store_file_bytes(&fixture.store);
    let error = ensure_joined_member_runtime_fabric(&stale, &mut joined)
        .expect_err("a generation that no longer owns the run cannot provision");
    assert!(
        error.is_supervisor_lease_lost(),
        "unexpected error type: {error}"
    );
    assert!(
        matches!(
            classify_member_fabric_failure(&error),
            MemberFabricFailure::LeaseLost
        ),
        "a lost lease must quiesce the generation, not fail the member: {error}"
    );
    assert_eq!(
        durable_store_file_bytes(&fixture.store),
        before,
        "a lease-lost provisioning attempt must write nothing at all"
    );
    assert_ne!(
        fixture.latest_joined_status(),
        MemberRunStatus::Failed,
        "the member must stay exactly as the successor generation will find it"
    );
    fixture.cleanup();
}

/// The MemberRun CAS inside provider-profile preparation loses routinely to a
/// concurrent Host append (DEV-149-REVIEW-02). That is an attempt-scoped race:
/// it must classify as retryable and leave the member provisionable, not burn
/// the one-way door to Failed.
#[test]
fn a_lost_member_run_cas_is_retryable_and_the_member_still_provisions() {
    let fixture = JoinedMemberFixture::new("joined-member-fabric-lost-cas");
    let mut stale_projection = fixture.joined.clone();

    // A real concurrent Host append through the ordinary production writer,
    // which strands the projection this caller still holds.
    let mut advanced = fixture.joined.clone();
    advanced.last_event_at = Some("unix-ms:2".into());
    fixture
        .ledger
        .save_member_run(&fixture.joined, &advanced)
        .expect("concurrent Host append");

    let error = ensure_joined_member_runtime_fabric(&fixture.ledger, &mut stale_projection)
        .expect_err("a stale MemberRun projection loses the profile CAS");
    assert!(
        matches!(error, CliError::Store(_)),
        "a lost CAS must keep the typed Store error: {error}"
    );
    assert!(
        matches!(
            classify_member_fabric_failure(&error),
            MemberFabricFailure::Transient
        ),
        "a lost CAS is a property of the attempt, not a verdict on the member: {error}"
    );
    assert_ne!(
        fixture.latest_joined_status(),
        MemberRunStatus::Failed,
        "a lost race must never journal the member Failed"
    );

    // The retry the drive loop performs: re-read the member and provision it.
    let mut retried = fixture
        .ledger
        .latest_member_run(&fixture.joined.id)
        .expect("read the member back")
        .expect("member run");
    match fixture
        .provision(&mut retried)
        .expect("the retry provisions the member the lost race left alone")
    {
        JoinedMemberRuntimeFabric::Provisioned { .. } => {}
        JoinedMemberRuntimeFabric::AlreadyProvisioned => {
            panic!("the failed attempt must not have provisioned a session")
        }
    }
    assert_eq!(
        current_sessions(
            &fixture.store,
            &fixture.execution_space_id,
            &fixture.joined.agent_member_id
        )
        .len(),
        1
    );
    fixture.cleanup();
}

/// One live TeamRun with a founding roster, a live Supervisor lease, and one
/// member admitted into it by `team-run add-member`.
struct JoinedMemberFixture {
    store: HarnessStore,
    root: std::path::PathBuf,
    execution_space_id: String,
    lease: TeamSupervisorLease,
    ledger: TeamRunLedger,
    run: AgentTeamRun,
    joined: ProviderRuntimeProjection,
}

impl JoinedMemberFixture {
    fn new(tag: &str) -> Self {
        let (store, root) = temp_store(tag);
        let created = create_two_member_team_run(&store);
        let execution_space_id = team_run_execution_space_id(&store, &created.team_run)
            .expect("TeamRun Execution Space");
        let lease = store
            .acquire_test_supervisor_lease(
                &created.team_run.id,
                "supervisor-joined-member-fabric",
                std::process::id(),
                "test://joined-member-fabric",
                current_unix_ms_u64(),
                600_000,
            )
            .expect("acquire the live Supervisor lease");
        let ledger = TeamRunLedger::new(
            &store,
            &created.team_run.id,
            &lease.supervisor_id,
            lease.generation,
            Arc::new(AtomicBool::new(true)),
        );
        let late = TeamMemberSpec {
            agent_member_id: "agent-joined-late".into(),
            name: "JoinedLate".into(),
            role: "reviewer".into(),
            provider: "codex".into(),
            execution_mode: Some("codex_app_server".into()),
            model: None,
            effort: None,
            service_tier: None,
            provider_cwd_hint: None,
            owned_paths: vec!["crates/review".into()],
            resume_native_session_id: None,
            initial_work: None,
        };
        ensure_unit_test_canonical_members(
            &store,
            &execution_space_id,
            &created.team_run.agent_team_id,
            std::slice::from_ref(&late),
        )
        .expect("the joining AgentMember earns its durable TeamMembership first");
        let (run, joined, _) = add_team_run_member(&store, None, &created.team_run.id, &late, None)
            .expect("admit the member into the live run");
        assert!(
            member_needs_agent_session(&store, &execution_space_id, &joined)
                .expect("session inventory"),
            "the defect precondition is that add-member leaves the joined member sessionless"
        );
        Self {
            store,
            root,
            execution_space_id,
            lease,
            ledger,
            run,
            joined,
        }
    }

    /// A ledger for a Supervisor generation that no longer owns this TeamRun.
    fn stale_ledger(&self) -> TeamRunLedger {
        TeamRunLedger::new(
            &self.store,
            &self.run.id,
            "supervisor-joined-member-fabric-successor",
            self.lease.generation.saturating_add(1),
            Arc::new(AtomicBool::new(true)),
        )
    }

    /// The probe-free half. The full seam refreshes the provider profile by
    /// running the provider executable, which CI does not install.
    fn provision(
        &self,
        member: &mut ProviderRuntimeProjection,
    ) -> CliResult<JoinedMemberRuntimeFabric> {
        provision_member_agent_session(
            &self.ledger,
            &self.lease,
            &self.run,
            &self.execution_space_id,
            member,
        )
    }

    fn latest_joined_status(&self) -> MemberRunStatus {
        self.ledger
            .latest_member_run(&self.joined.id)
            .expect("read the member back")
            .expect("member run")
            .status
    }

    fn cleanup(self) {
        std::fs::remove_dir_all(self.root).expect("cleanup");
    }
}

fn current_sessions(
    store: &HarnessStore,
    execution_space_id: &str,
    agent_member_id: &str,
) -> Vec<harness_core::agentfirm_api::AgentSession> {
    store
        .fabric_agent_sessions(execution_space_id)
        .expect("canonical AgentSession fabric")
        .into_iter()
        .filter(|session| {
            session.agent_member_id == agent_member_id
                && session.lifecycle != harness_core::agentfirm_api::AgentSessionStatus::Closed
        })
        .collect()
}
