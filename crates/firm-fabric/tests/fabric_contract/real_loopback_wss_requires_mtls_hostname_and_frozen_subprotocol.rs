use super::*;

#[test]
fn real_loopback_wss_requires_mtls_hostname_and_frozen_subprotocol() {
    use firm_fabric::transport::{
        accept_control_plane_mtls, connect_outbound_mtls, connect_outbound_mtls_material,
        ControlPlaneTlsFiles, NodeFabricConfig, NodeTlsIdentityFiles, NodeTlsIdentityMaterial,
    };
    let root = TestRoot::new("real-loopback-mtls");
    let company = "company-a";
    let node = "11111111-1111-4111-8111-111111111111";
    let now_unix_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock")
        .as_millis() as u64;
    let ca = firm_fabric::pki::generate_ca(company).expect("generate CA");
    let csr = firm_fabric::pki::generate_node_csr(company, node).expect("generate Node CSR");
    let node_certificate =
        firm_fabric::pki::issue_node_certificate(&ca, &csr.csr_pem, company, node, now_unix_ms)
            .expect("issue Node certificate");
    let server_certificate =
        firm_fabric::pki::issue_control_plane_server_certificate(&ca, "localhost", now_unix_ms)
            .expect("issue server certificate");
    let ca_path = root.path().join("ca.pem");
    let node_cert_path = root.path().join("node.pem");
    let node_key_path = root.path().join("node-key.pem");
    let server_cert_path = root.path().join("server.pem");
    let server_key_path = root.path().join("server-key.pem");
    std::fs::write(&ca_path, &ca.certificate_pem).unwrap();
    std::fs::write(
        &node_cert_path,
        format!("{}{}", node_certificate.certificate_pem, ca.certificate_pem),
    )
    .unwrap();
    std::fs::write(&node_key_path, &csr.private_key_pem).unwrap();
    std::fs::write(&server_cert_path, &server_certificate.certificate_chain_pem).unwrap();
    std::fs::write(&server_key_path, &server_certificate.private_key_pem).unwrap();
    secure_private_key(&node_key_path);
    secure_private_key(&server_key_path);

    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind Control Plane");
    let port = listener.local_addr().unwrap().port();
    let server_tls = ControlPlaneTlsFiles {
        server_certificate_chain_pem: server_cert_path,
        server_private_key_pem: server_key_path,
        node_ca_pem: ca_path.clone(),
    };
    let server = std::thread::spawn(move || {
        (0..2)
            .map(|_| {
                let (tcp, _) = listener.accept().expect("accept outbound Node connection");
                let (mut socket, identity) = accept_control_plane_mtls(tcp, &server_tls)
                    .expect("accept verified mTLS WebSocket");
                let frame = firm_fabric::transport::read_frame(&mut socket)
                    .expect("read authenticated Fabric frame");
                socket.close(None).ok();
                (identity, frame)
            })
            .collect::<Vec<_>>()
    });
    let config = NodeFabricConfig {
        company_id: company.into(),
        node_id: node.into(),
        control_plane_url: format!(
            "wss://localhost:{port}{}",
            firm_fabric::transport::FABRIC_GATEWAY_PATH
        ),
        reconnect_floor_ms: 100,
        reconnect_ceiling_ms: 1_000,
    };
    let client_tls = NodeTlsIdentityFiles {
        client_certificate_chain_pem: node_cert_path,
        client_private_key_pem: node_key_path,
        control_plane_ca_pem: ca_path,
    };
    let mut socket = connect_outbound_mtls(&config, &client_tls).expect("connect outbound mTLS");
    let sent = FabricFrame::new(
        "frame-loopback-heartbeat",
        company,
        node,
        7,
        format!("node-daemon:{node}"),
        3,
        11,
        now_unix_ms,
        "correlation-loopback",
        FabricPayload::Heartbeat {
            observed_at_unix_ms: now_unix_ms,
        },
    )
    .expect("build Fabric frame");
    firm_fabric::transport::write_frame(&mut socket, &sent)
        .expect("write authenticated Fabric frame");
    socket.close(None).ok();
    let keychain_like = NodeTlsIdentityMaterial {
        client_certificate_chain_pem: format!(
            "{}{}",
            node_certificate.certificate_pem, ca.certificate_pem
        )
        .into_bytes(),
        client_private_key_pem: csr.private_key_pem.as_bytes().to_vec(),
        control_plane_ca_pem: ca.certificate_pem.as_bytes().to_vec(),
    };
    let mut memory_socket = connect_outbound_mtls_material(&config, &keychain_like)
        .expect("connect using OS-credential material without a temporary private-key file");
    firm_fabric::transport::write_frame(&mut memory_socket, &sent).unwrap();
    memory_socket.close(None).ok();
    let observed_sessions = server.join().expect("join Control Plane");
    for (identity, observed) in observed_sessions {
        assert_eq!(identity.company_id, company);
        assert_eq!(identity.node_id, node);
        assert_eq!(identity.public_key_fingerprint, csr.public_key_fingerprint);
        assert_eq!(observed, sent);
    }
}
