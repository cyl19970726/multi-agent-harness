use super::*;

#[test]
fn application_owned_handles_can_move_to_the_supervisor_thread() {
    fn assert_send<T: Send>() {}
    assert_send::<CliPreparedRun>();
    assert_send::<CliNodeSession>();
    assert_send::<PreparedDaemonRun>();
}

#[test]
fn registration_drop_joins_control_before_heartbeat_and_invalidates_under_authority_gate() {
    let root = std::env::temp_dir().join(format!(
        "firm-s7-drop-{}-{}",
        std::process::id(),
        current_unix_ms()
    ));
    let store = HarnessStore::new(&root);
    store.init().expect("initialize isolated store");
    let control_stop = Arc::new(AtomicBool::new(false));
    let heartbeat_stop = Arc::new(AtomicBool::new(false));
    let heartbeat_valid = Arc::new(AtomicBool::new(true));
    let authority_gate = Arc::new(Mutex::new(()));
    let (control_done_tx, control_done_rx) = std::sync::mpsc::channel();
    let (heartbeat_done_tx, heartbeat_done_rx) = std::sync::mpsc::channel();
    let control_thread = {
        let stop = Arc::clone(&control_stop);
        let heartbeat_stop = Arc::clone(&heartbeat_stop);
        std::thread::spawn(move || {
            while !stop.load(Ordering::Acquire) {
                std::thread::yield_now();
            }
            assert!(
                !heartbeat_stop.load(Ordering::Acquire),
                "control must join before heartbeat stop"
            );
            control_done_tx.send(()).unwrap();
        })
    };
    let heartbeat_thread = {
        let stop = Arc::clone(&heartbeat_stop);
        let valid = Arc::clone(&heartbeat_valid);
        std::thread::spawn(move || {
            while !stop.load(Ordering::Acquire) {
                std::thread::yield_now();
            }
            control_done_rx
                .recv_timeout(Duration::from_secs(2))
                .unwrap();
            assert!(valid.load(Ordering::Acquire));
            heartbeat_done_tx.send(()).unwrap();
        })
    };
    let registration = TeamSupervisorRegistration {
        team_run_id: "s7-no-provider-run".into(),
        supervisor_id: "s7-supervisor".into(),
        generation: 1,
        store,
        heartbeat_stop,
        heartbeat_valid: Arc::clone(&heartbeat_valid),
        authority_gate: Arc::clone(&authority_gate),
        heartbeat_thread: Some(heartbeat_thread),
        control_stop,
        control_thread: Some(control_thread),
    };
    let gate = authority_gate.lock().unwrap();
    let owner = std::thread::spawn(move || drop(registration));
    heartbeat_done_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("both worker threads joined before authority invalidation");
    assert!(heartbeat_valid.load(Ordering::Acquire));
    drop(gate);
    owner.join().expect("registration owner joined");
    assert!(!heartbeat_valid.load(Ordering::Acquire));
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn message_body_digest_preserves_exact_bytes() {
    let application = DaemonApplication;
    assert_eq!(
        application.message_body_digest(""),
        "sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
    assert_eq!(
        application.message_body_digest("line one\r\n工具🧪\n"),
        "sha256:75220eca60b72dae2854a12e14779b811edffeca854c7cf01be0c9ea384b1279"
    );
    assert_eq!(
        application.message_body_digest(" a\0b "),
        "sha256:9c8cfe894156d6cbf11a15bac2ebde16d03dadc9717a1daa0386151bc0731998"
    );
}
