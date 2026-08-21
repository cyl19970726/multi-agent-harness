use super::*;

    #[test]
    fn cheatsheet_flags_are_wired() {
        // Flags are read straight off the CHEATSHEET_* consts (not a
        // hand-duplicated list) so editing a const can never silently skip
        // this check.
        for (scope, text) in [
            ("team", CHEATSHEET_TEAM),
            ("work", CHEATSHEET_WORK),
            ("mission", CHEATSHEET_MISSION),
            ("all", CHEATSHEET_ALL),
        ] {
            let flags = extract_flags(text);
            assert!(
                !flags.is_empty(),
                "{scope} cheatsheet must document at least one flag"
            );
            for flag in &flags {
                assert!(
                    flag_is_wired(MAIN_RS_SOURCE, flag),
                    "[{scope}] {flag} is documented in the cheatsheet but is not read by \
                     value()/many()/has_flag()/required() anywhere in main.rs -- it may be \
                     a typo, or a stale/renamed flag"
                );
            }
        }
    }

