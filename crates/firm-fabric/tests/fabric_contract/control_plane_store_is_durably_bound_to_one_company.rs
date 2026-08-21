use super::*;

#[test]
fn control_plane_store_is_durably_bound_to_one_company() {
    let root = TestRoot::new("control-company-binding");
    let store = FabricStore::open(root.path()).expect("open store");
    let keys = InMemoryArtifactKeyBackend::default();
    keys.insert(COMPANY, [7; 32]);
    let canonical = ControlPlane::new(COMPANY, "control-a", &store, &keys, [9; 32]);
    canonical
        .acquire_lease("lease-a", 0, 1)
        .expect("bind canonical Company");
    let before = store.snapshot().expect("bound snapshot");
    let foreign = ControlPlane::new("company-foreign", "control-b", &store, &keys, [8; 32]);
    let error = foreign
        .acquire_lease("lease-foreign", 0, 2)
        .expect_err("same physical FabricStore cannot serve another Company");
    assert_eq!(error.code, FabricErrorCode::WrongCompany);
    assert_eq!(error.effect, EffectCertainty::None);
    assert_eq!(store.snapshot().expect("unchanged snapshot"), before);
    drop(store);
    let reopened = FabricStore::open(root.path()).expect("reopen store");
    assert_eq!(
        reopened
            .snapshot()
            .expect("reopened snapshot")
            .authority_company_id
            .as_deref(),
        Some(COMPANY)
    );
}
