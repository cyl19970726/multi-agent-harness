//! Member Work lifecycle fixtures.
//!
//! The local `team-run work claim|start|submit` verbs were retired: member Work
//! writes have exactly one authenticated entrance, the Supervisor-bound
//! `firm member work <verb>` Role Action. Integration tests that only need a
//! Work to reach a later lifecycle state (and are not themselves testing the
//! entrance) drive the same authoritative Store seams directly here, with the
//! same ProviderRuntimeProjection / AgentMember identities the Role Action
//! binds. These helpers never widen a Store gate — a fixture whose MemberRun
//! does not hold exact responsibility still fails.

use super::{unix_ms, TempHome};
use harness_application::{WorkAction, WorkApplication};
use harness_core::agentfirm_api::{
    ActorKind, ActorRef, CandidateKind, CandidateRef, Confidence, MutationContext, WorkReport,
    WorkReportKind,
};
use harness_core::{GitHubLink, TeamActorKind, TeamActorRef, Work, WorkCommandContext};

fn store_for(home: &TempHome, execution_space_id: &str) -> harness_store::HarnessStore {
    harness_store::HarnessStore::new(home.spaces_dir().join(execution_space_id))
}

fn member_context(member_run_id: &str, key: &str) -> WorkCommandContext {
    WorkCommandContext {
        event_id: format!("fixture-member-work-event:{key}"),
        performed_by_actor: TeamActorRef {
            kind: TeamActorKind::ProviderRuntimeProjection,
            id: member_run_id.to_string(),
            display_name: None,
            authn_source: Some("agentfirm_http_credential".to_string()),
        },
        authority_actor: None,
        causation_ref: None,
        idempotency_key: key.to_string(),
        created_at: format!("unix-ms:{}", unix_ms()),
        duplicate_ok: false,
    }
}

fn current_work(store: &harness_store::HarnessStore, work_id: &str) -> Work {
    store
        .latest_works()
        .expect("read fixture Works")
        .into_iter()
        .find(|work| work.id == work_id)
        .unwrap_or_else(|| panic!("fixture Work {work_id} exists"))
}

/// Create unassigned follow-up Work as the exact MemberRun, the same Store
/// seam the authenticated `member work create` Role Action reaches: the Work
/// carries the creator's own `created_by_member_id` and no responsibility.
#[allow(clippy::too_many_arguments)]
pub fn create_work_for_member_run(
    home: &TempHome,
    execution_space_id: &str,
    team_run_id: &str,
    accountable_team_id: &str,
    work_id: &str,
    title: &str,
    completion_criteria: &str,
    member_run_id: &str,
    key: &str,
) -> Work {
    let store = store_for(home, execution_space_id);
    WorkApplication::new(&store)
        .execute(WorkAction::Create(harness_application::CreateWorkCommand {
            work_id: work_id.to_string(),
            team_run_id: team_run_id.to_string(),
            accountable_team_id: accountable_team_id.to_string(),
            title: title.to_string(),
            context_markdown: String::new(),
            completion_criteria_markdown: completion_criteria.to_string(),
            claim_mode: harness_core::WorkClaimMode::TeamClaim,
            eligible_member_ids: Vec::new(),
            prerequisite_work_ids: Vec::new(),
            priority: harness_core::WorkPriority::Normal,
            artifact_refs: Vec::new(),
            check_refs: Vec::new(),
            github_links: Vec::new(),
            expected_version: 0,
            context: member_context(member_run_id, key),
        }))
        .expect("fixture member creates unassigned Work")
        .work
}

/// Claim an eligible Work as the exact MemberRun.
pub fn claim_work_for_member_run(
    home: &TempHome,
    execution_space_id: &str,
    work_id: &str,
    member_run_id: &str,
    key: &str,
) -> Work {
    let store = store_for(home, execution_space_id);
    let current = current_work(&store, work_id);
    WorkApplication::new(&store)
        .execute(WorkAction::Claim {
            work_id: work_id.to_string(),
            expected_version: current.version,
            member_run_id: member_run_id.to_string(),
            context: member_context(member_run_id, key),
        })
        .expect("fixture member claims Work")
        .work
}

/// Start assigned Work as the exact MemberRun holding its responsibility.
pub fn start_work_for_member_run(
    home: &TempHome,
    execution_space_id: &str,
    work_id: &str,
    member_run_id: &str,
    key: &str,
) -> Work {
    let store = store_for(home, execution_space_id);
    let current = current_work(&store, work_id);
    WorkApplication::new(&store)
        .execute(WorkAction::Start {
            work_id: work_id.to_string(),
            expected_version: current.version,
            member_run_id: member_run_id.to_string(),
            context: member_context(member_run_id, key),
        })
        .expect("fixture member starts Work")
        .work
}

/// Block active Work as the exact MemberRun holding its responsibility.
pub fn block_work_for_member_run(
    home: &TempHome,
    execution_space_id: &str,
    work_id: &str,
    member_run_id: &str,
    reason: &str,
    key: &str,
) -> Work {
    let store = store_for(home, execution_space_id);
    let current = current_work(&store, work_id);
    WorkApplication::new(&store)
        .execute(WorkAction::BlockMember {
            work_id: work_id.to_string(),
            expected_version: current.version,
            member_run_id: member_run_id.to_string(),
            reason: reason.to_string(),
            context: member_context(member_run_id, key),
        })
        .expect("fixture member blocks Work")
        .work
}

pub struct FixtureSubmission<'a> {
    pub result_summary: &'a str,
    pub artifact_refs: Vec<String>,
    pub check_refs: Vec<String>,
    /// #369: structured GitHub links are themselves the submitted evidence;
    /// their URLs join the refs and the candidate is derived from the
    /// submission content, exactly as the submit surface does.
    pub github_links: Vec<GitHubLink>,
    /// `None` with no GitHub link is an explicit report-only submission
    /// (DEV-214): the Work produced no commit, so the immutable Result
    /// carries no candidate.
    pub candidate_revision: Option<String>,
}

impl<'a> FixtureSubmission<'a> {
    pub fn report_only(result_summary: &'a str) -> Self {
        Self {
            result_summary,
            artifact_refs: Vec::new(),
            check_refs: Vec::new(),
            github_links: Vec::new(),
            candidate_revision: None,
        }
    }

    pub fn with_candidate(result_summary: &'a str, candidate_revision: &str) -> Self {
        Self {
            result_summary,
            artifact_refs: Vec::new(),
            check_refs: Vec::new(),
            github_links: Vec::new(),
            candidate_revision: Some(candidate_revision.to_string()),
        }
    }

    /// A submission whose evidence is one structured GitHub link (#369).
    pub fn with_github_link(result_summary: &'a str, link: GitHubLink) -> Self {
        Self {
            result_summary,
            artifact_refs: Vec::new(),
            check_refs: Vec::new(),
            github_links: vec![link],
            candidate_revision: None,
        }
    }
}

/// Submit the immutable Result of the current Work revision as the exact
/// owning AgentMember, exactly as the authenticated submit Role Action does.
pub fn submit_work_for_member_run(
    home: &TempHome,
    execution_space_id: &str,
    work_id: &str,
    member_run_id: &str,
    submission: FixtureSubmission<'_>,
    key: &str,
) -> Work {
    let mut submission = submission;
    // The submit surface merges a structured link's PR URL and checks URL into
    // the submitted refs before the report is built; mirror that exactly.
    for link in &submission.github_links {
        if !submission.artifact_refs.contains(&link.url) {
            submission.artifact_refs.push(link.url.clone());
        }
        if let Some(ci_url) = &link.ci_url {
            if !submission.check_refs.contains(ci_url) {
                submission.check_refs.push(ci_url.clone());
            }
        }
    }
    let candidate_revision = match (
        &submission.candidate_revision,
        submission.github_links.is_empty(),
    ) {
        (Some(revision), _) => Some(revision.clone()),
        (None, false) => Some(harness_store::canonical_work_candidate_revision(
            submission.result_summary,
            &submission.artifact_refs,
            &submission.check_refs,
            &submission.github_links,
        )),
        (None, true) => None,
    };
    let store = store_for(home, execution_space_id);
    let member = store
        .trust_member_runs(execution_space_id)
        .expect("read fixture MemberRuns")
        .into_iter()
        .find(|run| run.id == member_run_id)
        .unwrap_or_else(|| panic!("fixture MemberRun {member_run_id} exists"));
    let current = current_work(&store, work_id);
    let team_id = current
        .accountable_team_id
        .clone()
        .expect("fixture Work has an accountable AgentTeam");
    let actor = ActorRef {
        kind: ActorKind::AgentMember,
        id: member.agent_member_id.clone(),
    };
    let mut evidence_refs = submission
        .artifact_refs
        .iter()
        .chain(submission.check_refs.iter())
        .filter(|value| !value.trim().is_empty())
        .cloned()
        .collect::<Vec<_>>();
    if evidence_refs.is_empty() {
        evidence_refs.push(match &candidate_revision {
            Some(revision) => format!("work-candidate:{revision}"),
            None => "work-report-only".to_string(),
        });
    }
    let (candidate, candidate_fingerprint) = match &candidate_revision {
        Some(revision) => {
            let candidate = CandidateRef {
                kind: CandidateKind::GitCommit,
                value: revision.clone(),
            };
            let fingerprint = harness_store::canonical_json_fingerprint(
                &serde_json::to_value(&candidate).expect("candidate serializes"),
            );
            (Some(candidate), Some(fingerprint))
        }
        None => (None, None),
    };
    let report = WorkReport {
        id: format!("work-report:{key}"),
        work_id: work_id.to_string(),
        work_revision: current.version + 1,
        report_revision: store
            .trust_work_reports(execution_space_id)
            .expect("read fixture WorkReports")
            .into_iter()
            .filter(|report| report.work_id == work_id)
            .count() as u64
            + 1,
        kind: WorkReportKind::Result,
        authored_by: actor.clone(),
        summary: submission.result_summary.to_string(),
        base_revision: None,
        candidate,
        candidate_fingerprint,
        report_only: candidate_revision.is_none(),
        finding_refs: Vec::new(),
        failure_analysis_ref: None,
        artifact_refs: submission.artifact_refs,
        check_refs: submission.check_refs,
        github_links: submission.github_links,
        evidence_refs,
        known_risks: Vec::new(),
        confidence: Some(Confidence::High),
        recommended_next_action: Some("host_review".into()),
        created_at: format!("unix-ms:{}", unix_ms()),
    };
    store
        .create_trust_work_report(
            &MutationContext {
                execution_space_id: execution_space_id.into(),
                authenticated_actor: actor,
                authority_actor: None,
                command_name: "work_report.create".into(),
                idempotency_key: key.to_string(),
                expected_version: 0,
                request_fingerprint: None,
            },
            &team_id,
            report,
        )
        .expect("fixture member submits the immutable Work Result");
    current_work(&store, work_id)
}
