//! Isolated native transport admission; no keychain or real browser profiles.
use super::*;
use crate::native::{self, BrowserHello, HostConfig, NativeGreeting, NativeHello, NATIVE_VERSION};
use magicvault_effect::bridge::{read_frame, write_frame, BRIDGE_VERSION};
use std::{fs, os::unix::fs::PermissionsExt, sync::atomic::Ordering};

fn hello(f: &Fixture, profile_id: Uuid) -> NativeHello {
    let config = HostConfig {
        version: BRIDGE_VERSION,
        root: f.root.path().to_owned(),
        instance_id: f.broker.lease.instance.id,
        profile: "native_fixture".into(),
        extension_id: native::EXTENSION_ID.into(),
        executable: "/tmp/synthetic-native-host".into(),
    };
    for (path, bytes) in [
        (
            native::config_path(f.root.path()),
            serde_json::to_vec(&config).unwrap(),
        ),
        (
            f.root.path().join("client-native_fixture.json"),
            serde_json::to_vec(&Pairing {
                client_id: f.auth.id,
                token: f.token.clone(),
            })
            .unwrap(),
        ),
    ] {
        fs::write(&path, bytes).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
    }
    let mut hello = native::hello(&config).unwrap();
    hello.browser = Some(BrowserHello {
        version: NATIVE_VERSION,
        profile_id,
        capability: "c".repeat(64),
        manual: false,
    });
    hello
}

async fn connect(f: &Fixture, hello: &NativeHello) -> (tokio::net::UnixStream, NativeGreeting) {
    let (stream, mut host) = tokio::net::UnixStream::pair().unwrap();
    let broker = f.broker.clone();
    let task = tokio::spawn(async move { broker.accept_native(stream).await });
    write_frame(&mut host, &serde_json::to_vec(hello).unwrap())
        .await
        .unwrap();
    let bytes = tokio::time::timeout(Duration::from_secs(3), read_frame(&mut host))
        .await
        .unwrap()
        .unwrap();
    let text = std::str::from_utf8(&bytes).unwrap();
    assert!(!text.contains(&hello.token));
    assert!(!text.contains(&hello.browser.as_ref().unwrap().capability));
    let greeting = serde_json::from_slice(&bytes).unwrap();
    let _ = task.await.unwrap();
    (host, greeting)
}
fn handle(greeting: NativeGreeting) -> Uuid {
    let NativeGreeting::Ready {
        version: NATIVE_VERSION,
        browser_handle,
    } = greeting
    else {
        panic!("expected ready");
    };
    browser_handle
}
fn error(greeting: NativeGreeting, expected: ErrorCode) {
    let NativeGreeting::Error {
        version: NATIVE_VERSION,
        code,
    } = greeting
    else {
        panic!("expected refusal");
    };
    assert_eq!(code, expected);
}

#[tokio::test]
async fn final_registration_revalidates_the_exact_extension_grant() {
    let f = Fixture::new().await;
    let first = hello(&f, Uuid::new_v4());
    let (host, ready) = connect(&f, &first).await;
    handle(ready);
    drop(host);
    let profile = first.browser.as_ref().unwrap().profile_id;
    let result = f
        .broker
        .register_adapter(
            f.auth.clone(),
            Uuid::new_v4(),
            BrowserInfo {
                browser_handle: Uuid::new_v4(),
                label: "synthetic other extension".into(),
                backend: BrowserBackend::Extension,
            },
            f.adapter.clone(),
            Some(NativeIdentity {
                profile_id: profile,
                extension_id: "b".repeat(32),
            }),
        )
        .await;
    assert!(matches!(result, Err(ErrorCode::Denied)));
}

#[tokio::test]
async fn fresh_daemon_reloads_native_approval_but_not_browser_handles_or_effect_authority() {
    let mut f = Fixture::new().await;
    let first = hello(&f, Uuid::new_v4());
    let (host, ready) = connect(&f, &first).await;
    let old_handle = handle(ready);
    let epoch = f.broker.epoch;
    f.broker.quiesce().await;
    drop(host);
    let restarted = Broker::with_components(
        f.broker.lease.clone(),
        SecretStore::new(Box::new(FixtureKey), f.root.path().join("vault")).unwrap(),
        f.human.clone(),
    )
    .unwrap();
    f.broker = restarted;
    assert_ne!(epoch, f.broker.epoch);
    assert!(f.broker.state.lock().unwrap().browsers.instances.is_empty());
    let (_host, ready) = connect(&f, &first).await;
    assert_ne!(handle(ready), old_handle);
    assert_eq!(f.human.native_prompts.load(Ordering::SeqCst), 1);
    assert!(f.broker.state.lock().unwrap().browsers.targets.is_empty());
}

#[tokio::test]
async fn independent_profiles_coexist_and_duplicate_identity_never_evicts_a_live_browser() {
    let f = Fixture::new().await;
    let first = hello(&f, Uuid::new_v4());
    let second = hello(&f, Uuid::new_v4());
    let (a, ready) = connect(&f, &first).await;
    let a_handle = handle(ready);
    let (_b, ready) = connect(&f, &second).await;
    let b_handle = handle(ready);
    assert_ne!(a_handle, b_handle);
    let (_, refused) = connect(&f, &first).await;
    error(refused, ErrorCode::Conflict);
    assert_eq!(f.human.native_prompts.load(Ordering::SeqCst), 2);
    assert!(f
        .broker
        .state
        .lock()
        .unwrap()
        .browsers
        .instances
        .contains_key(&a_handle));
    drop(a);
    // EOF is detected even without another browser request.
    let (_reconnected, ready) = connect(&f, &first).await;
    let fresh = handle(ready);
    assert_ne!(fresh, a_handle);
    assert_eq!(f.human.native_prompts.load(Ordering::SeqCst), 2);
    let state = f.broker.state.lock().unwrap();
    assert!(!state.browsers.instances.contains_key(&a_handle));
    assert!(state.browsers.instances.contains_key(&b_handle));
    assert!(state.browsers.instances.contains_key(&f.browser)); // CDP untouched.
}

#[tokio::test]
async fn refusal_is_durable_manual_retry_requires_consent_and_disconnect_revokes_only_its_profile()
{
    let f = Fixture::new().await;
    let mut first = hello(&f, Uuid::new_v4());
    f.human.deny_native.store(true, Ordering::SeqCst);
    error(connect(&f, &first).await.1, ErrorCode::Denied);
    error(connect(&f, &first).await.1, ErrorCode::Denied);
    assert_eq!(f.human.native_prompts.load(Ordering::SeqCst), 1);
    // Reload the persisted control registry as a fresh daemon would.
    let bytes = storage::read_private(
        &f.root.path().join("clients.json"),
        storage::MAX_STATE_BYTES,
    )
    .unwrap();
    let registry: Registry = serde_json::from_slice(&bytes).unwrap();
    assert!(valid_native_grants(&registry));
    assert!(!String::from_utf8(bytes.to_vec())
        .unwrap()
        .contains(&first.browser.as_ref().unwrap().capability));
    f.broker.state.lock().unwrap().registry = registry;
    error(connect(&f, &first).await.1, ErrorCode::Denied);
    first.browser.as_mut().unwrap().manual = true;
    f.human.deny_native.store(false, Ordering::SeqCst);
    let (_a, ready) = connect(&f, &first).await;
    let a_handle = handle(ready);
    let second = hello(&f, Uuid::new_v4());
    let (_b, ready) = connect(&f, &second).await;
    let b_handle = handle(ready);
    f.broker
        .disconnect_browser(
            f.auth.clone(),
            Uuid::new_v4(),
            BrowserQuery {
                browser_handle: a_handle,
            },
        )
        .await
        .unwrap();
    first.browser.as_mut().unwrap().manual = false;
    error(connect(&f, &first).await.1, ErrorCode::Denied);
    assert!(f
        .broker
        .state
        .lock()
        .unwrap()
        .browsers
        .instances
        .contains_key(&b_handle));
    assert_eq!(f.human.native_prompts.load(Ordering::SeqCst), 3);
    first.browser.as_mut().unwrap().capability = "d".repeat(64);
    first.browser.as_mut().unwrap().manual = true;
    error(connect(&f, &first).await.1, ErrorCode::Unauthorized);
}

#[tokio::test]
async fn remembered_reconnect_does_not_contend_for_human_prompt_and_client_revocation_stays_revoked(
) {
    let f = Fixture::new().await;
    let first = hello(&f, Uuid::new_v4());
    let (host, ready) = connect(&f, &first).await;
    handle(ready);
    drop(host);
    let permit = f.broker.human_gate.clone().acquire_owned().await.unwrap();
    let (host, ready) = connect(&f, &first).await;
    handle(ready);
    let second = hello(&f, Uuid::new_v4());
    error(connect(&f, &second).await.1, ErrorCode::Busy);
    assert_eq!(f.human.native_prompts.load(Ordering::SeqCst), 1);
    drop(permit);
    drop(host);
    super::invoke(
        &f.broker,
        Some(&f.token),
        Request::RevokeClient(RevokeRequest {
            client_id: f.auth.id,
        }),
    )
    .await
    .unwrap();
    error(connect(&f, &first).await.1, ErrorCode::Unauthorized);
    assert!(f
        .broker
        .state
        .lock()
        .unwrap()
        .registry
        .native_grants
        .is_empty());
}

#[tokio::test]
async fn wrong_version_instance_capability_and_extension_are_closed_errors_without_consent() {
    let f = Fixture::new().await;
    let mut first = hello(&f, Uuid::new_v4());
    first.version = 1;
    error(connect(&f, &first).await.1, ErrorCode::UnsupportedVersion);
    first.version = NATIVE_VERSION;
    first.instance_id = Uuid::new_v4();
    error(connect(&f, &first).await.1, ErrorCode::Unauthorized);
    first.instance_id = f.broker.lease.instance.id;
    first.extension_id = "b".repeat(32);
    error(connect(&f, &first).await.1, ErrorCode::Unauthorized);
    first.extension_id = native::EXTENSION_ID.into();
    first.token = "0".repeat(64);
    error(connect(&f, &first).await.1, ErrorCode::Unauthorized);
    assert_eq!(f.human.native_prompts.load(Ordering::SeqCst), 0);
}
