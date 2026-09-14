use super::*;

/// Host-owned Work sitting in normal Review, owned by the fixture Host's own
/// MemberRun, with its delivery already ProviderReceived. Returns the Host
/// MemberRun (the owner) and one ordinary peer MemberRun.
fn host_owned_review_work(
    store: &HarnessStore,
    run: &AgentTeamRun,
    host_run: &ProviderRuntimeProjection,
    work_id: &str,
) -> Work {
    let work = store
        .insert_work(
            unassigned_test_work(&run.id, work_id),
            host_work_context(
                &format!("{work_id}-we-1"),
                &format!("{work_id}-create"),
                "unix-ms:2",
            ),
        )
        .expect("create Work");
    let claimed = store
        .claim_work(
            &work.id,
            work.version,
            &host_run.id,
            member_work_context(
                &host_run.id,
                &format!("{work_id}-we-2"),
                &format!("{work_id}-claim"),
                "unix-ms:3",
            ),
        )
        .expect("Host runtime claims its own Work");
    let active = start_claimed_work_for_test(
        store,
        &claimed,
        host_run,
        &format!("{work_id}-we-3"),
        &format!("{work_id}-start"),
        "unix-ms:4",
    );
    let submitted = submit_started_work_for_test(
        store,
        &active,
        host_run,
        &format!("{work_id}-we-4"),
        "Host-owned candidate awaiting an independent reviewer",
        (vec![format!("artifact://{work_id}")], Vec::new()),
        "unix-ms:5",
    );
    assert_eq!(submitted.phase, WorkPhase::Review);
    assert_eq!(submitted.owner_member_id.as_deref(), Some("agent-host"));
    submitted
}

fn host_owned_review_fixture(
    name: &str,
) -> (
    PathBuf,
    HarnessStore,
    AgentTeamRun,
    ProviderRuntimeProjection,
    ProviderRuntimeProjection,
    Work,
) {
    let (root, store, run, peer, _other) = work_test_fixture(name);
    let host_run = store
        .member_runs()
        .expect("fixture MemberRuns")
        .into_iter()
        .rev()
        .find(|member| member.agent_member_id == "agent-host")
        .expect("fixture Host MemberRun");
    let submitted = host_owned_review_work(&store, &run, &host_run, &format!("{name}-work"));
    (root, store, run, host_run, peer, submitted)
}

fn peer_context(peer: &ProviderRuntimeProjection, suffix: &str) -> WorkCommandContext {
    member_work_context(
        &peer.id,
        &format!("we-peer-{suffix}"),
        &format!("peer-{suffix}"),
        "unix-ms:6",
    )
}

#[test]
fn exact_active_peer_returns_host_owned_work_and_reauthorizes_the_next_admission() {
    let (root, store, _run, _host_run, peer, submitted) =
        host_owned_review_fixture("peer-review-ok");

    // Control: the provider already received the previous revision, so the
    // next execution admission is fenced until review re-authorizes it.
    assert!(
        store
            .provider_received_work_requires_host_reauthorization(
                "unit-test-space",
                &submitted.id,
                submitted.version,
            )
            .expect("probe re-authorization before review"),
        "a provider-received revision must stay fenced before any review event"
    );

    let returned = store
        .request_work_changes_as_peer_reviewer(
            &submitted.id,
            submitted.version,
            "name the exact failing gate",
            &peer.id,
            peer_context(&peer, "ok"),
        )
        .expect("exact active non-owner Team peer returns Host-owned Work for changes");
    assert_eq!(returned.phase, WorkPhase::Open);
    assert_eq!(returned.condition, WorkCondition::Normal);
    assert_eq!(returned.resolution, None);
    assert_eq!(
        returned.blocker_reason.as_deref(),
        Some("name the exact failing gate")
    );
    assert_eq!(returned.version, submitted.version + 1);
    assert_eq!(
        returned.owner_member_id.as_deref(),
        Some("agent-host"),
        "peer review never moves responsibility"
    );
    let event = store
        .work_events()
        .expect("Work events")
        .into_iter()
        .rev()
        .find(|event| {
            event.work_id == submitted.id && event.kind == WorkEventKind::ChangesRequested
        })
        .expect("committed ChangesRequested event");
    // The performer is the durable AgentMember; the MemberRun that carried the
    // review is evidence beside it, not the identity.
    assert_eq!(
        event.performed_by_actor.kind,
        firm_core::TeamActorKind::AgentMember
    );
    assert_eq!(event.performed_by_actor.id, peer.agent_member_id);
    assert_eq!(event.executing_member_run_id(), Some(peer.id.as_str()));

    // A peer-performed ChangesRequested re-authorizes the next admission
    // exactly as the Host's does; otherwise the Work would be re-openable but
    // never re-bindable, and the next binding would fail closed until some
    // unrelated Host event happened to land.
    assert!(
        !store
            .provider_received_work_requires_host_reauthorization(
                "unit-test-space",
                &returned.id,
                returned.version,
            )
            .expect("probe re-authorization after peer review"),
        "peer request-changes must re-authorize the next execution admission"
    );
    std::fs::remove_dir_all(root).expect("remove temp store");
}

#[test]
fn peer_review_fails_closed_on_each_independent_mode_without_appending() {
    let (root, store, run, host_run, peer, submitted) =
        host_owned_review_fixture("peer-review-closed");
    let appended = || {
        store
            .work_operations_unlocked()
            .expect("Work operations")
            .len()
    };

    // 1. The performer must be the exact ProviderRuntimeProjection it names.
    let before = appended();
    let wrong_actor = store
        .request_work_changes_as_peer_reviewer(
            &submitted.id,
            submitted.version,
            "host actor on the peer path",
            &peer.id,
            host_work_context("we-peer-host-actor", "peer-host-actor", "unix-ms:6"),
        )
        .expect_err("a Host actor cannot travel the peer path");
    assert!(
        wrong_actor
            .to_string()
            .contains("trusted ProviderRuntimeProjection actor"),
        "{wrong_actor}"
    );
    assert_eq!(appended(), before);

    // 2. The accountable Work owner cannot review its own candidate.
    let before = appended();
    let owner = store
        .request_work_changes_as_peer_reviewer(
            &submitted.id,
            submitted.version,
            "reviewing myself",
            &host_run.id,
            peer_context(&host_run, "owner"),
        )
        .expect_err("the owner cannot review its own candidate");
    assert!(
        owner
            .to_string()
            .contains("owner cannot review its own candidate"),
        "{owner}"
    );
    assert_eq!(appended(), before);

    // 3. Ordinary Member-owned Work stays Host-reviewed.
    let member_work = store
        .insert_work(
            unassigned_test_work(&run.id, "peer-review-closed-member-work"),
            host_work_context("we-member-1", "create-member-work", "unix-ms:7"),
        )
        .expect("create Member Work");
    let claimed = store
        .claim_work(
            &member_work.id,
            member_work.version,
            &peer.id,
            member_work_context(&peer.id, "we-member-2", "claim-member-work", "unix-ms:8"),
        )
        .expect("peer claims its own Work");
    let active = start_claimed_work_for_test(
        &store,
        &claimed,
        &peer,
        "we-member-3",
        "start-member-work",
        "unix-ms:9",
    );
    let member_submitted = submit_started_work_for_test(
        &store,
        &active,
        &peer,
        "we-member-4",
        "member candidate",
        (vec!["artifact://member".into()], Vec::new()),
        "unix-ms:10",
    );
    let before = appended();
    let member_owned = store
        .request_work_changes_as_peer_reviewer(
            &member_submitted.id,
            member_submitted.version,
            "peer opinion on Member Work",
            &host_run.id,
            peer_context(&host_run, "member-owned"),
        )
        .expect_err("Member Work review authority stays with the exact Host");
    assert!(
        member_owned
            .to_string()
            .contains("only Host-owned Work admits a Team peer reviewer"),
        "{member_owned}"
    );
    assert_eq!(appended(), before);

    // 4. Only a Work awaiting acceptance can be returned. This one is
    // Host-owned and Active, so it passes every authority gate and fails only
    // on its phase.
    let open = store
        .insert_work(
            unassigned_test_work(&run.id, "peer-review-closed-active-work"),
            host_work_context("we-active-1", "create-active-work", "unix-ms:11"),
        )
        .expect("create a second Host-owned Work");
    let claimed_active = store
        .claim_work(
            &open.id,
            open.version,
            &host_run.id,
            member_work_context(
                &host_run.id,
                "we-active-2",
                "claim-active-work",
                "unix-ms:12",
            ),
        )
        .expect("Host runtime claims the second Work");
    let host_active = start_claimed_work_for_test(
        &store,
        &claimed_active,
        &host_run,
        "we-active-3",
        "start-active-work",
        "unix-ms:13",
    );
    assert_eq!(host_active.phase, WorkPhase::Active);
    let before = appended();
    let wrong_phase = store
        .request_work_changes_as_peer_reviewer(
            &host_active.id,
            host_active.version,
            "not in review",
            &peer.id,
            peer_context(&peer, "wrong-phase"),
        )
        .expect_err("only normal Review Work can be returned for changes");
    assert!(
        wrong_phase
            .to_string()
            .contains("must await Host acceptance"),
        "{wrong_phase}"
    );
    assert_eq!(appended(), before);

    // 5. Exactly one Active TeamMembership must bind the reviewer.
    let membership = store
        .fabric_team_memberships("unit-test-space")
        .expect("fixture memberships")
        .into_iter()
        .find(|membership| membership.agent_member_id == peer.agent_member_id)
        .expect("peer membership");
    store
        .leave_team_membership(
            &firm_core::agentfirm_api::MutationContext {
                execution_space_id: "unit-test-space".into(),
                authenticated_actor: firm_core::agentfirm_api::ActorRef {
                    kind: firm_core::agentfirm_api::ActorKind::AgentMember,
                    id: peer.agent_member_id.clone(),
                },
                authority_actor: None,
                command_name: "membership.leave".into(),
                idempotency_key: "peer-review-closed-leave".into(),
                expected_version: membership.revision,
                request_fingerprint: None,
            },
            &membership.id,
            "unix-ms:11",
        )
        .expect("deactivate the reviewer membership");
    let before = appended();
    let no_membership = store
        .request_work_changes_as_peer_reviewer(
            &submitted.id,
            submitted.version,
            "no longer a member",
            &peer.id,
            peer_context(&peer, "no-membership"),
        )
        .expect_err("a reviewer without exactly one Active membership is refused");
    assert!(
        no_membership
            .to_string()
            .contains("expected exactly one Active TeamMembership"),
        "{no_membership}"
    );
    assert_eq!(appended(), before);
    std::fs::remove_dir_all(root).expect("remove temp store");
}

/// A physical Store may temporarily hold more than one Execution Space during
/// recovery/import. Review authority is resolved in the Work's own TeamRun
/// space only: another scope's row neither grants it nor withdraws it.
#[test]
fn peer_review_counts_memberships_only_in_the_work_execution_space() {
    let (root, store, run, host_run, peer, first) = host_owned_review_fixture("peer-review-space");
    // Both candidates reach Review before the second Execution Space exists,
    // so every later assertion is about review authority alone.
    let second = host_owned_review_work(&store, &run, &host_run, "peer-review-space-second");
    let team = store
        .latest_teams()
        .expect("fixture teams")
        .remove(&run.agent_team_id)
        .expect("fixture AgentTeam");
    let creator = firm_core::agentfirm_api::ActorRef {
        kind: firm_core::agentfirm_api::ActorKind::Human,
        id: "fixture-host".into(),
    };
    let other_space = "other-unit-test-space";
    for (index, member_id) in team.member_ids.iter().enumerate() {
        store
            .create_trust_agent_member(
                &firm_core::agentfirm_api::MutationContext {
                    execution_space_id: other_space.into(),
                    authenticated_actor: creator.clone(),
                    authority_actor: None,
                    command_name: "agent_member.create".into(),
                    idempotency_key: format!("foreign-agent:{member_id}"),
                    expected_version: 0,
                    request_fingerprint: None,
                },
                firm_core::agentfirm_api::AgentMember {
                    id: member_id.clone(),
                    name: member_id.clone(),
                    description: "foreign Execution Space fixture".into(),
                    role: if index == 0 {
                        "host".into()
                    } else {
                        "builder".into()
                    },
                    capabilities: Vec::new(),
                    skill_refs: Vec::new(),
                    provider_profile_ref: None,
                    model_preference: None,
                    workspace_policy: "test".into(),
                    permission_ceiling: firm_core::agentfirm_api::PermissionCeiling::WorkspaceWrite,
                    organization_status:
                        firm_core::agentfirm_api::AgentMemberOrganizationStatus::Active,
                    version: 1,
                    created_by: creator.clone(),
                    created_at: "unix-ms:1".into(),
                    updated_at: "unix-ms:1".into(),
                },
            )
            .expect("seed the foreign Execution Space AgentMember");
    }
    let foreign_memberships = team
        .member_ids
        .iter()
        .enumerate()
        .map(
            |(index, member_id)| firm_core::agentfirm_api::TeamMembership {
                id: format!("membership:foreign:{member_id}"),
                team_id: team.id.clone(),
                agent_member_id: member_id.clone(),
                node_id: team.node_id.clone(),
                role: if index == 0 {
                    firm_core::agentfirm_api::TeamMembershipRole::Host
                } else {
                    firm_core::agentfirm_api::TeamMembershipRole::Member
                },
                state: firm_core::agentfirm_api::TeamMembershipStatus::Active,
                membership_generation: 1,
                default_subscription_refs: Vec::new(),
                created_by: creator.clone(),
                revision: 1,
                joined_at: "unix-ms:1".into(),
                left_at: None,
            },
        )
        .collect::<Vec<_>>();
    store
        .create_agent_team(
            &firm_core::agentfirm_api::MutationContext {
                execution_space_id: other_space.into(),
                authenticated_actor: creator.clone(),
                authority_actor: None,
                command_name: "agent_team.create".into(),
                idempotency_key: "foreign-space-team".into(),
                expected_version: 0,
                request_fingerprint: None,
            },
            team.clone(),
            foreign_memberships,
        )
        .expect("seed another Execution Space holding its own Active rows");

    // The reviewer now holds one Active row here and one over there. Folding
    // the scopes together would count two and refuse; the Work's own scope
    // still holds exactly one, so review stands.
    let returned = store
        .request_work_changes_as_peer_reviewer(
            &first.id,
            first.version,
            "another scope's rows are not this scope's truth",
            &peer.id,
            peer_context(&peer, "foreign-duplicate"),
        )
        .expect("another Execution Space's rows do not refuse review here");
    assert_eq!(returned.phase, WorkPhase::Open);

    // And a membership that is Active only over there grants nothing here.
    let membership = store
        .fabric_team_memberships("unit-test-space")
        .expect("fixture memberships")
        .into_iter()
        .find(|membership| membership.agent_member_id == peer.agent_member_id)
        .expect("peer membership");
    store
        .leave_team_membership(
            &firm_core::agentfirm_api::MutationContext {
                execution_space_id: "unit-test-space".into(),
                authenticated_actor: firm_core::agentfirm_api::ActorRef {
                    kind: firm_core::agentfirm_api::ActorKind::AgentMember,
                    id: peer.agent_member_id.clone(),
                },
                authority_actor: None,
                command_name: "membership.leave".into(),
                idempotency_key: "peer-review-space-leave".into(),
                expected_version: membership.revision,
                request_fingerprint: None,
            },
            &membership.id,
            "unix-ms:12",
        )
        .expect("deactivate the reviewer membership in the Work's own space");
    let before = store
        .work_operations_unlocked()
        .expect("Work operations")
        .len();
    let foreign_only = store
        .request_work_changes_as_peer_reviewer(
            &second.id,
            second.version,
            "membership lives only in another scope",
            &peer.id,
            peer_context(&peer, "foreign-only"),
        )
        .expect_err("a membership Active only in another Execution Space grants nothing");
    assert!(
        foreign_only
            .to_string()
            .contains("expected exactly one Active TeamMembership"),
        "{foreign_only}"
    );
    assert_eq!(
        store
            .work_operations_unlocked()
            .expect("Work operations")
            .len(),
        before
    );
    std::fs::remove_dir_all(root).expect("remove temp store");
}
