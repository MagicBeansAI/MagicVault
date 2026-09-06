#![cfg(unix)]
use std::{fs, os::unix::fs::PermissionsExt, sync::Arc, time::Duration};
use async_trait::async_trait;
use magicvault_core::{InMemoryKeyProvider, store::SecretStore};
use magicvault_service::{broker::Broker, human::HumanInteraction, ipc, protocol::ErrorCode, storage};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;
use zeroize::Zeroizing;

struct FixtureHuman;
#[async_trait]
impl HumanInteraction for FixtureHuman {
    async fn confirm(&self, _: &str, _: CancellationToken) -> Result<bool,ErrorCode> {Ok(true)}
    async fn secret(&self, _: &str, _: CancellationToken) -> Result<Zeroizing<String>,ErrorCode> {Ok(Zeroizing::new("SYNTHETIC-CLI-FLOW-CANARY".into()))}
}
#[tokio::test(flavor = "multi_thread")]
async fn actual_cli_pairs_enrolls_and_discovers_through_the_daemon() {
    let root = tempfile::Builder::new().permissions(fs::Permissions::from_mode(0o700)).tempdir().unwrap();
    let instance = storage::Instance {format_version:1,id:Uuid::new_v4()};
    fs::write(root.path().join("instance.json"),serde_json::to_vec(&instance).unwrap()).unwrap();
    fs::set_permissions(root.path().join("instance.json"),fs::Permissions::from_mode(0o600)).unwrap();
    let lease = storage::open(root.path()).unwrap();
    let store = SecretStore::new_empty(Box::new(InMemoryKeyProvider::new()),root.path().join("vault"));
    let broker = Broker::with_components(lease,store,Arc::new(FixtureHuman)).unwrap();
    let daemon = tokio::spawn(ipc::serve(Arc::clone(&broker)));
    tokio::time::timeout(Duration::from_secs(2),async {while !broker.socket().exists(){tokio::task::yield_now().await;}}).await.unwrap();
    for args in [vec!["pair","--label","CLI fixture"],vec!["enroll","--label","Fixture","--field","password"],vec!["list-credentials"],vec!["status"]] {
        let output = tokio::process::Command::new(env!("CARGO_BIN_EXE_magicvault")).arg("--root").arg(root.path()).args(args).output().await.unwrap();
        assert!(output.status.success(),"CLI failed with a closed diagnostic");
        assert!(!String::from_utf8_lossy(&output.stdout).contains("SYNTHETIC-CLI-FLOW-CANARY"));
        assert!(!String::from_utf8_lossy(&output.stderr).contains("SYNTHETIC-CLI-FLOW-CANARY"));
        let value:serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert!(value.is_object());
    }
    broker.shutdown.cancel();
    daemon.await.unwrap().unwrap();
}

#[tokio::test]
async fn rejected_arguments_never_echo_the_rejected_value() {
    for args in [vec!["enroll", "--password", "SYNTHETIC-REJECTED-CANARY"], vec!["approval-status", "--approval-id", "SYNTHETIC-REJECTED-CANARY"]] {
        let output = tokio::process::Command::new(env!("CARGO_BIN_EXE_magicvault")).args(args).output().await.unwrap();
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        assert_eq!(String::from_utf8(output.stderr).unwrap(), "{\"error\":\"invalid_request\"}\n");
    }
}
