use super::*;

#[test]
fn allowed_doc_path_kind_allows_docs_tree_and_root_entry_docs() {
    assert_eq!(
        allowed_doc_path_kind("docs/registry.json"),
        Ok(AllowedDocPathKind::DocsTree)
    );
    assert_eq!(
        allowed_doc_path_kind("README.md"),
        Ok(AllowedDocPathKind::RootDoc)
    );
    assert_eq!(
        allowed_doc_path_kind("AGENTS.md"),
        Ok(AllowedDocPathKind::RootDoc)
    );
    assert!(allowed_doc_path_kind("Cargo.toml").is_err());
    assert!(allowed_doc_path_kind("docs/../Cargo.toml").is_err());
}
