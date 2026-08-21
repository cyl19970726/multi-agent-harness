use super::*;

    #[test]
    fn classify_failure_reason_ok_is_none() {
        assert_eq!(classify_failure_reason(true, Some(0), false), None);
        assert_eq!(classify_failure_reason(true, Some(1), false), None);
    }

