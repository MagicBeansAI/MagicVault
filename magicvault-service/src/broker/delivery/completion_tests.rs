use super::*;

// Deliberately stop polling the current-thread async runtime after consent
// returns. The blocking completion writer must release admission itself.
#[tokio::test(flavor = "current_thread")]
async fn denied_delivery_releases_admission_before_status_is_observable() {
    let f = Fixture::new().await;
    let profile = f.register(f.process()).await;
    f.human.mode.store(7, Ordering::SeqCst);
    let request = f.run(profile, DeliveryKind::Process).await;
    tokio::time::timeout(Duration::from_secs(2), f.human.denied.notified())
        .await
        .unwrap();
    let until = Instant::now() + Duration::from_secs(2);
    loop {
        let s = f.broker.state.lock().unwrap();
        let status = &s.deliveries.jobs.get(&request.operation_id).unwrap().status;
        if !matches!(
            status.state,
            DeliveryState::Pending | DeliveryState::Running
        ) {
            assert_eq!(status.state, DeliveryState::Denied);
            assert!(!status.may_have_run);
            assert!(
                f.broker.human_gate.try_acquire().is_ok(),
                "terminal delivery became visible before admission was released"
            );
            break;
        }
        drop(s);
        assert!(
            Instant::now() < until,
            "completion transaction did not finish"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(!f.root.path().join("runs").exists());
    f.broker.quiesce().await;
    assert_eq!(
        Arc::strong_count(&f.broker),
        1,
        "shutdown retained a worker-owned broker/instance lock"
    );
}
