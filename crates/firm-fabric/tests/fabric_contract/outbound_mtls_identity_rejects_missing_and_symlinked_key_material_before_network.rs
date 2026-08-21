use super::*;

#[test]
fn outbound_mtls_identity_rejects_missing_and_symlinked_key_material_before_network() {
    use firm_fabric::transport::NodeTlsIdentityFiles;
    let root = TestRoot::new("tls-identity-files");
    let cert = root.path().join("client-cert.pem");
    let key = root.path().join("client-key.pem");
    let ca = root.path().join("ca.pem");
    std::fs::write(&cert, b"certificate").expect("write certificate fixture");
    std::fs::write(&key, b"key").expect("write key fixture");
    secure_private_key(&key);
    std::fs::write(&ca, b"ca").expect("write CA fixture");
    let identity = NodeTlsIdentityFiles {
        client_certificate_chain_pem: cert.clone(),
        client_private_key_pem: key.clone(),
        control_plane_ca_pem: ca,
    };
    identity.validate().expect("regular credential handles");

    let linked_key = root.path().join("linked-key.pem");
    std::os::unix::fs::symlink(&key, &linked_key).expect("create hostile symlink");
    let hostile = NodeTlsIdentityFiles {
        client_private_key_pem: linked_key,
        ..identity
    };
    assert_eq!(
        hostile
            .validate()
            .expect_err("credential loader cannot follow a key symlink")
            .effect,
        EffectCertainty::None
    );
}
