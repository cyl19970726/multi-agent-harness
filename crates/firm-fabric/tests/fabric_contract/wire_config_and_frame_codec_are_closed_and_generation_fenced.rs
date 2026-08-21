use super::*;

#[test]
fn wire_config_and_frame_codec_are_closed_and_generation_fenced() {
    let config = NodeFabricConfig {
        company_id: COMPANY.into(),
        node_id: "node-a".into(),
        control_plane_url: "wss://control.agentfirm.test/v1/node-gateway/connect".into(),
        reconnect_floor_ms: 250,
        reconnect_ceiling_ms: 10_000,
    };
    config.validate().expect("outbound secure endpoint");
    for endpoint in [
        "ws://control.agentfirm.test/v1/node-gateway/connect",
        "https://control.agentfirm.test/v1/node-gateway/connect",
        "wss://token@control.agentfirm.test/v1/node-gateway/connect",
        "wss://control.agentfirm.test/v1/node-gateway/connect?node=browser-selected",
    ] {
        let mut hostile = config.clone();
        hostile.control_plane_url = endpoint.into();
        assert!(hostile.validate().is_err(), "must reject {endpoint}");
    }

    let payload = FabricPayload::Heartbeat {
        observed_at_unix_ms: 100,
    };
    let frame = FabricFrame::new(
        "frame-1",
        COMPANY,
        "node-a",
        3,
        "node-daemon:node-a",
        5,
        2,
        100,
        "correlation-1",
        payload,
    )
    .expect("create frame");
    let bytes = encode_frame(&frame).expect("encode frame");
    assert_eq!(decode_frame(&bytes).expect("decode frame"), frame);
    FabricSessionFence {
        company_id: COMPANY.into(),
        node_id: "node-a".into(),
        gateway_generation: 3,
        node_daemon_id: "node-daemon:node-a".into(),
        node_daemon_generation: 5,
        control_plane_generation: 2,
    }
    .validate_frame(&frame)
    .expect("exact session fence");
    let before = frame.clone();
    let stale = FabricSessionFence {
        company_id: COMPANY.into(),
        node_id: "node-a".into(),
        gateway_generation: 4,
        node_daemon_id: "node-daemon:node-a".into(),
        node_daemon_generation: 5,
        control_plane_generation: 2,
    }
    .validate_frame(&frame)
    .expect_err("successor generation fences predecessor frame");
    assert_eq!(stale.code, FabricErrorCode::NodeStaleGeneration);
    assert_eq!(frame, before);

    let mut unknown: serde_json::Value = serde_json::from_slice(&bytes).expect("frame JSON");
    unknown["client_selected_actor"] = json!("host");
    let unknown_bytes = serde_json::to_vec(&unknown).expect("encode hostile frame");
    assert_eq!(
        decode_frame(&unknown_bytes)
            .expect_err("unknown field must fail closed")
            .code,
        FabricErrorCode::InvalidPayload
    );
    assert_eq!(
        decode_frame(&vec![b'x'; MAX_FABRIC_FRAME_BYTES + 1])
            .expect_err("oversized frame must fail before parsing")
            .code,
        FabricErrorCode::InvalidPayload
    );
}
