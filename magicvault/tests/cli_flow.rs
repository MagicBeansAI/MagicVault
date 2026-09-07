#![cfg(unix)]
use async_trait::async_trait;
use magicvault_core::{store::SecretStore, InMemoryKeyProvider};
use magicvault_service::{
    broker::Broker, human::HumanInteraction, ipc, protocol::ErrorCode, storage,
};
use std::{fs, os::unix::fs::PermissionsExt, sync::Arc, time::Duration};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;
use zeroize::Zeroizing;

#[test]
fn dependency_payload_logging_is_compiled_out_of_shipped_binaries() {
    assert_eq!(log::STATIC_MAX_LEVEL, log::LevelFilter::Off);
}

struct FixtureHuman;
#[async_trait]
impl HumanInteraction for FixtureHuman {
    async fn confirm(&self, _: &str, _: CancellationToken) -> Result<bool, ErrorCode> {
        Ok(true)
    }
    async fn secret(&self, _: &str, _: CancellationToken) -> Result<Zeroizing<String>, ErrorCode> {
        Ok(Zeroizing::new("SYNTHETIC-CLI-FLOW-CANARY".into()))
    }
}
#[tokio::test(flavor = "multi_thread")]
async fn actual_cli_pairs_enrolls_and_discovers_through_the_daemon() {
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
    let lease = storage::open(root.path()).unwrap();
    let store = SecretStore::new_empty(
        Box::new(InMemoryKeyProvider::new()),
        root.path().join("vault"),
    );
    let broker = Broker::with_components(lease, store, Arc::new(FixtureHuman)).unwrap();
    let daemon = tokio::spawn(ipc::serve(Arc::clone(&broker)));
    tokio::time::timeout(Duration::from_secs(2), async {
        while !broker.socket().exists() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    for args in [
        vec!["pair", "--label", "CLI fixture"],
        vec!["enroll", "--label", "Fixture", "--field", "password"],
        vec!["list-credentials"],
        vec!["status"],
    ] {
        let output = tokio::process::Command::new(env!("CARGO_BIN_EXE_magicvault"))
            .arg("--root")
            .arg(root.path())
            .args(args)
            .output()
            .await
            .unwrap();
        assert!(
            output.status.success(),
            "CLI failed with a closed diagnostic"
        );
        assert!(!String::from_utf8_lossy(&output.stdout).contains("SYNTHETIC-CLI-FLOW-CANARY"));
        assert!(!String::from_utf8_lossy(&output.stderr).contains("SYNTHETIC-CLI-FLOW-CANARY"));
        let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert!(value.is_object());
    }
    broker.shutdown.cancel();
    daemon.await.unwrap().unwrap();
}

#[tokio::test]
async fn rejected_arguments_never_echo_the_rejected_value() {
    for args in [
        vec!["enroll", "--password", "SYNTHETIC-REJECTED-CANARY"],
        vec![
            "approval-status",
            "--approval-id",
            "SYNTHETIC-REJECTED-CANARY",
        ],
    ] {
        let output = tokio::process::Command::new(env!("CARGO_BIN_EXE_magicvault"))
            .args(args)
            .output()
            .await
            .unwrap();
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        assert_eq!(
            String::from_utf8(output.stderr).unwrap(),
            "{\"error\":\"invalid_request\"}\n"
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn actual_cli_secure_fill_reaches_dedicated_cdp_and_returns_no_material() {
    use magicvault_service::{client::Client, protocol::*};
    use magicvault_test_support::CdpFixture;
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
    let lease = storage::open(root.path()).unwrap();
    let store = SecretStore::new_empty(
        Box::new(InMemoryKeyProvider::new()),
        root.path().join("vault"),
    );
    let broker = Broker::with_components(lease, store, Arc::new(FixtureHuman)).unwrap();
    let daemon = tokio::spawn(ipc::serve(Arc::clone(&broker)));
    tokio::time::timeout(Duration::from_secs(2), async {
        while !broker.socket().exists() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    Client::pair(
        root.path().to_owned(),
        "default",
        "CLI browser fixture".into(),
    )
    .await
    .unwrap();
    let client = Client::load(root.path().to_owned(), "default").unwrap();
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
    let cdp = CdpFixture::start().await;
    let Response::Browser(browser) = client
        .call(Request::RegisterCdp(RegisterCdp {
            label: "Fixture".into(),
            endpoint: cdp.endpoint.clone(),
        }))
        .await
        .unwrap()
    else {
        panic!("browser");
    };
    let Response::BrowserTargets(targets) = client
        .call(Request::BrowserTargets(BrowserQuery {
            browser_handle: browser.browser_handle,
        }))
        .await
        .unwrap()
    else {
        panic!("targets");
    };
    let request = SecureFill {
        operation_id: Uuid::new_v4(),
        browser_handle: browser.browser_handle,
        target_handle: targets[0].target_handle,
        fields: vec![FillField {
            css: "#password".into(),
            credential_ref: credential.credential_ref,
            credential_field: "password".into(),
        }],
    };
    let request_file = root.path().join("reference-only-fill.json");
    fs::write(&request_file, serde_json::to_vec(&request).unwrap()).unwrap();
    let output = tokio::process::Command::new(env!("CARGO_BIN_EXE_magicvault"))
        .arg("--root")
        .arg(root.path())
        .args(["secure-fill", "--request-file"])
        .arg(request_file)
        .output()
        .await
        .unwrap();
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    assert!(!String::from_utf8_lossy(&output.stdout).contains("SYNTHETIC-CLI-FLOW-CANARY"));
    let status = tokio::time::timeout(Duration::from_secs(3), async {
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
                break status;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert_eq!(status.state, FillState::Filled);
    assert_eq!(
        cdp.state.lock().unwrap().delivered,
        [vec!["SYNTHETIC-CLI-FLOW-CANARY".to_owned()]]
    );
    broker.shutdown.cancel();
    daemon.await.unwrap().unwrap();
}
