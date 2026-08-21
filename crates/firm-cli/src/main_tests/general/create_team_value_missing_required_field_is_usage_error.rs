use super::*;

#[test]
fn create_team_value_missing_required_field_is_usage_error() {
    let (store, root) = temp_store("wp-ii-team-bad");
    // No owner / name -> CliError::Usage (mapped to HTTP 400 by serve loop).
    let body = serde_json::json!({"description": "no name or owner"});
    let error =
        create_team_value(&store, "space-test", &body).expect_err("missing fields must error");
    assert!(
        matches!(error, CliError::Usage(_)),
        "malformed body must be a Usage error, got: {error:?}"
    );
    let _ = std::fs::remove_dir_all(root);
}
