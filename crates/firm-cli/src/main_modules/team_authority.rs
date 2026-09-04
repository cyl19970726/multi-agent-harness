use super::*;

/// Derive each durable Team's current CAS revision from its append-only rows.
/// Every CLI and RoleView caller uses this fold so advertised and submitted
/// Team revisions cannot drift apart.
pub(crate) fn derive_team_revisions(team_rows: &[AgentTeam]) -> BTreeMap<String, u64> {
    team_rows
        .iter()
        .fold(BTreeMap::new(), |mut revisions, team| {
            *revisions.entry(team.id.clone()).or_insert(0) += 1;
            revisions
        })
}
