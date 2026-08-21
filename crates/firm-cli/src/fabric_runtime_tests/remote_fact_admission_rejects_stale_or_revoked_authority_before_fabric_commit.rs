use super::*;

#[test]
fn remote_fact_admission_rejects_stale_or_revoked_authority_before_fabric_commit() {
    for case in ["stale-revision", "revoked-policy"] {
        let root = std::env::temp_dir().join(format!(
            "agentfirm-remote-fact-fence-{case}-{}-{}",
            std::process::id(),
            now_unix_ms().unwrap()
        ));
        let (attestation, delegation, mut policy, _publication, mut operation) =
            current_remote_fact_fixture();
        if case == "stale-revision" {
            let mut reference = match operation.closed_body().unwrap() {
                harness_fabric::ClosedOperationBody::CollaborationBusiness(reference) => reference,
                _ => unreachable!(),
            };
            reference.expected_revision = delegation.revision - 1;
            operation.body = serde_json::to_value(reference).unwrap();
            operation.body_digest = harness_store::canonical_json_fingerprint(&operation.body);
        } else {
            policy.revoked_at = Some("unix-ms:9".into());
        }
        seed_current_remote_fact_authority(&root, &attestation, &delegation, &policy);
        let before = std::fs::read(root.join("agentfirm_collaboration_operations.jsonl")).unwrap();
        let application = Wave6ControlPlaneApplication {
            collaboration_root: root.clone(),
            company_id: "company-1".into(),
            actor_id: "control-plane".into(),
        };
        let commits = std::sync::atomic::AtomicUsize::new(0);
        let mut accept = || {
            commits.fetch_add(1, Ordering::SeqCst);
            panic!("authority rejection must happen before Fabric commit")
        };
        assert!(application
            .admit_and_accept_source(&operation, &operation.actor, &mut accept,)
            .is_err());
        assert_eq!(commits.load(Ordering::SeqCst), 0);
        assert_eq!(
            std::fs::read(root.join("agentfirm_collaboration_operations.jsonl")).unwrap(),
            before
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}
