use super::*;

    #[test]
    fn generated_ids_do_not_collide_across_processes_with_same_millis_and_counter() {
        let left = generated_id_from_parts("rpc", 1_782_832_612_114, 1001, 0);
        let right = generated_id_from_parts("rpc", 1_782_832_612_114, 1002, 0);

        assert_ne!(left, right);
    }

