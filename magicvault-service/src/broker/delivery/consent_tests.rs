use super::*;

async fn grants(f: &Fixture) -> Vec<ConsentGrantInfo> {
    let Response::Consents(rows) = response(&f.broker, Some(&f.token), Request::ListConsents).await
    else {
        panic!("consents")
    };
    rows
}

async fn refusal_fixture(kind: DeliveryKind) -> (Fixture, Uuid, Option<std::net::TcpListener>) {
    let f = Fixture::new().await;
    let (profile, listener) = match kind {
        DeliveryKind::Process => (f.process(), None),
        DeliveryKind::Http => {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            listener.set_nonblocking(true).unwrap();
            let profile = DeliveryProfile {
                label: "Undispatched HTTP fixture".into(),
                destination: DeliveryDestination::Http(HttpDestination {
                    url: format!("http://{}/", listener.local_addr().unwrap()),
                    method: "POST".into(),
                    headers: vec![NamedValue {
                        name: "Authorization".into(),
                        value: f.value(),
                    }],
                    query: vec![],
                    body: None,
                    timeout_secs: 2,
                }),
            };
            (profile, Some(listener))
        }
    };
    let profile = f.register(profile).await;
    (f, profile, listener)
}

async fn prompt_started(f: &Fixture) {
    tokio::time::timeout(Duration::from_secs(2), async {
        while f.human.uses.load(Ordering::SeqCst) == 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
}

async fn assert_no_dispatch(f: &Fixture, listener: &Option<std::net::TcpListener>) {
    assert!(grants(f).await.is_empty());
    assert!(!f.root.path().join("runs").exists());
    if let Some(listener) = listener {
        assert_eq!(
            listener.accept().unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn native_style_delivery_cancellation_is_not_a_human_denial() {
    for kind in [DeliveryKind::Process, DeliveryKind::Http] {
        for clear in [false, true] {
            let (f, profile, listener) = refusal_fixture(kind).await;
            f.human.mode.store(5, Ordering::SeqCst);
            let op = f.run(profile, kind).await;
            prompt_started(&f).await;
            response(
                &f.broker,
                Some(&f.token),
                if clear {
                    Request::ClearConsents
                } else {
                    Request::CancelDelivery(FillQuery {
                        operation_id: op.operation_id,
                    })
                },
            )
            .await;
            let status = f.settled(op.operation_id).await;
            assert_eq!(
                status.state,
                DeliveryState::Cancelled,
                "{kind:?}, clear={clear}"
            );
            assert_eq!(status.error, Some(ErrorCode::Cancelled));
            assert!(!status.may_have_run);
            assert_no_dispatch(&f, &listener).await;
            f.broker.quiesce().await;
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn native_style_delivery_denial_without_cancellation_stays_denied() {
    for kind in [DeliveryKind::Process, DeliveryKind::Http] {
        let (f, profile, listener) = refusal_fixture(kind).await;
        f.human.mode.store(7, Ordering::SeqCst);
        let op = f.run(profile, kind).await;
        let status = f.settled(op.operation_id).await;
        assert_eq!(status.state, DeliveryState::Denied);
        assert_eq!(status.error, Some(ErrorCode::Denied));
        assert!(!status.may_have_run);
        assert_no_dispatch(&f, &listener).await;
        f.broker.quiesce().await;
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn native_style_delivery_expiry_takes_precedence_over_teardown_cancellation() {
    for kind in [DeliveryKind::Process, DeliveryKind::Http] {
        let (f, profile, listener) = refusal_fixture(kind).await;
        f.human.mode.store(6, Ordering::SeqCst);
        let op = f.run(profile, kind).await;
        prompt_started(&f).await;
        response(
            &f.broker,
            Some(&f.token),
            Request::CancelDelivery(FillQuery {
                operation_id: op.operation_id,
            }),
        )
        .await;
        let status = f.settled(op.operation_id).await;
        assert_eq!(status.state, DeliveryState::Expired);
        assert_eq!(status.error, Some(ErrorCode::Expired));
        assert!(!status.may_have_run);
        assert_no_dispatch(&f, &listener).await;
        f.broker.quiesce().await;
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn default_once_prompts_again_while_always_is_exact_and_revocable() {
    let f = Fixture::new().await;
    let profile = f.register(f.process()).await;
    for _ in 0..2 {
        let op = f.run(profile, DeliveryKind::Process).await;
        assert_eq!(
            f.settled(op.operation_id).await.state,
            DeliveryState::Completed
        );
    }
    assert_eq!(f.human.uses.load(Ordering::SeqCst), 2);
    assert!(grants(&f).await.is_empty());
    f.human.mode.store(3, Ordering::SeqCst);
    let op = f.run(profile, DeliveryKind::Process).await;
    assert_eq!(
        f.settled(op.operation_id).await.state,
        DeliveryState::Completed
    );
    let rows = grants(&f).await;
    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0].scope,
        ConsentScope::Delivery {
            profile_id: profile
        }
    );
    f.human.mode.store(1, Ordering::SeqCst); // Another prompt would deny.
    let op = f.run(profile, DeliveryKind::Process).await;
    assert_eq!(
        f.settled(op.operation_id).await.state,
        DeliveryState::Completed
    );
    assert_eq!(f.human.uses.load(Ordering::SeqCst), 3);
    // Same config under a new immutable profile identity is NOT covered.
    let mut other = f.process();
    other.label = "Separate recipient approval".into();
    let other = f.register(other).await;
    let op = f.run(other, DeliveryKind::Process).await;
    assert_eq!(
        f.settled(op.operation_id).await.state,
        DeliveryState::Denied
    );
    response(
        &f.broker,
        Some(&f.token),
        Request::RevokeConsent(ConsentQuery {
            grant_id: rows[0].grant_id,
        }),
    )
    .await;
    assert!(grants(&f).await.is_empty());
    let op = f.run(profile, DeliveryKind::Process).await;
    assert_eq!(
        f.settled(op.operation_id).await.state,
        DeliveryState::Denied
    );
    assert_eq!(
        fs::read_to_string(f.root.path().join("runs"))
            .unwrap()
            .lines()
            .count(),
        4
    );
    f.broker.quiesce().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn clear_during_prompt_rejects_late_always_and_never_persists_it() {
    let f = Fixture::new().await;
    let profile = f.register(f.process()).await;
    f.human.mode.store(4, Ordering::SeqCst);
    let op = f.run(profile, DeliveryKind::Process).await;
    tokio::time::timeout(Duration::from_secs(2), async {
        while f.human.uses.load(Ordering::SeqCst) == 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    response(&f.broker, Some(&f.token), Request::ClearConsents).await;
    assert_eq!(
        f.settled(op.operation_id).await.state,
        DeliveryState::Cancelled
    );
    assert!(grants(&f).await.is_empty());
    assert!(!f.root.path().join("runs").exists());
    let persisted: Registry =
        serde_json::from_slice(&fs::read(f.root.path().join("clients.json")).unwrap()).unwrap();
    assert!(persisted.consents.is_empty());
    f.broker.quiesce().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn grant_survives_restart_but_not_profile_removal_or_executable_mutation() {
    let f = Fixture::new().await;
    let profile = f.register(f.process()).await;
    f.human.mode.store(3, Ordering::SeqCst);
    let op = f.run(profile, DeliveryKind::Process).await;
    assert_eq!(
        f.settled(op.operation_id).await.state,
        DeliveryState::Completed
    );
    f.broker.quiesce().await;
    let Fixture {
        root,
        broker,
        human,
        token,
        key,
        reference,
    } = f;
    drop(broker);
    human.mode.store(1, Ordering::SeqCst);
    let store = SecretStore::new(Box::new(Key(key.clone())), root.path().join("vault")).unwrap();
    let broker =
        Broker::with_components(storage::open(root.path()).unwrap(), store, human.clone()).unwrap();
    let f = Fixture {
        root,
        broker,
        human,
        token,
        key,
        reference,
    };
    assert_eq!(grants(&f).await.len(), 1);
    let op = f.run(profile, DeliveryKind::Process).await;
    assert_eq!(
        f.settled(op.operation_id).await.state,
        DeliveryState::Completed
    );
    assert_eq!(f.human.uses.load(Ordering::SeqCst), 1);
    fs::write(f.root.path().join("recipient"), "#!/bin/sh\nexit 0\n").unwrap();
    let op = f.run(profile, DeliveryKind::Process).await;
    let status = f.settled(op.operation_id).await;
    assert_eq!(status.state, DeliveryState::Failed);
    assert!(!status.may_have_run);
    assert_eq!(
        fs::read_to_string(f.root.path().join("runs"))
            .unwrap()
            .lines()
            .count(),
        2
    );
    response(
        &f.broker,
        Some(&f.token),
        Request::RemoveDeliveryProfile(ProfileQuery {
            profile_id: profile,
        }),
    )
    .await;
    assert!(grants(&f).await.is_empty());
    f.broker.quiesce().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn another_pairing_cannot_list_or_revoke_a_remembered_use() {
    let f = Fixture::new().await;
    let profile = f.register(f.process()).await;
    f.human.mode.store(3, Ordering::SeqCst);
    let op = f.run(profile, DeliveryKind::Process).await;
    f.settled(op.operation_id).await;
    let grant = grants(&f).await.remove(0);
    let Response::Paired(other) = response(
        &f.broker,
        None,
        Request::Pair(PairRequest {
            label: "Other consent owner".into(),
        }),
    )
    .await
    else {
        panic!("pair")
    };
    let Response::Consents(rows) =
        response(&f.broker, Some(&other.token), Request::ListConsents).await
    else {
        panic!("list")
    };
    assert!(rows.is_empty());
    assert!(matches!(
        request_raw(
            &f.broker,
            Some(&other.token),
            Request::RevokeConsent(ConsentQuery {
                grant_id: grant.grant_id
            })
        )
        .await,
        Reply::Error(ErrorCode::NotFound)
    ));
    response(&f.broker, Some(&other.token), Request::ClearConsents).await;
    assert_eq!(grants(&f).await.len(), 1);
    f.broker.quiesce().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn registry_write_failure_blocks_always_before_recipient_dispatch() {
    let f = Fixture::new().await;
    let profile = f.register(f.process()).await;
    // Synthetic fault: the destination of the atomic registry write is invalid.
    let registry = f.root.path().join("clients.json");
    fs::remove_file(&registry).unwrap();
    fs::create_dir(&registry).unwrap();
    f.human.mode.store(3, Ordering::SeqCst);
    let op = f.run(profile, DeliveryKind::Process).await;
    let status = f.settled(op.operation_id).await;
    assert_eq!(status.error, Some(ErrorCode::PersistenceUncertain));
    assert!(!status.may_have_run);
    assert!(!f.root.path().join("runs").exists());
    assert!(matches!(
        request_raw(
            &f.broker,
            Some(&f.token),
            Request::SecureNewProcess(SecureDelivery {
                operation_id: Uuid::new_v4(),
                profile_id: profile
            })
        )
        .await,
        Reply::Error(ErrorCode::PersistenceUncertain)
    ));
    f.broker.quiesce().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn remembered_http_use_still_withholds_recipient_echo_and_can_return_to_per_use() {
    let f = Fixture::new().await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let profile = f
        .register(DeliveryProfile {
            label: "Remembered HTTP fixture".into(),
            destination: DeliveryDestination::Http(HttpDestination {
                url: format!("http://{}/", listener.local_addr().unwrap()),
                method: "POST".into(),
                headers: vec![NamedValue {
                    name: "Authorization".into(),
                    value: f.value(),
                }],
                query: vec![],
                body: None,
                timeout_secs: 2,
            }),
        })
        .await;
    let server = tokio::spawn(async move {
        for _ in 0..2 {
            let (mut stream, _) = tokio::time::timeout(Duration::from_secs(5), listener.accept())
                .await
                .unwrap()
                .unwrap();
            let mut request = Vec::new();
            let mut buffer = [0u8; 2048];
            while !request.windows(4).any(|w| w == b"\r\n\r\n") {
                let read = stream.read(&mut buffer).await.unwrap();
                assert!(read > 0);
                request.extend_from_slice(&buffer[..read]);
                assert!(request.len() < 8192);
            }
            assert!(String::from_utf8_lossy(&request).contains(CANARY));
            stream.write_all(format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{CANARY}", CANARY.len()).as_bytes()).await.unwrap();
        }
    });
    f.human.mode.store(3, Ordering::SeqCst);
    let first = f.run(profile, DeliveryKind::Http).await;
    assert_eq!(
        f.settled(first.operation_id).await.state,
        DeliveryState::Completed
    );
    f.human.mode.store(1, Ordering::SeqCst);
    let next = f.run(profile, DeliveryKind::Http).await;
    let status = f.settled(next.operation_id).await;
    assert_eq!(status.state, DeliveryState::Completed);
    assert!(!serde_json::to_string(&status).unwrap().contains(CANARY));
    assert_eq!(f.human.uses.load(Ordering::SeqCst), 1);
    server.await.unwrap();
    response(&f.broker, Some(&f.token), Request::ClearConsents).await;
    let denied = f.run(profile, DeliveryKind::Http).await;
    assert_eq!(
        f.settled(denied.operation_id).await.state,
        DeliveryState::Denied
    );
    f.broker.quiesce().await;
}
