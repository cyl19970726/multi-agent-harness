use super::*;

#[test]
fn control_plane_backup_restores_exact_transaction_and_rejects_tamper_or_overwrite() {
    let root = TestRoot::new("control-backup-restore");
    let source_root = root.path().join("source");
    let store = FabricStore::open(&source_root).expect("source Store");
    let keys = InMemoryArtifactKeyBackend::default();
    keys.insert(COMPANY, [7; 32]);
    let control = ControlPlane::new(COMPANY, "control-backup", &store, &keys, [9; 32]);
    control
        .acquire_lease("backup-lease", 0, 100)
        .expect("populate source Store");
    let expected = store.snapshot().unwrap();
    let backup_root = root.path().join("backup");
    let manifest = store.create_backup(&backup_root).expect("atomic backup");
    assert_eq!(manifest.transaction_sequence, expected.revision);
    assert_eq!(manifest.state_digest, json_digest(&expected).unwrap());

    let restored_root = root.path().join("restored");
    let restored_manifest = FabricStore::restore_backup(&backup_root, &restored_root)
        .expect("validated empty-root restore");
    assert_eq!(restored_manifest, manifest);
    assert_eq!(
        FabricStore::open(&restored_root)
            .unwrap()
            .snapshot()
            .unwrap(),
        expected
    );

    let occupied = root.path().join("occupied");
    std::fs::create_dir(&occupied).unwrap();
    std::fs::write(occupied.join("authority.jsonl"), b"do-not-overwrite").unwrap();
    assert_eq!(
        FabricStore::restore_backup(&backup_root, &occupied)
            .expect_err("restore cannot overwrite existing authority")
            .code,
        FabricErrorCode::StoreUnavailable
    );
    OpenOptions::new()
        .append(true)
        .open(backup_root.join("fabric-transactions.jsonl"))
        .unwrap()
        .write_all(b"tamper")
        .unwrap();
    let tampered_target = root.path().join("tampered-target");
    assert_eq!(
        FabricStore::restore_backup(&backup_root, &tampered_target)
            .expect_err("digest-bound backup rejects tampering")
            .code,
        FabricErrorCode::StoreUnavailable
    );
    assert!(!tampered_target.exists());
}
