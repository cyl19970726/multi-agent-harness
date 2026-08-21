use super::*;

    #[test]
    fn source_work_attestation_identity_is_stable_per_proposal_not_per_work_revision() {
        let first = source_work_attestation_id("work-a", 3, 9, "proposal-a");
        assert_eq!(
            first,
            source_work_attestation_id("work-a", 3, 9, "proposal-a")
        );
        assert_ne!(
            first,
            source_work_attestation_id("work-a", 3, 9, "proposal-b"),
            "independent proposals for one frozen Work revision need independent immutable attestations"
        );
    }
