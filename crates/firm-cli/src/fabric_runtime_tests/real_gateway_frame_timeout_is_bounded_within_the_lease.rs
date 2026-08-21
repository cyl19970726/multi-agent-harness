use super::*;

    #[test]
    fn real_gateway_frame_timeout_is_bounded_within_the_lease() {
        assert_eq!(GATEWAY_FRAME_READ_TIMEOUT, Duration::from_secs(5));
        assert!(GATEWAY_FRAME_READ_TIMEOUT < Duration::from_secs(30));
    }

