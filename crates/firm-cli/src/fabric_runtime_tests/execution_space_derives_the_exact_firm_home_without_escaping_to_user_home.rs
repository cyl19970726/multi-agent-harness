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

/// ADR 0075 names one machine lease per canonical Firm home, so the CLI's
/// derivation must apply the same absolute + canonical rule the Store applies
/// (#993): a relative root is refused (its lexical parent would be the empty
/// path, silently binding the home to the caller's cwd), and an aliased home
/// resolves to the one directory every other process names.
#[test]
fn execution_space_firm_home_derivation_is_absolute_and_canonical() {
    assert!(
        firm_home_from_execution_space_root(Path::new("execution-spaces/space-a")).is_err(),
        "a relative Execution Space root must not bind a cwd-relative Firm home"
    );

    let temp = std::env::temp_dir().join(format!(
        "firm-home-derivation-test-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or_default()
    ));
    let real_home = temp.join("real-home");
    let store_root = real_home.join("execution-spaces").join("space-a");
    std::fs::create_dir_all(&store_root).expect("create fixture store root");
    let alias = temp.join("alias-home");
    std::os::unix::fs::symlink(&real_home, &alias).expect("symlink alias to the real home");

    let derived =
        firm_home_from_execution_space_root(&alias.join("execution-spaces").join("space-a"))
            .expect("aliased spelling of a canonical layout");
    assert_eq!(
        derived,
        std::fs::canonicalize(&real_home).expect("canonical real home"),
        "the derived home must be the one canonical directory, not the caller's spelling"
    );

    std::fs::remove_dir_all(&temp).expect("clean up fixture");
}
