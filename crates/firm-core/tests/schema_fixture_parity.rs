//! Rust half of the official schema-fixture acceptance contract.
//!
//! `pnpm check:schema-fixtures` first runs the JSON Schema checker (including
//! its documented canonical GateSpec semantic layer), then this test applies
//! Rust serde + `Validate` to the exact same Work and Review fixture corpus.
//! The directory classification is therefore the shared expected verdict, not
//! an independent JS-only rule set.

use firm_core::{Review, Validate, Work};
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
fn omitted_and_explicit_empty_gate_configs_are_one_canonical_duplicate() {
    let fixture = fixture_root().join("work/invalid/duplicate-gate-spec-omitted-config.json");
    let bytes = fs::read(&fixture).expect("canonical duplicate fixture");
    let work: Work = serde_json::from_slice(&bytes).expect("both old and canonical wires parse");
    assert_eq!(work.gates[0], work.gates[1]);
    assert!(work.validate().is_err());
}
