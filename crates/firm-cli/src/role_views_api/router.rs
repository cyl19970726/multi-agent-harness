use super::*;

pub(crate) fn handle_get(
    current: &HarnessStore,
    spaces: &[(String, HarnessStore)],
    current_space_id: &str,
    path: &str,
    target: &str,
    build_sha: &str,
    identity: Option<&ReadIdentity>,
) -> Option<HttpResponse> {
    if !path.starts_with("/v1/views/") {
        return None;
    }
    let query = match Query::parse(target) {
        Ok(value) => value,
        Err(detail) => return Some(error("400 Bad Request", "INVALID_QUERY", detail)),
    };
    let result = if path == "/v1/views/global-work" {
        global_work_view(spaces, &query)
    } else if path == "/v1/views/viewer-context" {
        viewer_context_view(current_space_id, current, identity)
    } else if let Some(team_id) = path.strip_prefix("/v1/views/team-workspace/") {
        team_view(
            current_space_id,
            current,
            team_id,
            false,
            identity,
            query.company.as_deref(),
        )
    } else if let Some(team_id) = path.strip_prefix("/v1/views/host-console/") {
        team_view(
            current_space_id,
            current,
            team_id,
            true,
            identity,
            query.company.as_deref(),
        )
    } else if let Some(team_id) = path.strip_prefix("/v1/views/team-inbox/") {
        team_inbox_view(current_space_id, current, team_id, &query, identity)
    } else if let Some(route_ref) = path.strip_prefix("/v1/views/agent-workspace/") {
        agent_workspace_view(current_space_id, current, route_ref, &query, identity)
    } else if let Some(member_run_id) = path.strip_prefix("/v1/views/member-workbench/") {
        member_view(
            current_space_id,
            current,
            member_run_id,
            identity,
            query.company.as_deref(),
        )
    } else if let Some(node_id) = path.strip_prefix("/v1/views/operator/") {
        operator_view(
            current_space_id,
            current,
            node_id,
            build_sha,
            identity,
            query.company.as_deref(),
        )
    } else {
        return Some(error(
            "404 Not Found",
            "ROLE_VIEW_NOT_FOUND",
            "unknown role view",
        ));
    };
    Some(match result {
        Ok(body) => HttpResponse {
            status: "200 OK",
            body,
        },
        Err((status, code, detail)) => error(status, code, detail),
    })
}

pub(crate) type ViewResult = Result<Value, (&'static str, &'static str, String)>;

/// In-process Global Work read for the CLI (`harness work list`). This is the
/// identical projection served at `/v1/views/global-work`; there is no second
/// aggregate implementation or writer.
pub(crate) fn global_work_view_json(
    spaces: &[(String, HarnessStore)],
    target: &str,
) -> Result<Value, String> {
    let query = Query::parse(target)?;
    global_work_view(spaces, &query).map_err(|(_, code, detail)| format!("{code}: {detail}"))
}

/// The one Global Work read projection (DOC-106): every canonical Work across
/// the provided Execution Space stores, keyed by durable Team/TeamMembership
/// identifiers, failing closed on cross-store Work id collisions. It never
/// writes and never folds a second ledger.
pub(crate) fn global_work_view(spaces: &[(String, HarnessStore)], query: &Query) -> ViewResult {
    let mut all = Vec::new();
    let mut max_sequence = 0;
    let mut identities = Vec::new();
    let mut snapshot_vector = Vec::new();
    let mut work_sources = BTreeMap::<String, String>::new();
    let mut facet_nodes = BTreeSet::new();
    let mut facet_hosts = BTreeSet::new();
    let mut facet_members = BTreeSet::new();
    let mut pending_migration = Vec::new();
    let mut ordered_spaces = spaces.iter().collect::<Vec<_>>();
    ordered_spaces.sort_by(|left, right| left.0.cmp(&right.0));
    let team_scope = query
        .values
        .get("team_id")
        .map(|values| values.iter().cloned().collect::<BTreeSet<_>>());
    for (space_id, store) in &ordered_spaces {
        let facts = Facts::read_for_teams(space_id, store, team_scope.as_ref())
            .map_err(|e| ("500 Internal Server Error", "ROLE_VIEW_BUILD_FAILED", e))?;
        max_sequence = max_sequence.max(facts.sequence);
        identities.push(facts.store_identity.clone());
        snapshot_vector.push(facts.snapshot_point());
        for work in &facts.works {
            if let Some(previous_space) = work_sources.insert(work.id.clone(), (*space_id).clone())
            {
                if previous_space != **space_id {
                    return Err((
                        "409 Conflict",
                        "WORK_ID_CONFLICT",
                        format!(
                            "Work {} exists in both {previous_space} and {space_id}",
                            work.id
                        ),
                    ));
                }
            }
            let Some(team) = work
                .accountable_team_id
                .as_deref()
                .and_then(|id| facts.teams.iter().find(|team| team.id == id))
                .or_else(|| {
                    facts
                        .runs
                        .iter()
                        .find(|run| run.id == work.team_run_id)
                        .and_then(|run| {
                            facts.teams.iter().find(|team| team.id == run.agent_team_id)
                        })
                })
            else {
                // A Work with no resolvable accountable Team is a pre-cutover
                // legacy row. It is never hidden silently: it surfaces in the
                // view's pending-migration list and skips item projection until
                // responsibility migration binds it to one durable Team.
                pending_migration.push(work.id.clone());
                continue;
            };
            let summary = work_summary(&facts, team, work);
            if !query.matches("team_id", Some(&team.id))
                || !query.matches("mission_id", Some(&team.mission_id))
                || !query.matches("node_id", Some(&team.node_id))
                || !query.matches("host_id", Some(&team.host_agent_id))
                || !query.matches("member_id", work.owner_member_id.as_deref())
                || !query.matches(
                    "assignee_membership_id",
                    work.assignee_membership_id.as_deref(),
                )
                || !query.matches("assignee_kind", summary["assignee_kind"].as_str())
                || !query.matches("phase", Some(&enum_string(&work.phase)))
                || !query.matches("condition", Some(&enum_string(&work.condition)))
                || !query.matches(
                    "resolution",
                    work.resolution.as_ref().map(enum_string).as_deref(),
                )
                || !query.matches("priority", Some(&enum_string(&work.priority)))
            {
                continue;
            }
            if !query.matches(
                "module_id",
                summary["module_refs"]
                    .as_array()
                    .and_then(|values| values.iter().find_map(Value::as_str)),
            ) && query.values.contains_key("module_id")
            {
                let wanted = &query.values["module_id"];
                if !summary["module_refs"].as_array().is_some_and(|values| {
                    values
                        .iter()
                        .filter_map(Value::as_str)
                        .any(|value| wanted.iter().any(|item| item == value))
                }) {
                    continue;
                }
            }
            let gate_states = ["passed", "failed", "pending", "waived", "stale"]
                .into_iter()
                .filter(|state| summary["gate_summary"][*state].as_u64().unwrap_or(0) > 0)
                .collect::<Vec<_>>();
            if let Some(wanted) = query.values.get("gate_state") {
                if !gate_states
                    .iter()
                    .any(|state| wanted.iter().any(|item| item == state))
                {
                    continue;
                }
            }
            let delegated = summary["delegation_summary"]["incoming"]
                .as_u64()
                .unwrap_or(0)
                > 0
                || summary["delegation_summary"]["outgoing"]
                    .as_u64()
                    .unwrap_or(0)
                    > 0;
            if query.delegated.is_some_and(|wanted| wanted != delegated) {
                continue;
            }
            let updated = work.updated_at.as_str();
            if query
                .values
                .get("updated_after")
                .is_some_and(|values| values.iter().all(|after| updated < after.as_str()))
                || query
                    .values
                    .get("updated_before")
                    .is_some_and(|values| values.iter().all(|before| updated >= before.as_str()))
            {
                continue;
            }
            facet_nodes.insert(team.node_id.clone());
            facet_hosts.insert(team.host_agent_id.clone());
            if let Some(member) = &work.owner_member_id {
                facet_members.insert(member.clone());
            }
            all.push(summary);
        }
    }
    all.sort_by(|a, b| {
        b["updated_at"]
            .as_str()
            .cmp(&a["updated_at"].as_str())
            .then_with(|| a["work_id"].as_str().cmp(&b["work_id"].as_str()))
    });
    let stable_hash = |value: &Value| {
        serde_json::to_vec(value)
            .unwrap_or_default()
            .into_iter()
            .fold(0xcbf29ce484222325_u64, |hash, byte| {
                (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3)
            })
    };
    let query_fingerprint = stable_hash(&json!({
        "schema": SCHEMA_VERSION,
        "filters": query.values,
        "delegated": query.delegated,
        "sort": "updated_at:desc,work_id:asc",
        "limit": query.limit,
    }));
    let snapshot_fingerprint =
        stable_hash(&serde_json::to_value(&snapshot_vector).map_err(|error| {
            (
                "500 Internal Server Error",
                "ROLE_VIEW_BUILD_FAILED",
                error.to_string(),
            )
        })?);
    let offset = if let Some(cursor) = &query.cursor {
        let parts = cursor.split(':').collect::<Vec<_>>();
        if parts.len() != 4
            || parts[0] != "rv1"
            || u64::from_str_radix(parts[1], 16).ok() != Some(query_fingerprint)
            || u64::from_str_radix(parts[2], 16).ok() != Some(snapshot_fingerprint)
        {
            return Err((
                "400 Bad Request",
                "INVALID_CURSOR",
                "cursor does not belong to this filter/sort/sequence".into(),
            ));
        }
        parts[3].parse::<usize>().map_err(|_| {
            (
                "400 Bad Request",
                "INVALID_CURSOR",
                "cursor offset is invalid".into(),
            )
        })?
    } else {
        0
    };
    let page_items = all
        .iter()
        .skip(offset)
        .take(query.limit)
        .cloned()
        .collect::<Vec<_>>();
    let next = (offset + page_items.len() < all.len()).then(|| {
        format!(
            "rv1:{query_fingerprint:016x}:{snapshot_fingerprint:016x}:{}",
            offset + page_items.len()
        )
    });
    let mut after_vector = Vec::new();
    for (space_id, store) in &ordered_spaces {
        after_vector.push(
            Facts::read(space_id, store)
                .map_err(|e| ("500 Internal Server Error", "ROLE_VIEW_BUILD_FAILED", e))?
                .snapshot_point(),
        );
    }
    if snapshot_vector != after_vector {
        return Err((
            "503 Service Unavailable",
            "SNAPSHOT_UNSTABLE",
            "Global Work sources changed during projection; retry the read".into(),
        ));
    }
    let facets = |field: &str| {
        let mut values = all
            .iter()
            .filter_map(|v| v.get(field).and_then(Value::as_str).map(str::to_owned))
            .collect::<Vec<_>>();
        values.sort();
        values.dedup();
        values
    };
    let facts = Facts {
        space_id: "global".into(),
        store_identity: identities.join("|"),
        sequence: max_sequence,
        work_sequence: 0,
        team_sequence: 0,
        run_sequence: 0,
        team_revisions: BTreeMap::new(),
        run_revisions: BTreeMap::new(),
        teams: vec![],
        runs: vec![],
        works: vec![],
        members: vec![],
        member_runs: vec![],
        provider_runtime_projections: vec![],
        messages: vec![],
        message_deliveries: vec![],
        agent_identities: vec![],
        agent_sessions: vec![],
        team_memberships: vec![],
        message_subscriptions: vec![],
        work_execution_bindings: vec![],
        work_execution_runtime_bindings: vec![],
        canonical_messages: vec![],
        canonical_message_deliveries: vec![],
        runtime_commands: vec![],
        work_deliveries: vec![],
        work_events: vec![],
        side: vec![],
    };
    pending_migration.sort();
    pending_migration.dedup();
    let migration_attention = if pending_migration.is_empty() {
        Vec::new()
    } else {
        vec![json!({
            "kind":"legacy_work_pending_migration",
            "severity":"warning",
            "source_ref":{"kind":"work","id":pending_migration.first().cloned().unwrap_or_default()},
            "reason_code":"work_missing_accountable_team",
            "first_seen_at":now(),
            "last_seen_at":now(),
            "recommended_action":"Run `harness team-run work migrate-responsibility` to bind legacy TeamRun-scoped Work to one durable Team; ambiguous rows fail closed for manual reconciliation",
        })]
    };
    Ok(envelope(
        "global_work",
        &facts,
        json!({"query":query.values,"sort":[{"field":"updated_at","direction":"desc"},{"field":"work_id","direction":"asc"}],"items":page_items,"page":{"as_of_event_sequence":max_sequence,"item_count":all.len(),"next_cursor":next,"snapshot_vector":snapshot_vector},"pending_migration_work_ids":pending_migration,"facets":{"teams":facets("team_id"),"missions":facets("mission_id"),"nodes":facet_nodes,"hosts":facet_hosts,"members":facet_members,"phases":facets("phase"),"conditions":facets("condition"),"resolutions":facets("resolution"),"modules":all.iter().flat_map(|v|v["module_refs"].as_array().into_iter().flatten()).filter_map(Value::as_str).collect::<BTreeSet<_>>(),"gate_states":["passed","failed","pending","waived","stale"]}}),
        migration_attention,
        vec![],
    ))
}

pub(crate) fn list_team_collaboration_delegations(
    store: &HarnessStore,
    company_id: &str,
    team_id: &str,
) -> Result<(u64, Vec<harness_core::collaboration::WorkDelegationV1>), String> {
    let source_filter = harness_store::CollaborationDelegationFilter {
        source_team_id: Some(team_id.to_string()),
        target_team_id: None,
        node_id: None,
        state: None,
    };
    let mut source_page = store
        .list_collaboration_delegations(company_id, &source_filter, None, 500)
        .map_err(|error| error.to_string())?;
    let as_of_store_sequence = source_page.as_of_store_sequence;
    let mut by_id = BTreeMap::new();
    loop {
        for delegation in source_page.items {
            by_id.insert(delegation.id.clone(), delegation);
        }
        let Some(cursor) = source_page.next_cursor else {
            break;
        };
        source_page = store
            .list_collaboration_delegations(company_id, &source_filter, Some(cursor), 500)
            .map_err(|error| error.to_string())?;
    }
    let target_filter = harness_store::CollaborationDelegationFilter {
        source_team_id: None,
        target_team_id: Some(team_id.to_string()),
        node_id: None,
        state: None,
    };
    let mut target_page = store
        .list_collaboration_delegations(
            company_id,
            &target_filter,
            Some(harness_store::CollaborationCursor {
                as_of_store_sequence,
                offset: 0,
            }),
            500,
        )
        .map_err(|error| error.to_string())?;
    loop {
        for delegation in target_page.items {
            by_id.insert(delegation.id.clone(), delegation);
        }
        let Some(cursor) = target_page.next_cursor else {
            break;
        };
        target_page = store
            .list_collaboration_delegations(company_id, &target_filter, Some(cursor), 500)
            .map_err(|error| error.to_string())?;
    }
    Ok((as_of_store_sequence, by_id.into_values().collect()))
}

pub(crate) fn collaboration_projection(
    company_id: Option<&str>,
    team_id: &str,
    member_work_ids: Option<&BTreeSet<String>>,
) -> Value {
    let Some(company_id) = company_id else {
        return json!({"state":"unavailable","reason":"Company scope is required"});
    };
    let result = (|| -> Result<Value, String> {
        let home = crate::execution_space::firm_home().map_err(|error| error.to_string())?;
        let layout = harness_store::remote_fabric_store::RemoteFabricStoreLayout::open(&home)
            .map_err(|error| error.to_string())?;
        let root = layout
            .collaboration_root(company_id)
            .map_err(|error| error.to_string())?;
        if !root.exists() {
            return Ok(json!({
                "company_id":company_id,
                "team_id":team_id,
                "state":"unavailable",
                "reason":"Company collaboration projection is not present on this server",
            }));
        }
        let store = HarnessStore::new(root);
        let (as_of_store_sequence, all_team_delegations) =
            list_team_collaboration_delegations(&store, company_id, team_id)?;
        let mut delegations = all_team_delegations
            .into_iter()
            .filter(|delegation| {
                member_work_ids.is_none_or(|work_ids| {
                    delegation.state == harness_core::collaboration::DelegationState::Active
                        && (work_ids.contains(&delegation.source_work_ref.work_id)
                            || delegation
                                .target_work_ref
                                .as_ref()
                                .is_some_and(|target| work_ids.contains(&target.work_id)))
                })
            })
            .collect::<Vec<_>>();
        delegations.sort_by(|left, right| {
            right
                .updated_at
                .cmp(&left.updated_at)
                .then_with(|| left.id.cmp(&right.id))
        });
        let pending_cancellations = delegations
            .iter()
            .map(|delegation| store.collaboration_cancellation_requests(company_id, &delegation.id))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())?
            .into_iter()
            .flatten()
            .filter(|request| {
                request.state == harness_core::collaboration::CancellationRequestState::Pending
                    && delegations
                        .iter()
                        .any(|delegation| delegation.id == request.delegation_id)
            })
            .collect::<Vec<_>>();
        let publication_count = delegations
            .iter()
            .map(|delegation| {
                store
                    .collaboration_publications(company_id, &delegation.id)
                    .map(|items| items.len())
                    .unwrap_or_default()
            })
            .sum::<usize>();
        let attention_count = delegations
            .iter()
            .filter(|delegation| {
                matches!(
                    delegation.state,
                    harness_core::collaboration::DelegationState::AwaitingTargetDecision
                        | harness_core::collaboration::DelegationState::ProvisioningTargetWork
                        | harness_core::collaboration::DelegationState::CancellationRequested
                )
            })
            .count()
            + pending_cancellations.len();
        Ok(json!({
            "company_id":company_id,
            "team_id":team_id,
            "state":"observed",
            "as_of_store_sequence":as_of_store_sequence,
            "delegation_count":delegations.len(),
            "delegations":delegations,
            "pending_cancellations":pending_cancellations,
            "publication_count":publication_count,
            "attention_count":attention_count,
        }))
    })();
    result.unwrap_or_else(|reason| {
        json!({
            "company_id":company_id,
            "team_id":team_id,
            "state":"unavailable",
            "reason":reason,
        })
    })
}
