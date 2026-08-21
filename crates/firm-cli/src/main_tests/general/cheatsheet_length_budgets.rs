use super::*;

    #[test]
    fn cheatsheet_length_budgets() {
        // `all` <= 2000 chars; each scoped page <= 1200 chars.
        assert!(
            CHEATSHEET_ALL.len() <= 2000,
            "CHEATSHEET_ALL length {} exceeds 2000",
            CHEATSHEET_ALL.len()
        );
        assert!(
            CHEATSHEET_TEAM.len() <= 1200,
            "CHEATSHEET_TEAM length {} exceeds 1200",
            CHEATSHEET_TEAM.len()
        );
        assert!(
            CHEATSHEET_WORK.len() <= 1200,
            "CHEATSHEET_WORK length {} exceeds 1200",
            CHEATSHEET_WORK.len()
        );
        assert!(
            CHEATSHEET_MISSION.len() <= 1200,
            "CHEATSHEET_MISSION length {} exceeds 1200",
            CHEATSHEET_MISSION.len()
        );
    }

