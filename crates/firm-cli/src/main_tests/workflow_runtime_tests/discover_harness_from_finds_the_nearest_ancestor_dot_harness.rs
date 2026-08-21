use super::*;

    #[test]
    fn discover_harness_from_finds_the_nearest_ancestor_dot_harness() {
        let base = std::env::temp_dir().join(format!("harness-disc-{}", generated_id("d")));
        let proj = base.join("proj");
        let deep = proj.join("a").join("b");
        std::fs::create_dir_all(&deep).expect("mk deep");
        std::fs::create_dir_all(proj.join(".harness")).expect("mk .harness");

        // From a nested subdir, discovery walks UP to proj/.harness.
        let found = discover_harness_from(&deep).expect("found ancestor .harness");
        assert_eq!(
            std::fs::canonicalize(&found).unwrap(),
            std::fs::canonicalize(proj.join(".harness")).unwrap()
        );
        // A tree with no .harness returns None.
        let bare = base.join("bare").join("x");
        std::fs::create_dir_all(&bare).expect("mk bare");
        // (only true if no ancestor of `bare` has .harness — base/bare has none)
        assert!(discover_harness_from(&base.join("bare")).is_none());

        let _ = std::fs::remove_dir_all(&base);
    }

