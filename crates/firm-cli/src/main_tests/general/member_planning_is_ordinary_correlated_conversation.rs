use super::*;

#[cfg(any())] // Historical CLI-send planning conversation; canonical RoleAction messaging is covered store-live.
fn member_planning_is_ordinary_correlated_conversation() {
    let (store, root) = temp_store("member-plan-conversation");
    let created = create_two_member_team_run(&store);
    let member = &created.member_runs[0];
    let assignment = seed_host_conversation(&store, &created, 0);

    let request = send_team_message(
        &store,
        &created.team_run.id,
        "host",
        vec![member.id.clone()],
        ProviderDispatchIntent::Message,
        "Return a Markdown plan before implementation. Do not execute yet.",
        Some(assignment.correlation_id.clone()),
        Some(assignment.id.clone()),
        None,
        None,
    )
    .expect("Host requests plan");
    let proposal_one = send_team_message(
        &store,
        &created.team_run.id,
        &member.id,
        vec!["host".into()],
        ProviderDispatchIntent::Message,
        "1. Inspect\n2. Implement\n3. Verify",
        Some(assignment.correlation_id.clone()),
        Some(request.id.clone()),
        None,
        None,
    )
    .expect("owner proposes");
    let feedback = send_team_message(
        &store,
        &created.team_run.id,
        "host",
        vec![member.id.clone()],
        ProviderDispatchIntent::Message,
        "Add rollback and integration checks",
        Some(assignment.correlation_id.clone()),
        Some(proposal_one.id.clone()),
        None,
        None,
    )
    .expect("Host challenges proposal");

    let proposal_two = send_team_message(
        &store,
        &created.team_run.id,
        &member.id,
        vec!["host".into()],
        ProviderDispatchIntent::Message,
        "1. Inspect\n2. Implement\n3. Integrate\n4. Roll back if checks fail",
        Some(assignment.correlation_id.clone()),
        Some(feedback.id),
        None,
        None,
    )
    .expect("owner revises");
    send_team_message(
        &store,
        &created.team_run.id,
        "host",
        vec![member.id.clone()],
        ProviderDispatchIntent::Message,
        "Plan reviewed. Execute revision 2.",
        Some(assignment.correlation_id.clone()),
        Some(proposal_two.id),
        None,
        None,
    )
    .expect("Host instructs execution");
    let _ = std::fs::remove_dir_all(root);
}
