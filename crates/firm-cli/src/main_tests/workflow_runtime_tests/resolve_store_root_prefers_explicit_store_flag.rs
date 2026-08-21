use super::*;

#[test]
fn resolve_store_root_prefers_explicit_store_flag() {
    let mut args = vec![
        "--store".to_string(),
        "/explicit/store".to_string(),
        "serve".to_string(),
    ];
    let root = resolve_store_root(&mut args);
    assert_eq!(root, PathBuf::from("/explicit/store"));
    // Flag stripped so dispatch sees only the subcommand.
    assert_eq!(args, vec!["serve"]);
}
