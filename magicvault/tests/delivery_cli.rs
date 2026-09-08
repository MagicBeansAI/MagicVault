#![cfg(unix)]
use async_trait::async_trait;
use magicvault_core::{store::SecretStore, InMemoryKeyProvider};
use magicvault_service::{broker::Broker, human::HumanInteraction, ipc, protocol::*, storage};
use serde_json::{json, Value};
use std::{fs, os::unix::fs::PermissionsExt, path::Path, sync::Arc, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;
use zeroize::Zeroizing;

const CANARY: &str = "SYNTHETIC-CLI-DELIVERY-CANARY";
struct Human;
#[async_trait]
impl HumanInteraction for Human {
    async fn confirm(&self, message: &str, _: CancellationToken) -> Result<bool, ErrorCode> {
        assert!(!message.contains(CANARY));
        Ok(true)
    }
    async fn secret(&self, _: &str, _: CancellationToken) -> Result<Zeroizing<String>, ErrorCode> {
        Ok(Zeroizing::new(CANARY.into()))
    }
}
async fn cli(root: &Path, args: &[&str]) -> Value {
    let output = tokio::time::timeout(
        Duration::from_secs(5),
        tokio::process::Command::new(
            std::env::var_os("MAGICVAULT_TEST_CLI")
                .unwrap_or_else(|| env!("CARGO_BIN_EXE_magicvault").into()),
        )
        .arg("--root")
        .arg(root)
        .args(args)
        .kill_on_drop(true)
        .output(),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(
        output.status.success(),
        "CLI closed error: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!String::from_utf8_lossy(&output.stdout).contains(CANARY));
    assert!(!String::from_utf8_lossy(&output.stderr).contains(CANARY));
    serde_json::from_slice(&output.stdout).unwrap()
}
async fn settled(root: &Path, operation: &str) -> Value {
    tokio::time::timeout(Duration::from_secs(6), async {
        loop {
            let status = cli(root, &["delivery-status", "--operation-id", operation]).await;
            if !matches!(
                status["data"]["state"].as_str(),
                Some("pending" | "running")
            ) {
                return status;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap()
}
async fn setup() -> (
    tempfile::TempDir,
    Arc<Broker>,
    tokio::task::JoinHandle<Result<(), ErrorCode>>,
    String,
) {
    let root = tempfile::Builder::new()
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir()
        .unwrap();
    fs::write(
        root.path().join("instance.json"),
        serde_json::to_vec(&storage::Instance {
            format_version: 1,
            id: Uuid::new_v4(),
        })
        .unwrap(),
    )
    .unwrap();
    fs::set_permissions(
        root.path().join("instance.json"),
        fs::Permissions::from_mode(0o600),
    )
    .unwrap();
    let store = SecretStore::new_empty(
        Box::new(InMemoryKeyProvider::new()),
        root.path().join("vault"),
    );
    let broker =
        Broker::with_components(storage::open(root.path()).unwrap(), store, Arc::new(Human))
            .unwrap();
    let daemon = tokio::spawn(ipc::serve(broker.clone()));
    tokio::time::timeout(Duration::from_secs(3), async {
        while !broker.socket().exists() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    cli(root.path(), &["pair", "--label", "CLI delivery fixture"]).await;
    let enrolled = cli(
        root.path(),
        &["enroll", "--label", "Synthetic", "--field", "token"],
    )
    .await;
    let reference = enrolled["data"]["credential_ref"]
        .as_str()
        .unwrap()
        .to_owned();
    (root, broker, daemon, reference)
}
fn value(reference: &str) -> Value {
    json!({"kind":"credential","credential_ref":reference,"credential_field":"token"})
}
async fn register(root: &Path, profile: Value) -> String {
    let file = root.join("profile.json");
    fs::write(&file, serde_json::to_vec(&profile).unwrap()).unwrap();
    let response = cli(
        root,
        &[
            "register-delivery-profile",
            "--request-file",
            file.to_str().unwrap(),
        ],
    )
    .await;
    response["data"]["profile_id"].as_str().unwrap().to_owned()
}

#[tokio::test(flavor = "multi_thread")]
async fn actual_cli_to_daemon_to_magicrun_executes_without_echoing_material() {
    let (root, broker, daemon, reference) = setup().await;
    let executable = root.path().join("recipient");
    fs::write(&executable,"#!/bin/sh\n[ -n \"$MV_TOKEN\" ] || exit 1\nprintf '%s' \"$MV_TOKEN\"\nprintf 'done' > marker\n").unwrap();
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
    let profile=register(root.path(),json!({"label":"CLI process","destination":{"kind":"process","config":{
        "executable":executable,"arguments":[],"working_directory":root.path(),"environment":[{"name":"MV_TOKEN","value":value(&reference)}],"stdin":null,"timeout_secs":2}}})).await;
    let operation = Uuid::new_v4().to_string();
    cli(
        root.path(),
        &[
            "secure-new-process",
            "--profile-id",
            &profile,
            "--operation-id",
            &operation,
        ],
    )
    .await;
    let status = settled(root.path(), &operation).await;
    assert_eq!(
        status["data"]["state"],
        "completed",
        "closed error: {}; may_have_run: {}; recipient marker exists: {}",
        status["data"]["error"],
        status["data"]["may_have_run"],
        root.path().join("marker").exists()
    );
    assert_eq!(
        fs::read_to_string(root.path().join("marker")).unwrap(),
        "done"
    );
    cli(
        root.path(),
        &["remove-delivery-profile", "--profile-id", &profile],
    )
    .await;
    assert_eq!(
        cli(root.path(), &["list-delivery-profiles"]).await["data"],
        json!([])
    );
    broker.shutdown.cancel();
    daemon.await.unwrap().unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn actual_cli_http_completion_and_audit_failure_remain_reconcilable() {
    for poison in [false, true] {
        let (root, broker, daemon, reference) = setup().await;
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/", listener.local_addr().unwrap());
        let profile=register(root.path(),json!({"label":"CLI HTTP","destination":{"kind":"http","config":{
            "url":url,"method":"POST","headers":[{"name":"Authorization","value":value(&reference)}],"query":[],"body":null,"timeout_secs":2}}})).await;
        let audit = root
            .path()
            .join("vault")
            .join(magicvault_core::store::SECRET_AUDIT_FILENAME);
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut bytes = Vec::new();
            let mut chunk = [0; 4096];
            while !bytes.windows(4).any(|w| w == b"\r\n\r\n") {
                let n = stream.read(&mut chunk).await.unwrap();
                assert!(n > 0);
                bytes.extend_from_slice(&chunk[..n]);
                assert!(bytes.len() < 16384);
            }
            assert!(String::from_utf8_lossy(&bytes).contains(CANARY));
            if poison {
                fs::rename(&audit, audit.with_extension("saved")).unwrap();
                fs::create_dir(&audit).unwrap();
            }
            stream.write_all(format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{CANARY}",CANARY.len()).as_bytes()).await.unwrap();
        });
        let operation = Uuid::new_v4().to_string();
        cli(
            root.path(),
            &[
                "secure-new-http",
                "--profile-id",
                &profile,
                "--operation-id",
                &operation,
            ],
        )
        .await;
        let status = settled(root.path(), &operation).await;
        server.await.unwrap();
        assert_eq!(
            status["data"]["state"],
            if poison { "uncertain" } else { "completed" }
        );
        assert_eq!(status["data"]["may_have_run"], true);
        if poison {
            assert_eq!(status["data"]["error"], "persistence_uncertain");
            assert_eq!(cli(root.path(), &["status"]).await["data"]["ready"], false);
        }
        broker.shutdown.cancel();
        daemon.await.unwrap().unwrap();
    }
}

// Includes fresh CLI startup, IPC, synthetic consent, delivery and 10ms status
// polling. No wall-clock pass threshold: this is a bounded local observation,
// not an Internet/TLS, native-dialog, saturation or production benchmark.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "explicit repeated real CLI/process/loopback HTTP latency qualification"]
async fn repeated_cli_delivery_latency_and_withheld_output() {
    for http in [false, true] {
        let (root, broker, daemon, reference) = setup().await;
        let _stop = broker.shutdown.clone().drop_guard();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/", listener.local_addr().unwrap());
        let executable = root.path().join("recipient");
        fs::write(&executable, "#!/bin/sh\n[ -n \"$MV_TOKEN\" ] || exit 1\nprintf '%s' \"$MV_TOKEN\"\nprintf '%s' \"$MV_TOKEN\" >&2\nprintf x >> marker\n").unwrap();
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
        let destination = if http {
            json!({"kind":"http","config":{"url":url,"method":"POST","headers":[{"name":"Authorization","value":value(&reference)}],"query":[],"body":null,"timeout_secs":2}})
        } else {
            json!({"kind":"process","config":{"executable":executable,"arguments":[],"working_directory":root.path(),"environment":[{"name":"MV_TOKEN","value":value(&reference)}],"stdin":null,"timeout_secs":2}})
        };
        let profile = register(
            root.path(),
            json!({"label":"Synthetic latency", "destination":destination}),
        )
        .await;
        let mut receivers = tokio::task::JoinSet::new();
        if http {
            receivers.spawn(async move {
                for _ in 0..20 {
                    tokio::time::timeout(Duration::from_secs(6), async {
                        let (mut stream, _) = listener.accept().await.unwrap();
                        let mut header = Vec::new();
                        while !header.ends_with(b"\r\n\r\n") {
                            assert!(header.len() < 8192);
                            header.push(stream.read_u8().await.unwrap());
                        }
                        assert!(String::from_utf8_lossy(&header).contains(CANARY));
                        stream.write_all(format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{CANARY}", CANARY.len()).as_bytes()).await.unwrap();
                    }).await.unwrap();
                }
            });
        }
        let mut samples = Vec::new();
        for _ in 0..20 {
            let operation = Uuid::new_v4().to_string();
            let started = std::time::Instant::now();
            cli(
                root.path(),
                &[
                    if http {
                        "secure-new-http"
                    } else {
                        "secure-new-process"
                    },
                    "--profile-id",
                    &profile,
                    "--operation-id",
                    &operation,
                ],
            )
            .await;
            assert_eq!(
                settled(root.path(), &operation).await["data"]["state"],
                "completed"
            );
            samples.push(started.elapsed().as_micros());
        }
        if http {
            receivers.join_next().await.unwrap().unwrap();
        } else {
            assert_eq!(
                fs::read(root.path().join("marker")).unwrap(),
                vec![b'x'; 20]
            );
        }
        let audit = fs::read_to_string(
            root.path()
                .join("vault")
                .join(magicvault_core::store::SECRET_AUDIT_FILENAME),
        )
        .unwrap();
        assert!(!audit.contains(CANARY));
        samples.sort_unstable();
        println!(
            "cli_delivery_measurement {}",
            json!({"kind":if http {"http"} else {"process"},"synthetic_consent":true,"samples":samples.len(),"us":{"min":samples[0],"median":(samples[9]+samples[10])/2,"p95":samples[18],"max":samples[19]}})
        );
        broker.shutdown.cancel();
        daemon.await.unwrap().unwrap();
    }
}
