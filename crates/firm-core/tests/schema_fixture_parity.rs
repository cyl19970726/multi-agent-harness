//! Rust half of the official schema-fixture acceptance contract.
//!
//! `pnpm check:schema-fixtures` first runs the JSON Schema checker (including
//! its documented canonical GateSpec semantic layer), then this test applies
//! Rust serde + `Validate` to the exact same Work and Review fixture corpus.
//! The directory classification is therefore the shared expected verdict, not
//! an independent JS-only rule set.

use firm_core::{Review, TeamMessage, Validate, Work};
use serde::de::DeserializeOwned;
use std::fs;
use std::path::{Path, PathBuf};

fn fixture_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas/fixtures")
}

fn json_files(path: &Path) -> Vec<PathBuf> {
    let mut files = fs::read_dir(path)
        .unwrap_or_else(|error| panic!("read {}: {error}", path.display()))
        .map(|entry| entry.expect("fixture directory entry").path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "json")
        })
        .collect::<Vec<_>>();
    files.sort();
    files
}

fn rust_accepts<T>(path: &Path) -> Result<(), String>
where
    T: DeserializeOwned + Validate,
{
    let bytes = fs::read(path).map_err(|error| error.to_string())?;
    let value: T = serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
    value.validate().map_err(|error| error.to_string())
}

fn assert_fixture_contract<T>(fixture_name: &str)
where
    T: DeserializeOwned + Validate,
{
    let root = fixture_root().join(fixture_name);
    for path in json_files(&root.join("valid")) {
        if let Err(error) = rust_accepts::<T>(&path) {
            panic!(
                "Rust rejected official valid {fixture_name} fixture {}: {error}",
                path.display()
            );
        }
    }
    for path in json_files(&root.join("invalid")) {
        if rust_accepts::<T>(&path).is_ok() {
            panic!(
                "Rust accepted official invalid {fixture_name} fixture {}",
                path.display()
            );
        }
    }
}

#[test]
fn work_fixtures_match_rust_serde_and_validate() {
    assert_fixture_contract::<Work>("work");
}

#[test]
fn review_fixtures_match_rust_serde_and_validate() {
    assert_fixture_contract::<Review>("review");
}

#[test]
fn team_message_fixtures_match_rust_serde_and_provider_body_contract() {
    let root = fixture_root().join("team-message");
    for path in json_files(&root.join("valid")) {
        let bytes = fs::read(&path).expect("read valid TeamMessage fixture");
        let message: TeamMessage = serde_json::from_slice(&bytes)
            .unwrap_or_else(|error| panic!("Rust rejected {}: {error}", path.display()));
        message
            .validate_provider_interaction_contract()
            .unwrap_or_else(|error| panic!("Rust rejected {}: {error}", path.display()));
    }
    for path in json_files(&root.join("invalid")) {
        let accepted = fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<TeamMessage>(&bytes).ok())
            .is_some_and(|message| message.validate_provider_interaction_contract().is_ok());
        assert!(
            !accepted,
            "Rust accepted invalid fixture {}",
            path.display()
        );
    }
}

#[test]
fn team_message_wire_rejects_unknown_fields_at_every_closed_layer() {
    let fixture = fixture_root().join("team-message/valid/basic.json");
    let bytes = fs::read(&fixture).expect("basic TeamMessage fixture");
    let base: serde_json::Value = serde_json::from_slice(&bytes).expect("valid fixture JSON");

    let cases = ["TeamMessage", "TeamRecipientRef", "TeamMessageDelivery"];
    for label in cases {
        let mut value = base.clone();
        match label {
            "TeamMessage" => {
                value["unknown_top_level"] = serde_json::json!(true);
            }
            "TeamRecipientRef" => {
                value["recipients"] = serde_json::json!([{
                    "kind": "member_run",
                    "id": "mrun-1",
                    "unknown_recipient_field": true
                }]);
            }
            "TeamMessageDelivery" => {
                value["deliveries"][0]["unknown_delivery_field"] = serde_json::json!(true);
            }
            _ => unreachable!(),
        }

        let error = serde_json::from_value::<TeamMessage>(value)
            .expect_err(&format!("Rust accepted unknown field inside {label}"));
        assert!(
            error.to_string().contains("unknown field"),
            "unexpected {label} rejection: {error}"
        );
    }
}

#[test]
fn omitted_and_explicit_empty_gate_configs_are_one_canonical_duplicate() {
    let fixture = fixture_root().join("work/invalid/duplicate-gate-spec-omitted-config.json");
    let bytes = fs::read(&fixture).expect("canonical duplicate fixture");
    let work: Work = serde_json::from_slice(&bytes).expect("both old and canonical wires parse");
    assert_eq!(work.gates[0], work.gates[1]);
    assert!(work.validate().is_err());
}

#[test]
fn work_wire_rejects_unknown_fields_at_every_closed_schema_layer() {
    let fixture = fixture_root().join("work/valid/basic.json");
    let bytes = fs::read(&fixture).expect("basic Work fixture");
    let base: serde_json::Value = serde_json::from_slice(&bytes).expect("valid fixture JSON");

    let cases = [
        ("Work", vec!["unknown_top_level"]),
        (
            "TeamActorRef",
            vec!["created_by_actor", "unknown_actor_field"],
        ),
        (
            "WorkWorkspace",
            vec!["workspace", "unknown_workspace_field"],
        ),
        (
            "GitHubLink",
            vec!["github_links", "0", "unknown_link_field"],
        ),
    ];

    for (label, path) in cases {
        let mut value = base.clone();
        match label {
            "Work" => {
                value["unknown_top_level"] = serde_json::json!(true);
            }
            "TeamActorRef" => {
                value["created_by_actor"]["unknown_actor_field"] = serde_json::json!(true);
            }
            "WorkWorkspace" => {
                value["workspace"] = serde_json::json!({
                    "kind": "dir",
                    "path": "tmp/work",
                    "unknown_workspace_field": true
                });
            }
            "GitHubLink" => {
                value["github_links"] = serde_json::json!([{
                    "kind": "issue",
                    "owner": "example",
                    "repo": "harness",
                    "number": 1,
                    "url": "https://example.invalid/issues/1",
                    "unknown_link_field": true
                }]);
            }
            _ => unreachable!(),
        }

        let error = serde_json::from_value::<Work>(value).expect_err(&format!(
            "Rust accepted unknown field inside {label}: {path:?}"
        ));
        assert!(
            error.to_string().contains("unknown field"),
            "unexpected {label} rejection: {error}"
        );
    }
}
