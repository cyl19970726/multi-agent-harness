use super::*;

#[test]
fn live_member_control_client_retries_a_partial_response_timeout_on_the_same_socket() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind Supervisor control socket");
    let address = listener.local_addr().expect("Supervisor control address");
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept live-member control");
        let mut request = String::new();
        BufReader::new(&mut stream)
            .read_line(&mut request)
            .expect("read one live-member control request");
        assert_eq!(request.lines().count(), 1, "client writes the request once");
        let request = serde_json::from_str::<serde_json::Value>(request.trim())
            .expect("live-member control request JSON");
        assert_eq!(request["command"], "interrupt");
        assert_eq!(request["team_run_id"], "team-run-read-retry");

        stream
            .write_all(b"{\"ok\":true,\"result\":")
            .expect("write partial live-member control response");
        stream.flush().expect("flush partial response");
        std::thread::sleep(Duration::from_millis(45));
        stream
            .write_all(b"{\"retried\":true},\"error\":null,\"store_lock_timeout\":null}\n")
            .expect("complete delayed live-member control response");
        stream
            .set_read_timeout(Some(Duration::from_millis(20)))
            .expect("bound duplicate request check");
        let mut extra = [0_u8; 1];
        match stream.read(&mut extra) {
            Ok(0) => {}
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) => {}
            Ok(count) => panic!("client replayed {count} unexpected request byte(s)"),
            Err(error) => panic!("check for a replayed request: {error}"),
        }
    });
    let request = LiveMemberControlRequest::Interrupt {
        team_run_id: "team-run-read-retry".into(),
        member_run_id: "member-run-read-retry".into(),
        reason: "test transient response timeout".into(),
        requested_by: "host-read-retry".into(),
    };

    let stream = TcpStream::connect(address).expect("connect to Supervisor control socket");
    let response = send_live_member_control_request(
        stream,
        &request,
        Duration::from_millis(20),
        20,
        Duration::from_millis(2),
    )
    .expect("same-socket retry receives accepted live-member control response");

    assert_eq!(response["retried"], true);
    server.join().expect("Supervisor control server");
}
