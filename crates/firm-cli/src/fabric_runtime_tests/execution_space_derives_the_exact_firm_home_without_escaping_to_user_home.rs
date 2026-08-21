use super::*;

#[test]
fn execution_space_derives_the_exact_firm_home_without_escaping_to_user_home() {
    assert_eq!(
        firm_home_from_execution_space_root(Path::new(
            "/Users/test/.firm/execution-spaces/space-a"
        ))
        .expect("canonical Execution Space layout"),
        PathBuf::from("/Users/test/.firm")
    );
    assert!(
        firm_home_from_execution_space_root(Path::new("/Users/test/arbitrary/space-a")).is_err()
    );
}
