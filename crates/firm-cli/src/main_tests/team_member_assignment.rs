use super::*;

/// A member brief must survive the identity grammar: it is free text and
/// may contain the `@` and `:` that owned-paths / identity parsing consume.
#[test]
fn member_spec_brief_is_split_before_paths_and_identity() {
    let spec = parse_team_member_spec(
        "Alice:reviewer:codex:gpt-5@crates/a.rs,crates/b.rs#attack the lease change: see a@b",
    )
    .expect("spec parses");
    assert_eq!(spec.name, "Alice");
    assert_eq!(spec.role, "reviewer");
    assert_eq!(spec.provider, "codex");
    assert_eq!(spec.model.as_deref(), Some("gpt-5"));
    assert_eq!(spec.owned_paths, vec!["crates/a.rs", "crates/b.rs"]);
    assert_eq!(
        spec.initial_work.as_deref(),
        Some("attack the lease change: see a@b"),
        "the brief keeps its own separators"
    );

    // No brief keeps the historical shape exactly.
    let plain = parse_team_member_spec("Bob:builder:claude@src/x.rs").expect("spec parses");
    assert_eq!(plain.initial_work, None);
    assert_eq!(plain.owned_paths, vec!["src/x.rs"]);
}
