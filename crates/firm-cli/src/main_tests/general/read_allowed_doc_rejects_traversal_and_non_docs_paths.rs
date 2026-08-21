use super::*;

#[test]
fn read_allowed_doc_rejects_traversal_and_non_docs_paths() {
    // Missing parameter.
    assert!(read_allowed_doc("/v1/docs").is_err());
    // Outside the docs/ + root-doc allow-list.
    assert!(read_allowed_doc("/v1/docs?path=etc/passwd").is_err());
    assert!(read_allowed_doc("/v1/docs?path=Cargo.toml").is_err());
    // Path traversal, even under docs/.
    assert!(read_allowed_doc("/v1/docs?path=docs/../Cargo.toml").is_err());
}
