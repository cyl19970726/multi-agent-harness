use super::*;

#[test]
fn take_flag_value_removes_the_pair_and_returns_the_value() {
    let mut args = vec![
        "--store".to_string(),
        "/tmp/store".to_string(),
        "serve".to_string(),
        "--addr".to_string(),
        "127.0.0.1:1".to_string(),
    ];
    assert_eq!(
        take_flag_value(&mut args, "--store").as_deref(),
        Some("/tmp/store")
    );
    // The pair is stripped so the subcommand parser never sees it.
    assert_eq!(args, vec!["serve", "--addr", "127.0.0.1:1"]);
    // Absent flag -> None, args untouched.
    assert_eq!(take_flag_value(&mut args, "--store"), None);
    assert_eq!(args.len(), 3);
    // Trailing flag with no value -> flag removed, None returned.
    let mut trailing = vec!["serve".to_string(), "--store".to_string()];
    assert_eq!(take_flag_value(&mut trailing, "--store"), None);
    assert_eq!(trailing, vec!["serve"]);
}
