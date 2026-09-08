use super::*;

#[tokio::test(flavor = "current_thread")]
async fn decided_metadata_releases_admission_before_status_is_observable() {
    let f = Fixture::new().await;
    let id = Uuid::new_v4();
    f.broker
        .request_access(
            f.auth.clone(),
            id,
            AccessRequest {
                credential_ref: f.reference.clone(),
            },
        )
        .await
        .unwrap();
    tokio::time::timeout(
        Duration::from_secs(2),
        f.human.metadata_completed.notified(),
    )
    .await
    .unwrap();
    let until = Instant::now() + Duration::from_secs(2);
    loop {
        let s = f.broker.state.lock().unwrap();
        let decision = s.jobs.get(&id).unwrap().decision;
        if decision != Decision::Pending {
            assert_eq!(decision, Decision::Allowed);
            assert!(
                f.broker.human_gate.try_acquire().is_ok(),
                "decided metadata became visible before admission was released"
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
    f.broker.quiesce().await;
    assert_eq!(
        Arc::strong_count(&f.broker),
        1,
        "shutdown retained a worker-owned broker/instance lock"
    );
}

// The current-thread runtime is deliberately not polled after the adapter
// returns. The final blocking transaction can finish, but its async caller
// cannot resume to release an incorrectly retained permit. No scheduler luck
// or arbitrary post-completion sleep is needed to exercise the old race.
#[tokio::test(flavor = "current_thread")]
async fn terminal_fill_releases_admission_before_status_is_observable() {
    for poison in [false, true] {
        let f = Fixture::new().await;
        f.configure(vec!["https://example.com".into()]).await;
        if poison {
            *f.adapter.fail_audit.lock().unwrap() = Some(
                f.root
                    .path()
                    .join("vault")
                    .join(magicvault_core::store::SECRET_AUDIT_FILENAME),
            );
        }
        let request = f.request().await;
        let id = request.operation_id;
        f.broker.secure_fill(f.auth.clone(), request).await.unwrap();
        tokio::time::timeout(Duration::from_secs(2), f.adapter.completed.notified())
            .await
            .unwrap();
        let until = Instant::now() + Duration::from_secs(2);
        loop {
            let s = f.broker.state.lock().unwrap();
            let status = &s.browsers.fills.get(&id).unwrap().status;
            if !matches!(status.state, FillState::Pending | FillState::Filling) {
                assert_eq!(
                    status.state,
                    if poison {
                        FillState::Uncertain
                    } else {
                        FillState::Filled
                    }
                );
                assert!(
                    f.broker.human_gate.try_acquire().is_ok(),
                    "terminal receipt became visible before admission was released"
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
        f.broker.quiesce().await;
        assert_eq!(
            Arc::strong_count(&f.broker),
            1,
            "shutdown retained a worker-owned broker/instance lock"
        );
    }
}
