use super::*;

async fn use_field(f: &Fixture, css: &str) -> FillStatus {
    let mut request = f.request().await;
    request.fields[0].css = css.into();
    let id = request.operation_id;
    f.broker.secure_fill(f.auth.clone(), request).await.unwrap();
    f.settled(id).await
}
async fn grants(f: &Fixture) -> Vec<ConsentGrantInfo> {
    let Response::Consents(rows) = invoke(&f.broker, Some(&f.token), Request::ListConsents)
        .await
        .unwrap()
    else {
        panic!("grants")
    };
    rows
}

#[tokio::test]
async fn exact_mapping_is_remembered_but_changed_selector_needs_new_consent() {
    let f = Fixture::new().await;
    f.configure(vec!["https://example.com".into()]).await;
    f.human.remember.store(true, Ordering::SeqCst);
    assert_eq!(use_field(&f, "#password").await.state, FillState::Filled);
    assert_eq!(grants(&f).await.len(), 1);
    f.human.deny_fill.store(true, Ordering::SeqCst);
    assert_eq!(use_field(&f, "#password").await.state, FillState::Filled);
    assert_eq!(f.human.prompts.load(Ordering::SeqCst), 1);
    assert_eq!(use_field(&f, "#another").await.state, FillState::Denied);
    assert_eq!(f.human.prompts.load(Ordering::SeqCst), 2);
    let grant = grants(&f).await.remove(0);
    invoke(
        &f.broker,
        Some(&f.token),
        Request::RevokeConsent(ConsentQuery {
            grant_id: grant.grant_id,
        }),
    )
    .await
    .unwrap();
    assert_eq!(use_field(&f, "#password").await.state, FillState::Denied);
    assert_eq!(f.adapter.calls.load(Ordering::SeqCst), 2);
    f.broker.quiesce().await;
}

#[tokio::test]
async fn rule_reconfiguration_and_disconnect_forget_browser_consents() {
    let f = Fixture::new().await;
    f.configure(vec!["https://example.com".into()]).await;
    f.human.remember.store(true, Ordering::SeqCst);
    assert_eq!(use_field(&f, "#password").await.state, FillState::Filled);
    f.configure(vec!["https://example.com".into()]).await;
    assert!(grants(&f).await.is_empty());
    assert_eq!(use_field(&f, "#password").await.state, FillState::Filled);
    f.broker
        .disconnect_browser(
            f.auth.clone(),
            Uuid::new_v4(),
            BrowserQuery {
                browser_handle: f.browser,
            },
        )
        .await
        .unwrap();
    assert!(grants(&f).await.is_empty());
    f.broker.quiesce().await;
}

#[tokio::test]
async fn cdp_grant_is_not_restored_by_daemon_restart() {
    let f = Fixture::new().await;
    f.configure(vec!["https://example.com".into()]).await;
    f.human.remember.store(true, Ordering::SeqCst);
    assert_eq!(use_field(&f, "#password").await.state, FillState::Filled);
    f.broker.quiesce().await;
    let Fixture {
        root,
        broker,
        human,
        token,
        ..
    } = f;
    drop(broker);
    let store = SecretStore::new(Box::new(FixtureKey), root.path().join("vault")).unwrap();
    let broker =
        Broker::with_components(storage::open(root.path()).unwrap(), store, human).unwrap();
    let Response::Consents(rows) = invoke(&broker, Some(&token), Request::ListConsents)
        .await
        .unwrap()
    else {
        panic!("list")
    };
    assert!(rows.is_empty());
    broker.quiesce().await;
}

#[tokio::test]
async fn extension_grant_survives_restart_only_for_the_same_profile() {
    let f = Fixture::new().await;
    f.configure(vec!["https://example.com".into()]).await;
    let identity = NativeIdentity {
        profile_id: Uuid::new_v4(),
        extension_id: crate::native::EXTENSION_ID.into(),
    };
    let (identity2, auth, browser) = (identity.clone(), f.auth.clone(), f.browser);
    // Test-only native identity fixture; real native admission is covered by
    // native_tests and the opt-in Chrome lane. No production bypass is added.
    f.broker
        .transaction(move |b, s| {
            s.registry.native_grants.push(NativeGrant {
                client_id: auth.id,
                extension_id: identity2.extension_id.clone(),
                profile_id: identity2.profile_id,
                token_hash: [3; 32],
                allowed: true,
            });
            s.browsers
                .instances
                .get_mut(&browser)
                .unwrap()
                .native_profile = Some(identity2);
            b.save(s)
        })
        .await
        .unwrap();
    f.human.remember.store(true, Ordering::SeqCst);
    assert_eq!(use_field(&f, "#password").await.state, FillState::Filled);
    f.broker.quiesce().await;
    let Fixture {
        root,
        broker,
        human,
        adapter,
        auth,
        token,
        reference,
        ..
    } = f;
    drop(broker);
    let store = SecretStore::new(Box::new(FixtureKey), root.path().join("vault")).unwrap();
    let broker =
        Broker::with_components(storage::open(root.path()).unwrap(), store, human.clone()).unwrap();
    adapter.alive.store(true, Ordering::SeqCst);
    let browser = Uuid::new_v4();
    broker
        .register_adapter(
            auth.clone(),
            Uuid::new_v4(),
            BrowserInfo {
                browser_handle: browser,
                label: "Remembered native profile".into(),
                backend: BrowserBackend::Extension,
            },
            adapter.clone(),
            Some(identity),
        )
        .await
        .unwrap();
    let f = Fixture {
        root,
        broker,
        human,
        adapter,
        auth,
        token,
        reference,
        browser,
    };
    f.human.deny_fill.store(true, Ordering::SeqCst);
    assert_eq!(use_field(&f, "#password").await.state, FillState::Filled);
    assert_eq!(f.human.prompts.load(Ordering::SeqCst), 1);
    // A different native profile never inherits the grant, even for identical
    // origins, credentials and selectors.
    let browser = f.browser;
    f.broker
        .transaction(move |_, s| {
            s.browsers
                .instances
                .get_mut(&browser)
                .unwrap()
                .native_profile
                .as_mut()
                .unwrap()
                .profile_id = Uuid::new_v4();
            Ok(())
        })
        .await
        .unwrap();
    assert_eq!(use_field(&f, "#password").await.state, FillState::Denied);
    assert_eq!(f.human.prompts.load(Ordering::SeqCst), 2);
    f.broker.quiesce().await;
}

#[tokio::test]
async fn changed_frame_origin_kind_and_field_never_match_a_remembered_fill() {
    let f = Fixture::new().await;
    f.configure(vec![
        "https://example.com".into(),
        "https://frame.example".into(),
    ])
    .await;
    f.human.remember.store(true, Ordering::SeqCst);
    assert_eq!(use_field(&f, "#password").await.state, FillState::Filled);
    f.human.deny_fill.store(true, Ordering::SeqCst);
    for variant in 0..3 {
        let mut request = f.request().await;
        let handle = request.target_handle;
        if variant == 2 {
            request.fields[0].credential_field = "username".into();
        }
        f.broker
            .transaction(move |_, s| {
                let target = &mut s.browsers.targets.get_mut(&handle).unwrap().target;
                if variant == 0 {
                    target.origin = "https://frame.example".into();
                    target.is_main_frame = false;
                }
                if variant == 1 {
                    target.is_main_frame = false;
                }
                Ok(())
            })
            .await
            .unwrap();
        let id = request.operation_id;
        f.broker.secure_fill(f.auth.clone(), request).await.unwrap();
        assert_eq!(f.settled(id).await.state, FillState::Denied);
    }
    assert_eq!(f.adapter.calls.load(Ordering::SeqCst), 1);
    f.broker.quiesce().await;
}
