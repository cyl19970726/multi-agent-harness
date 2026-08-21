use super::*;

#[test]
fn control_plane_ca_issues_exact_company_execution_node_client_identity() {
    let company = "company-a";
    let node = "11111111-1111-4111-8111-111111111111";
    let ca = firm_fabric::pki::generate_ca(company).expect("generate Company CA");
    let csr = firm_fabric::pki::generate_node_csr(company, node).expect("generate Node CSR");
    let issued = firm_fabric::pki::issue_node_certificate(&ca, &csr.csr_pem, company, node, 1_000)
        .expect("issue exact Node client certificate");
    let certificates = rustls_pemfile::certs(&mut std::io::BufReader::new(
        issued.certificate_pem.as_bytes(),
    ))
    .collect::<Result<Vec<_>, _>>()
    .expect("parse issued certificate");
    let identity = firm_fabric::pki::parse_peer_node_identity(&certificates[0])
        .expect("parse mTLS peer identity");
    assert_eq!(identity.company_id, company);
    assert_eq!(identity.node_id, node);
    assert_eq!(identity.public_key_fingerprint, csr.public_key_fingerprint);
    assert_eq!(identity.certificate_serial, issued.serial);

    let hostile = firm_fabric::pki::issue_node_certificate(
        &ca,
        &csr.csr_pem,
        company,
        "22222222-2222-4222-8222-222222222222",
        1_000,
    )
    .expect_err("CSR cannot be reassigned to another ExecutionNode");
    assert_eq!(hostile.code, FabricErrorCode::UnauthorizedActor);
}
