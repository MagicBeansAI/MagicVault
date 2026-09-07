#![cfg(unix)]
use async_trait::async_trait;
use magicvault_core::{store::SecretStore, InMemoryKeyProvider};
use magicvault_effect::{bridge::*, Target};
use magicvault_service::{
    broker::Broker,
    client::Client,
    human::HumanInteraction,
    ipc,
    native::{config_path, HostConfig},
    protocol::*,
    storage,
};
use std::{fs, os::unix::fs::PermissionsExt, sync::Arc, time::Duration};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;
use zeroize::Zeroizing;

struct Human;
#[async_trait]
impl HumanInteraction for Human {
    async fn confirm(&self, _: &str, _: CancellationToken) -> Result<bool, ErrorCode> {
        Ok(true)
    }
    async fn secret(&self, _: &str, _: CancellationToken) -> Result<Zeroizing<String>, ErrorCode> {
        Ok(Zeroizing::new("SYNTHETIC-HOST-CANARY".into()))
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn shipped_native_host_authenticates_and_bridges_an_actual_daemon_fill() {
    let root = tempfile::Builder::new()
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir()
        .unwrap();
    let instance = storage::Instance {
        format_version: 1,
        id: Uuid::new_v4(),
    };
    fs::write(
        root.path().join("instance.json"),
        serde_json::to_vec(&instance).unwrap(),
    )
    .unwrap();
    fs::set_permissions(
        root.path().join("instance.json"),
        fs::Permissions::from_mode(0o600),
    )
    .unwrap();
    let broker = Broker::with_components(
        storage::open(root.path()).unwrap(),
        SecretStore::new_empty(
            Box::new(InMemoryKeyProvider::new()),
            root.path().join("vault"),
        ),
        Arc::new(Human),
    )
    .unwrap();
    let daemon = tokio::spawn(ipc::serve(Arc::clone(&broker)));
    tokio::time::timeout(Duration::from_secs(2), async {
        while !root.path().join("bridge.sock").exists() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    Client::pair(
        root.path().to_owned(),
        "extension",
        "Extension fixture".into(),
    )
    .await
    .unwrap();
    let client = Client::load(root.path().to_owned(), "extension").unwrap();
    let Response::Enrolled(credential) = client
        .call(Request::Enroll(EnrollRequest {
            label: "Fixture".into(),
            field_names: vec!["password".into()],
        }))
        .await
        .unwrap()
    else {
        panic!("enroll");
    };
    client
        .call(Request::ConfigureBrowserCredential(BrowserRule {
            credential_ref: credential.credential_ref.clone(),
            origins: vec!["https://example.com".into()],
            field_names: vec!["password".into()],
        }))
        .await
        .unwrap();
    let extension_id = "a".repeat(32);
    let config = HostConfig {
        version: BRIDGE_VERSION,
        root: root.path().to_owned(),
        instance_id: instance.id,
        profile: "extension".into(),
        extension_id: extension_id.clone(),
        executable: std::env::var_os("MAGICVAULT_TEST_NATIVE_HOST")
            .unwrap_or_else(|| env!("CARGO_BIN_EXE_magicvault-native-host").into()).into(),
    };
    fs::write(
        config_path(root.path()),
        serde_json::to_vec(&config).unwrap(),
    )
    .unwrap();
    fs::set_permissions(config_path(root.path()), fs::Permissions::from_mode(0o600)).unwrap();
    let mut host = tokio::process::Command::new(std::env::var_os("MAGICVAULT_TEST_NATIVE_HOST")
        .unwrap_or_else(|| env!("CARGO_BIN_EXE_magicvault-native-host").into()))
        .arg("--config")
        .arg(config_path(root.path()))
        .arg("--")
        .arg(format!("chrome-extension://{extension_id}/"))
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let mut read = host.stdout.take().unwrap();
    let mut write = host.stdin.take().unwrap();
    let greeting = tokio::time::timeout(Duration::from_secs(3), read_frame(&mut read))
        .await
        .unwrap()
        .unwrap();
    let greeting: serde_json::Value = serde_json::from_slice(&greeting).unwrap();
    assert_eq!(greeting["kind"], "ready");
    assert!(!greeting.to_string().contains("token"));
    assert!(!greeting.to_string().contains("CANARY"));
    let browser_handle: Uuid = serde_json::from_value(greeting["browser_handle"].clone()).unwrap();
    let document = Uuid::new_v4().to_string();
    let target = Target {
        tab: "1".into(),
        frame: "0".into(),
        document: document.clone(),
        top_document: document,
        origin: "https://example.com".into(),
        top_origin: "https://example.com".into(),
        is_main_frame: true,
    };
    let extension = tokio::spawn(async move {
        let bytes = read_frame(&mut read).await.unwrap();
        let request: BridgeCommand = serde_json::from_slice(&bytes).unwrap();
        assert!(matches!(request.request, BridgeRequest::Targets));
        write_frame(
            &mut write,
            &serde_json::to_vec(&BridgeReply {
                request_id: request.request_id,
                result: BridgeResult::Targets(vec![target]),
            })
            .unwrap(),
        )
        .await
        .unwrap();
        let bytes = read_frame(&mut read).await.unwrap();
        let request: BridgeCommand = serde_json::from_slice(&bytes).unwrap();
        let BridgeRequest::Fill { fields, .. } = &request.request else {
            panic!("fill");
        };
        assert_eq!(fields[0].value, "SYNTHETIC-HOST-CANARY");
        let reply = BridgeReply {
            request_id: request.request_id,
            result: BridgeResult::Filled(magicvault_effect::Outcome {
                fields: vec![FieldState::Filled],
                error: None,
            }),
        };
        let bytes = serde_json::to_vec(&reply).unwrap();
        assert!(!String::from_utf8_lossy(&bytes).contains("CANARY"));
        write_frame(&mut write, &bytes).await.unwrap();
        (read, write)
    });
    let Response::BrowserTargets(targets) = client
        .call(Request::BrowserTargets(BrowserQuery { browser_handle }))
        .await
        .unwrap()
    else {
        panic!("targets");
    };
    let request = SecureFill {
        operation_id: Uuid::new_v4(),
        browser_handle,
        target_handle: targets[0].target_handle,
        fields: vec![FillField {
            css: "#password".into(),
            credential_ref: credential.credential_ref,
            credential_field: "password".into(),
        }],
    };
    client
        .call(Request::SecureFill(request.clone()))
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let Response::Fill(status) = client
                .call(Request::FillStatus(FillQuery {
                    operation_id: request.operation_id,
                }))
                .await
                .unwrap()
            else {
                panic!("status");
            };
            if !matches!(status.state, FillState::Pending | FillState::Filling) {
                assert_eq!(status.state, FillState::Filled);
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let (_read, write) = extension.await.unwrap();
    drop(write);
    tokio::time::timeout(Duration::from_secs(3), host.wait())
        .await
        .unwrap()
        .unwrap();
    use tokio::io::AsyncReadExt;
    let mut stderr = String::new();
    host.stderr
        .take()
        .unwrap()
        .read_to_string(&mut stderr)
        .await
        .unwrap();
    assert!(stderr.is_empty());
    broker.shutdown.cancel();
    daemon.await.unwrap().unwrap();
}

#[tokio::test]
async fn native_host_rejected_arguments_are_never_echoed() {
    let output = tokio::process::Command::new(std::env::var_os("MAGICVAULT_TEST_NATIVE_HOST")
        .unwrap_or_else(|| env!("CARGO_BIN_EXE_magicvault-native-host").into()))
        .args(["--token", "SYNTHETIC-REJECTED-CANARY"])
        .output()
        .await
        .unwrap();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());
}
