use super::*;
use harness_core::CurrentWorkDraft;

struct CliWorkActionOutcome(crate::work_action_service::CanonicalWorkActionOutcome);

impl std::ops::Deref for CliWorkActionOutcome {
    type Target = Work;

    fn deref(&self) -> &Self::Target {
        &self.0.work
    }
}

impl serde::Serialize for CliWorkActionOutcome {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.0.work.serialize(serializer)
    }
}

fn execute_work_action(
    store: &HarnessStore,
    action: harness_application::WorkAction,
) -> CliResult<CliWorkActionOutcome> {
    Ok(CliWorkActionOutcome(crate::work_action_service::execute(
        store,
        crate::work_action_service::CanonicalWorkCommand::Lifecycle {
            auth: None,
            action: Box::new(action),
        },
    )?))
}

fn local_work_auth(
    resolved: &ResolvedStore,
    actor: harness_core::agentfirm_api::ActorRef,
    authority: Option<harness_core::agentfirm_api::ActorRef>,
    context: &WorkCommandContext,
    expected_version: u64,
) -> CliResult<crate::agentfirm_api::AuthenticatedMutation> {
    let execution_space_id = resolved
        .execution_space_context
        .as_ref()
        .map(|space| space.id.clone())
        .ok_or_else(|| {
            CliError::Usage(
                "canonical Work mutations require an explicitly selected --space".into(),
            )
        })?;
    Ok(crate::agentfirm_api::AuthenticatedMutation {
        execution_space_id,
        actor,
        authorized_authority_actors: authority.into_iter().collect(),
        idempotency_key: context.idempotency_key.clone(),
        expected_version,
        request_fingerprint: None,
    })
}

pub(super) fn team_run_work_command(
    store: &HarnessStore,
    resolved: &ResolvedStore,
    args: &[String],
) -> CliResult<()> {
    require_subcommand(
        args,
        "team-run work list|show|create|replace-dependencies|delegate|delegation|assign|claim|start|block|resume|release|submit|request-changes|accept|cancel|retarget|reconcile-projection|migrate-responsibility|poll-github-ci",
    )?;
    if matches!(args[0].as_str(), "delegate" | "delegation") {
        return Err(CliError::Usage(
            "RETIRED_WRITE_AUTHORITY: local TeamRun WorkDelegation commands are retired; use the Company Control Plane collaboration API and collaboration_delegation MCP reads"
                .into(),
        ));
    }
    match args[0].as_str() {
        "list" => {
            let team_run_id = value(args, "--team-run-id");
            let team_id = value(args, "--team-id");
            if team_run_id.is_some() == team_id.is_some() {
                return Err(CliError::Usage(
                    "team-run work list requires exactly one of --team-run-id or --team-id"
                        .to_string(),
                ));
            }
            let phase = value(args, "--phase")
                .map(|raw| parse_work_phase(&raw))
                .transpose()?;
            let condition = value(args, "--condition")
                .map(|raw| parse_work_condition(&raw))
                .transpose()?;
            let resolution = value(args, "--resolution")
                .map(|raw| parse_work_resolution(&raw))
                .transpose()?;
            let member_run_id = value(args, "--member-run-id");
            let member_agent_id = member_run_id
                .as_deref()
                .map(|member_run_id| {
                    latest_member_runs_in_append_order(store)?
                        .into_iter()
                        .find(|member| member.id == member_run_id)
                        .map(|member| member.agent_member_id)
                        .ok_or_else(|| {
                            CliError::Usage(format!("member run not found: {member_run_id}"))
                        })
                })
                .transpose()?;
            // `--since <cursor>`: delta read against the WorkOperation append
            // order (see `work_operation_cursors` for why that order, and not
            // Work::version or updated_at, is the cursor). Independent of the
            // --status/--member-run-id value filters below: a Work can match
            // both, either, or neither.
            let since = value(args, "--since")
                .map(|raw| {
                    raw.parse::<u64>().map_err(|_| {
                        CliError::Usage(
                            "--since must be an integer WorkOperation-order cursor (pass the \
                             next_since a previous `work list --since` call returned)"
                                .to_string(),
                        )
                    })
                })
                .transpose()?;
            let brief = has_flag(args, "--brief");
            // The cursor is intentionally a per-TeamRun total order. A Team
            // can span several runs, so accepting `--since` with `--team-id`
            // would silently compare unrelated run-local positions.
            let cursors = if since.is_some() {
                let run_id = team_run_id.as_deref().ok_or_else(|| {
                    CliError::Usage(
                        "--since requires --team-run-id; Team-scoped list cursors are not yet a durable cross-run order"
                            .to_string(),
                    )
                })?;
                Some(work_operation_cursors(store, run_id)?)
            } else {
                None
            };
            let mut works = store
                .latest_works()?
                .into_iter()
                .filter(|work| {
                    team_run_id
                        .as_deref()
                        .is_some_and(|run_id| work.team_run_id == run_id)
                        || team_id
                            .as_deref()
                            .is_some_and(|id| work.accountable_team_id.as_deref() == Some(id))
                })
                .filter(|work| phase.is_none_or(|phase| work.phase == phase))
                .filter(|work| condition.is_none_or(|condition| work.condition == condition))
                .filter(|work| {
                    resolution.is_none_or(|resolution| work.resolution == Some(resolution))
                })
                .filter(|work| {
                    member_agent_id.as_deref().is_none_or(|member| {
                        work.owner_member_id.as_deref() == Some(member)
                    })
                })
                .filter(|work| {
                    since.is_none_or(|cursor| {
                        cursors
                            .as_ref()
                            .and_then(|cursors| cursors.get(&work.id))
                            .is_some_and(|sequence| *sequence > cursor)
                    })
                })
                .collect::<Vec<_>>();
            works.sort_by(|left, right| {
                work_priority_rank(right.priority)
                    .cmp(&work_priority_rank(left.priority))
                    .then_with(|| left.created_at.cmp(&right.created_at))
                    .then_with(|| left.id.cmp(&right.id))
            });
            if brief {
                // Plain text, one Work per line, no JSON wrapper: --since
                // still filters this list, but the next_since watermark below
                // is JSON-only (there is no room for a 6th field in the fixed
                // brief line shape without breaking its stable format).
                for work in &works {
                    println!("{}", format_work_brief_line(work));
                }
                Ok(())
            } else if let Some(since) = since {
                let next_since = cursors
                    .as_ref()
                    .and_then(|cursors| cursors.values().copied().max())
                    .unwrap_or(0)
                    .max(since);
                print_json(&serde_json::json!({
                    "since": since,
                    "next_since": next_since,
                    "works": works,
                }))
            } else {
                print_json(&works)
            }
        }
        "show" => {
            let work_id = required(args, "--work-id")?;
            let work = store
                .latest_works()?
                .into_iter()
                .find(|work| work.id == work_id)
                .ok_or_else(|| CliError::Usage(format!("Work not found: {work_id}")))?;
            let events = store
                .work_events()?
                .into_iter()
                .filter(|event| event.work_id == work_id)
                .collect::<Vec<_>>();
            let deliveries = store
                .current_work_deliveries_for_team_run(&work.team_run_id)?
                .into_iter()
                .filter(|delivery| delivery.work_id == work_id)
                .collect::<Vec<_>>();
            // GitHub linkage display (issue #369 Phase 2): render each stored
            // link, live-refreshing state/CI through `gh` when available.
            // `source` distinguishes a fresh observation from the stored
            // snapshot so a reader never mistakes stale data for live state.
            let mut github_links = Vec::new();
            for link in &work.github_links {
                let raw = format!("{}/{}#{}", link.owner, link.repo, link.number);
                let live = match link.kind {
                    GitHubLinkKind::Issue => github_issue_link(&raw).ok(),
                    GitHubLinkKind::PullRequest => github_pr_link(&raw).ok(),
                };
                let shown = live.as_ref().unwrap_or(link);
                github_links.push(serde_json::json!({
                    "kind": shown.kind,
                    "owner": shown.owner,
                    "repo": shown.repo,
                    "number": shown.number,
                    "url": shown.url,
                    "status": shown.status,
                    "ci_status": shown.ci_status,
                    "ci_url": shown.ci_url,
                    "source": if live.is_some() { "live" } else { "snapshot" },
                }));
            }
            print_json(&serde_json::json!({
                "work": work,
                "events": events,
                "deliveries": deliveries,
                "github_links": github_links,
            }))
        }
        "delegate" => {
            let source_run_id = required(args, "--team-run-id")?;
            let source_work_id = required(args, "--work-id")?;
            let expected_version = required_work_version(args)?;
            let target_team_id = required(args, "--target-team-id")?;
            let source = store
                .latest_works()?
                .into_iter()
                .find(|work| work.id == source_work_id)
                .ok_or_else(|| CliError::Usage(format!("Work not found: {source_work_id}")))?;
            if source.team_run_id != source_run_id {
                return Err(CliError::Usage(format!(
                    "Work {source_work_id} belongs to TeamRun {}, not {source_run_id}",
                    source.team_run_id
                )));
            }
            let source_owner = source.owner_member_id.clone().ok_or_else(|| {
                CliError::Usage("DELEGATION_NOT_AUTHORIZED: source Work has no durable owner".to_string())
            })?;
            let target_runs = latest_team_runs_in_append_order(store)?
                .into_iter()
                .filter(|run| run.agent_team_id == target_team_id)
                .filter(|run| {
                    !matches!(
                        run.status,
                        TeamRunStatus::Completed | TeamRunStatus::Failed | TeamRunStatus::Cancelled
                    )
                })
                .collect::<Vec<_>>();
            if target_runs.len() != 1 {
                return Err(CliError::Usage(format!(
                    "DELEGATION_TARGET_INVALID: target Team {target_team_id} must have exactly one active TeamRun, found {}",
                    target_runs.len()
                )));
            }
            let target_run = &target_runs[0];
            let context = if let Some(member_run_id) = value(args, "--member-run-id") {
                member_work_context(args, &source_run_id, &member_run_id)?
            } else {
                host_work_context(store, &source_run_id, args)?
            };
            let now = context.created_at.clone();
            let request_hash = content_hash_hex16(&context.idempotency_key);
            let target_work_id = value(args, "--target-work-id")
                .unwrap_or_else(|| format!("delegated-work-{request_hash}"));
            let target_work = CurrentWorkDraft::new(
                target_work_id.clone(),
                target_run.id.clone(),
                target_team_id.clone(),
                required(args, "--target-title")?,
                required(args, "--target-context")?,
                required(args, "--target-completion-criteria")?,
                WorkClaimMode::TeamClaim,
                source.priority,
                context.performed_by_actor.clone(),
                now.clone(),
            )
            .into_work();
            let delegation = WorkDelegation {
                id: value(args, "--delegation-id")
                    .unwrap_or_else(|| format!("work-delegation-{request_hash}")),
                source_work_ref: WorkRef {
                    team_run_id: source_run_id,
                    work_id: source_work_id,
                },
                source_work_version: expected_version,
                source_owner_member_id: source_owner,
                created_by_member_run_id: None,
                target_agent_team_id: target_team_id,
                target_work_ref: WorkRef {
                    team_run_id: target_run.id.clone(),
                    work_id: target_work_id,
                },
                delegated_by_actor: context.performed_by_actor.clone(),
                state: WorkDelegationState::Active,
                resolution_summary: None,
                blocker_reason: None,
                version: 1,
                created_at: now.clone(),
                updated_at: now,
            };
            let (delegation, target_work) = store
                .create_work_delegation_with_target_work(delegation, target_work, context)?;
            print_json(&serde_json::json!({
                "delegation": delegation,
                "target_work": target_work,
            }))
        }
        "delegation" => {
            require_subcommand(args, "team-run work delegation list|show|cancel")?;
            match args[1].as_str() {
                "list" => {
                    let source_work_id = value(args, "--source-work-id");
                    let target_team_id = value(args, "--target-team-id");
                    let state = value(args, "--state");
                    let delegations = store
                        .latest_work_delegations()?
                        .into_iter()
                        .filter(|delegation| {
                            source_work_id.as_deref().is_none_or(|id| {
                                delegation.source_work_ref.work_id == id
                            })
                        })
                        .filter(|delegation| {
                            target_team_id.as_deref().is_none_or(|id| {
                                delegation.target_agent_team_id == id
                            })
                        })
                        .filter(|delegation| {
                            state.as_deref().is_none_or(|state| {
                                serde_snake_label(&delegation.state) == state
                            })
                        })
                        .collect::<Vec<_>>();
                    print_json(&delegations)
                }
                "show" => {
                    let id = required(args, "--delegation-id")?;
                    let delegation = store
                        .latest_work_delegations()?
                        .into_iter()
                        .find(|delegation| delegation.id == id)
                        .ok_or_else(|| CliError::Usage(format!("Delegation not found: {id}")))?;
                    let events = store
                        .work_delegation_events()?
                        .into_iter()
                        .filter(|event| event.delegation_id == id)
                        .collect::<Vec<_>>();
                    print_json(&serde_json::json!({"delegation": delegation, "events": events}))
                }
                "cancel" => {
                    let delegation = store.cancel_work_delegation(
                        &required(args, "--delegation-id")?,
                        required_work_version(args)?,
                        &required(args, "--reason")?,
                        host_work_context(store, &required(args, "--team-run-id")?, args)?,
                    )?;
                    print_json(&delegation)
                }
                other => Err(CliError::Usage(format!(
                    "unknown delegation command: {other}"
                ))),
            }
        }
        "create" => {
            if value(args, "--owner-member-run-id").is_some() {
                return Err(CliError::Usage(
                    "create-time MemberRun ownership is retired; create Work, then assign one canonical TeamMembership"
                        .into(),
                ));
            }
            let team_run_id = required(args, "--team-run-id")?;
            let run = latest_team_run(store, &team_run_id)?;
            let acting_member_run_id = value(args, "--as-member-run-id");
            let context = if let Some(member_run_id) = acting_member_run_id.as_deref() {
                member_work_context(args, &team_run_id, member_run_id)?
            } else {
                host_work_context(store, &team_run_id, args)?
            };
            let claim_mode = value(args, "--claim-mode")
                .map(|raw| parse_work_claim_mode(&raw))
                .transpose()?
                .unwrap_or(WorkClaimMode::TeamClaim);
            // `--github-issue owner/repo#N` links the Work to a GitHub issue;
            // `--github-pr owner/repo#N` links it to a pull request (issue
            // #369). Both auto-populate artifact_refs (object URL) and PR
            // links also populate check_refs (CI checks URL). A create-time
            // PR link is what lets the daemon poll CI on the open/in-progress
            // Work and auto-submit when the PR merges (Phase 2).
            let mut github_links = Vec::new();
            let mut artifact_refs = Vec::new();
            let mut check_refs = Vec::new();
            if let Some(raw) = value(args, "--github-issue") {
                let link = github_issue_link(&raw)?;
                if !artifact_refs.contains(&link.url) {
                    artifact_refs.push(link.url.clone());
                }
                github_links.push(link);
            }
            if let Some(raw) = value(args, "--github-pr") {
                let link = github_pr_link(&raw)?;
                if !artifact_refs.contains(&link.url) {
                    artifact_refs.push(link.url.clone());
                }
                if let Some(ci_url) = &link.ci_url {
                    if !check_refs.contains(ci_url) {
                        check_refs.push(ci_url.clone());
                    }
                }
                github_links.push(link);
            }
            let context_markdown = value(args, "--context").unwrap_or_default();
            let work = execute_work_action(
                store,
                harness_application::WorkAction::Create(harness_application::CreateWorkCommand {
                work_id: value(args, "--work-id").unwrap_or_else(|| generated_id("work")),
                team_run_id,
                accountable_team_id: run.agent_team_id,
                title: required(args, "--title")?,
                context_markdown,
                completion_criteria_markdown: required(args, "--completion-criteria")?,
                claim_mode,
                eligible_member_ids: many(args, "--eligible-member-id"),
                prerequisite_work_ids: many(args, "--prerequisite-work-id"),
                priority: value(args, "--priority")
                    .map(|raw| parse_work_priority(&raw))
                    .transpose()?
                    .unwrap_or(WorkPriority::Normal),
                artifact_refs,
                check_refs,
                github_links,
                expected_version: 0,
                context,
                }),
            )?;
            print_json(&work)
        }
        "replace-dependencies" => {
            let work_id = required(args, "--work-id")?;
            let work = execute_work_action(
                store,
                harness_application::WorkAction::ReplaceDependencies(
                    harness_application::ReplaceWorkDependenciesCommand {
                    accountable_team_id: required(args, "--team-id")?,
                    work_id: work_id.clone(),
                    expected_version: required_work_version(args)?,
                    prerequisite_work_ids: many(args, "--prerequisite-work-id"),
                    context: host_work_context_for_work(store, &work_id, args)?,
                    },
                ),
            )?;
            print_json(&work)
        }
        "assign" => {
            if value(args, "--member-run-id").is_some() {
                return Err(CliError::Usage(
                    "runtime-bound Work assignment is retired; use --membership-id".into(),
                ));
            }
            let membership_id = required(args, "--membership-id")?;
            let space_id = resolved
                .execution_space_context
                .as_ref()
                .map(|space| space.id.clone())
                .ok_or_else(|| {
                    CliError::Usage(
                        "membership assignment requires an explicitly selected --space".to_string(),
                    )
                })?;
            let work_id = required(args, "--work-id")?;
            let work = execute_work_action(
                store,
                harness_application::WorkAction::AssignMembership {
                    expected_version: required_work_version(args)?,
                    context: host_work_context_for_work(store, &work_id, args)?,
                    work_id,
                    membership_id: membership_id.clone(),
                    execution_space_id: space_id,
                },
            )?;
            append_work_event(
                store,
                &work,
                TeamRunEventSourceKind::Host,
                None,
                "assigned",
                &format!("Work assigned to TeamMembership {membership_id}"),
            )?;
            print_json(&work)
        }
        "migrate-responsibility" => {
            let space_id = resolved
                .execution_space_context
                .as_ref()
                .map(|space| space.id.clone())
                .ok_or_else(|| {
                    CliError::Usage(
                        "responsibility migration requires an explicitly selected --space"
                            .to_string(),
                    )
                })?;
            let report =
                store.migrate_work_responsibility(&space_id, migration_host_work_context(args))?;
            print_json(&report)
        }
        "claim" => {
            let team_run_id = required(args, "--team-run-id")?;
            let member_run_id = required(args, "--member-run-id")?;
            let work = execute_work_action(
                store,
                harness_application::WorkAction::Claim {
                    work_id: required(args, "--work-id")?,
                    expected_version: required_work_version(args)?,
                    member_run_id: member_run_id.clone(),
                    context: member_work_context(args, &team_run_id, &member_run_id)?,
                },
            )?;
            append_work_event(
                store,
                &work,
                TeamRunEventSourceKind::Member,
                Some(member_run_id.clone()),
                "claimed",
                &format!("Work claimed by {member_run_id}"),
            )?;
            print_json(&work)
        }
        "start" => {
            let team_run_id = required(args, "--team-run-id")?;
            let member_run_id = required(args, "--member-run-id")?;
            let work = execute_work_action(
                store,
                harness_application::WorkAction::Start {
                    work_id: required(args, "--work-id")?,
                    expected_version: required_work_version(args)?,
                    member_run_id: member_run_id.clone(),
                    context: member_work_context(args, &team_run_id, &member_run_id)?,
                },
            )?;
            append_work_event(
                store,
                &work,
                TeamRunEventSourceKind::Member,
                Some(member_run_id.clone()),
                "started",
                &format!("Work started by {member_run_id}"),
            )?;
            print_json(&work)
        }
        "block" => {
            let team_run_id = required(args, "--team-run-id")?;
            let work_id = required(args, "--work-id")?;
            let expected_version = required_work_version(args)?;
            let reason = required(args, "--reason")?;
            if let Some(member_run_id) = value(args, "--member-run-id") {
                let work = execute_work_action(
                    store,
                    harness_application::WorkAction::BlockMember {
                        work_id: work_id.clone(),
                        expected_version,
                        member_run_id: member_run_id.clone(),
                        reason: reason.clone(),
                        context: member_work_context(args, &team_run_id, &member_run_id)?,
                    },
                )?;
                append_work_event(
                    store,
                    &work,
                    TeamRunEventSourceKind::Member,
                    Some(member_run_id.clone()),
                    "blocked",
                    &format!("Work blocked by {member_run_id}: {reason}"),
                )?;
                roll_up_target_work_delegations(store, &work, args)?;
                print_json(&work)
            } else {
                let work = execute_work_action(
                    store,
                    harness_application::WorkAction::BlockHost {
                        work_id: work_id.clone(),
                        expected_version,
                        reason: reason.clone(),
                        context: host_work_context(store, &team_run_id, args)?,
                    },
                )?;
                append_work_event(
                    store,
                    &work,
                    TeamRunEventSourceKind::Host,
                    None,
                    "blocked",
                    &format!("Work blocked by host: {reason}"),
                )?;
                roll_up_target_work_delegations(store, &work, args)?;
                print_json(&work)
            }
        }
        "resume" => {
            let team_run_id = required(args, "--team-run-id")?;
            let work_id = required(args, "--work-id")?;
            let expected_version = required_work_version(args)?;
            let resolution = required(args, "--resolution")?;
            if let Some(member_run_id) = value(args, "--member-run-id") {
                let work = execute_work_action(
                    store,
                    harness_application::WorkAction::ResumeMember {
                        work_id: work_id.clone(),
                        expected_version,
                        member_run_id: member_run_id.clone(),
                        resolution: resolution.clone(),
                        context: member_work_context(args, &team_run_id, &member_run_id)?,
                    },
                )?;
                append_work_event(
                    store,
                    &work,
                    TeamRunEventSourceKind::Member,
                    Some(member_run_id.clone()),
                    "resumed",
                    &format!("Work resumed by {member_run_id}: {resolution}"),
                )?;
                roll_up_target_work_delegations(store, &work, args)?;
                print_json(&work)
            } else {
                let work = execute_work_action(
                    store,
                    harness_application::WorkAction::ResumeHost {
                        work_id: work_id.clone(),
                        expected_version,
                        resolution: resolution.clone(),
                        context: host_work_context(store, &team_run_id, args)?,
                    },
                )?;
                append_work_event(
                    store,
                    &work,
                    TeamRunEventSourceKind::Host,
                    None,
                    "resumed",
                    &format!("Work resumed by host: {resolution}"),
                )?;
                roll_up_target_work_delegations(store, &work, args)?;
                print_json(&work)
            }
        }
        "release" => {
            let team_run_id = required(args, "--team-run-id")?;
            let work_id = required(args, "--work-id")?;
            let expected_version = required_work_version(args)?;
            if let Some(member_run_id) = value(args, "--member-run-id") {
                let work = execute_work_action(
                    store,
                    harness_application::WorkAction::ReleaseMember {
                        work_id: work_id.clone(),
                        expected_version,
                        member_run_id: member_run_id.clone(),
                        context: member_work_context(args, &team_run_id, &member_run_id)?,
                    },
                )?;
                append_work_event(
                    store,
                    &work,
                    TeamRunEventSourceKind::Member,
                    Some(member_run_id.clone()),
                    "released",
                    &format!("Work released by {member_run_id}"),
                )?;
                print_json(&work)
            } else {
                let work = execute_work_action(
                    store,
                    harness_application::WorkAction::ReleaseHost {
                        work_id: work_id.clone(),
                        expected_version,
                        context: host_work_context(store, &team_run_id, args)?,
                    },
                )?;
                append_work_event(
                    store,
                    &work,
                    TeamRunEventSourceKind::Host,
                    None,
                    "released",
                    "Work released by host",
                )?;
                print_json(&work)
            }
        }
        "submit" => {
            let team_run_id = required(args, "--team-run-id")?;
            let member_run_id = required(args, "--member-run-id")?;
            // `--github-pr owner/repo#N` attaches the PR to the submission,
            // auto-fetches its CI status via the `gh` API, and auto-populates
            // artifact_refs (PR URL) + check_refs (CI checks URL) (issue #369).
            let mut artifact_refs = many(args, "--artifact-ref");
            let mut check_refs = many(args, "--check-ref");
            let mut github_links = Vec::new();
            let result = required(args, "--result")?;
            if let Some(raw) = value(args, "--github-pr") {
                let link = github_pr_link(&raw)?;
                if !artifact_refs.contains(&link.url) {
                    artifact_refs.push(link.url.clone());
                }
                if let Some(ci_url) = &link.ci_url {
                    if !check_refs.contains(ci_url) {
                        check_refs.push(ci_url.clone());
                    }
                }
                github_links.push(link);
            }
            let work_id = required(args, "--work-id")?;
            let expected_version = required_work_version(args)?;
            let context = member_work_context(args, &team_run_id, &member_run_id)?;
            let execution_space_id = resolved
                .execution_space_context
                .as_ref()
                .map(|space| space.id.clone())
                .ok_or_else(|| {
                    CliError::Usage(
                        "canonical Work submission requires an explicitly selected --space".into(),
                    )
                })?;
            let member = store
                .trust_member_runs(&execution_space_id)?
                .into_iter()
                .find(|run| run.id == member_run_id)
                .ok_or_else(|| {
                    CliError::Usage(format!("MemberRun not found: {member_run_id}"))
                })?;
            let current = crate::work_action_service::current_work(
                store,
                &execution_space_id,
                &work_id,
            )?;
            let team_id = current.accountable_team_id.clone().ok_or_else(|| {
                CliError::Usage("canonical Work has no accountable Team".into())
            })?;
            let auth = local_work_auth(
                resolved,
                harness_core::agentfirm_api::ActorRef {
                    kind: harness_core::agentfirm_api::ActorKind::AgentMember,
                    id: member.agent_member_id,
                },
                None,
                &context,
                expected_version,
            )?;
            let outcome = crate::work_action_service::execute(
                store,
                crate::work_action_service::CanonicalWorkCommand::SubmitResult {
                    auth,
                    team_id,
                    work_id: work_id.clone(),
                    submission: crate::work_action_service::ResultSubmission {
                        result_summary: required(args, "--result")?,
                        artifact_refs,
                        check_refs,
                        github_links,
                        base_revision: value(args, "--base-revision"),
                        candidate_revision: value(args, "--candidate-revision"),
                    },
                },
            )?;
            let work = outcome.work;
            append_work_event(
                store,
                &work,
                TeamRunEventSourceKind::Member,
                Some(member_run_id.clone()),
                "submitted",
                &format!("Work submitted by {member_run_id}: {result}"),
            )?;
            print_json(&work)
        }
        "poll-github-ci" => {
            let team_run_id = required(args, "--team-run-id")?;
            let summary = poll_team_run_github_linkages(store, &team_run_id)?;
            print_json(&serde_json::json!({
                "team_run_id": team_run_id,
                "works_checked": summary.works_checked,
                "links_refreshed": summary.links_refreshed,
                "auto_submitted": summary.auto_submitted,
                "blocked_on_failure": summary.blocked_on_failure,
                "gate_ready": summary.gate_ready,
                "gh_unavailable": summary.gh_unavailable,
            }))
        }
        "request-changes" => {
            let reason = required(args, "--reason")?;
            let work_id = required(args, "--work-id")?;
            let work = execute_work_action(
                store,
                harness_application::WorkAction::RequestChanges {
                    work_id: work_id.clone(),
                    expected_version: required_work_version(args)?,
                    reason: reason.clone(),
                    context: host_work_context_for_work(store, &work_id, args)?,
                },
            )?;
            append_work_event(
                store,
                &work,
                TeamRunEventSourceKind::Host,
                None,
                "changes_requested",
                &format!("Changes requested: {reason}"),
            )?;
            print_json(&work)
        }
        "accept" => {
            let work_id = required(args, "--work-id")?;
            let expected_version = required_work_version(args)?;
            if has_flag(args, "--skip-gates") {
                return Err(CliError::Usage(
                    "--skip-gates is retired: declared Work gates are a Store invariant and cannot be bypassed"
                        .to_string(),
                ));
            }
            let context = host_work_context_for_work(store, &work_id, args)?;
            let execution_space_id = resolved
                .execution_space_context
                .as_ref()
                .map(|space| space.id.clone())
                .ok_or_else(|| {
                    CliError::Usage(
                        "canonical Work acceptance requires an explicitly selected --space".into(),
                    )
                })?;
            let current = crate::work_action_service::current_work(
                store,
                &execution_space_id,
                &work_id,
            )?;
            let team_id = current.accountable_team_id.clone().ok_or_else(|| {
                CliError::Usage("canonical Work has no accountable Team".into())
            })?;
            let host = harness_core::agentfirm_api::ActorRef {
                kind: harness_core::agentfirm_api::ActorKind::AgentMember,
                id: context.performed_by_actor.id.clone(),
            };
            let auth = local_work_auth(
                resolved,
                host.clone(),
                Some(host),
                &context,
                expected_version,
            )?;
            let outcome = crate::work_action_service::execute(
                store,
                crate::work_action_service::CanonicalWorkCommand::Accept {
                    auth,
                    team_id,
                    work_id: work_id.clone(),
                },
            )?;
            let work = outcome.work;
            append_work_event(
                store,
                &work,
                TeamRunEventSourceKind::Host,
                None,
                "accepted",
                &format!("Work accepted: {}", work.title),
            )?;
            roll_up_target_work_delegations(store, &work, args)?;
            print_json(&work)
        }
        "cancel" => {
            let reason = required(args, "--reason")?;
            let work_id = required(args, "--work-id")?;
            let work = execute_work_action(
                store,
                harness_application::WorkAction::Cancel {
                    work_id: work_id.clone(),
                    expected_version: required_work_version(args)?,
                    reason: reason.clone(),
                    context: host_work_context_for_work(store, &work_id, args)?,
                },
            )?;
            append_work_event(
                store,
                &work,
                TeamRunEventSourceKind::Host,
                None,
                "cancelled",
                &format!("Work cancelled: {reason}"),
            )?;
            roll_up_target_work_delegations(store, &work, args)?;
            print_json(&work)
        }
        "retarget" => {
            let work_id = required(args, "--work-id")?;
            print_json(&store.retarget_work_execution(
                &work_id,
                required_work_version(args)?,
                &required(args, "--successor-team-run-id")?,
                value(args, "--successor-member-run-id").as_deref(),
                host_work_context_for_work(store, &work_id, args)?,
            )?)
        }
        "reconcile-projection" => {
            let work_id = required(args, "--work-id")?;
            print_json(&store.reconcile_work_projection_provenance(
                &work_id,
                required_work_version(args)?,
                host_work_context_for_work(store, &work_id, args)?,
            )?)
        }
        other => Err(CliError::Usage(format!(
            "unknown team-run work command: {other}; usage: team-run work list|show|create|assign|claim|start|block|resume|release|submit|review|request-changes|accept|cancel|retarget|reconcile-projection"
        ))),
    }
}
