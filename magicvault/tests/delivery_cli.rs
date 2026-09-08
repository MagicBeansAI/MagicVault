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
#[path = "support/measurements.rs"]
mod measurements;

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
    // Fixture-owned, value-free stage markers distinguish interpreter startup
    // from environment delivery. Never copy recipient output into diagnostics.
    fs::write(&executable,"#!/bin/sh\nprintf 'entered' > entered\n[ -n \"$MV_TOKEN\" ] || exit 1\nprintf 'present' > material-present\nprintf '%s' \"$MV_TOKEN\"\nprintf 'done' > marker\n").unwrap();
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
    let profile=register(root.path(),json!({"label":"CLI process","destination":{"kind":"process","config":{
        "executable":executable,"arguments":[],"working_directory":root.path(),"environment":[{"name":"MV_TOKEN","value":value(&reference)}],"stdin":null,"timeout_secs":2}}})).await;
    let operation = Uuid::new_v4().to_string();
    #[cfg(magicvault_test_diagnostics)]
    let observation =
        magicvault_effect::test_diagnostics::Capture::register(Uuid::parse_str(&operation).unwrap())
            .unwrap();
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
    #[cfg(magicvault_test_diagnostics)]
    {
        let snapshot = observation.snapshot();
        eprintln!("process_fixture_observation {snapshot:?}");
        if status["data"]["state"] == "completed" {
            assert!(
                snapshot.runtime_entered && snapshot.adapter_returned,
                "exact process path did not publish its diagnostic stages: {snapshot:?}"
            );
            let child = snapshot.process.expect("exact path must capture the owned process");
            assert_eq!(child.spawned_children, 1);
            #[cfg(target_os = "macos")]
            assert_eq!(
                child.spawn_method,
                Some(magicvault_effect::test_diagnostics::SpawnMethod::MacosPosixSpawn)
            );
            #[cfg(target_os = "macos")]
            assert_eq!(child.pre_exec_stage, None);
            assert_eq!(child.reaped_normal_success, Some(true));
        }
    }
    assert_eq!(
        status["data"]["state"],
        "completed",
        "closed error: {}; may_have_run: {}; recipient entered: {}; material present: {}; recipient marker exists: {}",
        status["data"]["error"],
        status["data"]["may_have_run"],
        root.path().join("entered").exists(),
        root.path().join("material-present").exists(),
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
    measure_deliveries(20, false).await;
}

async fn measure_deliveries(count: usize, capacity_probe: bool) {
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
                for _ in 0..count {
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
        let before = measurements::snapshot();
        let mut max_resident_kib = before["resident_kib"].as_u64().unwrap_or(0);
        let mut first_operation = None;
        for _ in 0..count {
            let operation = Uuid::new_v4().to_string();
            first_operation.get_or_insert_with(|| operation.clone());
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
            let status = settled(root.path(), &operation).await;
            assert_eq!(
                status["data"]["state"],
                "completed",
                "closed error: {}; may_have_run: {}; recipient marker bytes: {:?}",
                status["data"]["error"],
                status["data"]["may_have_run"],
                fs::metadata(root.path().join("marker"))
                    .map(|m| m.len())
                    .ok()
            );
            samples.push(started.elapsed().as_micros());
            max_resident_kib = max_resident_kib.max(
                measurements::snapshot()["resident_kib"]
                    .as_u64()
                    .unwrap_or(0),
            );
        }
        if capacity_probe {
            use magicvault_service::client::Client;
            let client = Client::load(root.path().to_owned(), "default").unwrap();
            let request = SecureDelivery {
                profile_id: profile.parse().unwrap(),
                operation_id: Uuid::new_v4(),
            };
            let request = if http {
                Request::SecureNewHttp(request)
            } else {
                Request::SecureNewProcess(request)
            };
            assert!(matches!(
                client.call(request).await,
                Err(ErrorCode::Capacity)
            ));
            // An identical ID remains reconcilable while full. This queries an
            // already completed operation, not a retry of an uncertain effect.
            let replay = cli(
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
                    first_operation.as_ref().unwrap(),
                ],
            )
            .await;
            assert_eq!(replay["data"]["state"], "completed");
        }
        if http {
            receivers.join_next().await.unwrap().unwrap();
        } else {
            assert_eq!(
                fs::read(root.path().join("marker")).unwrap(),
                vec![b'x'; count]
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
            json!({"kind":if http {"http"} else {"process"},"synthetic_consent":true,"samples":samples.len(),
                "capacity_refused_without_dispatch":capacity_probe,
                "resources_before":before,"resources_after":measurements::snapshot(),"sampled_max_resident_kib":max_resident_kib,
                "us":{"min":samples[0],"median":(samples[(count-1)/2]+samples[count/2])/2,"p95":samples[(count*95).div_ceil(100)-1],"max":samples[count-1]}})
        );
        broker.shutdown.cancel();
        daemon.await.unwrap().unwrap();
    }
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "explicit bounded installed CLI load, IPC/job saturation, CPU/RSS and shutdown qualification"]
async fn bounded_cli_delivery_capacity_and_service_resources() {
    use magicvault_service::client::Client;
    use std::time::Instant;
    measure_deliveries(MAX_DELIVERY_JOBS, true).await;
    let (root, broker, daemon, _) = setup().await;
    let _stop = broker.shutdown.clone().drop_guard();
    let before_idle = measurements::snapshot();
    tokio::time::sleep(Duration::from_secs(2)).await;
    let after_idle = measurements::snapshot();
    let started = Instant::now();
    let mut workers = tokio::task::JoinSet::new();
    for _ in 0..4 {
        let client = Client::load(root.path().to_owned(), "default").unwrap();
        workers.spawn(async move {
            for _ in 0..500 {
                assert!(client.status().await.unwrap().ready);
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        });
    }
    tokio::time::timeout(Duration::from_secs(30), async {
        while let Some(result) = workers.join_next().await {
            result.unwrap();
        }
    })
    .await
    .unwrap();
    let load_us = started.elapsed().as_micros();
    let after_load = measurements::snapshot();
    // Sixteen incomplete frames occupy the documented shared connection cap.
    // No capability or credential bytes are needed for this admission probe.
    let mut stalled = Vec::new();
    for _ in 0..16 {
        let mut socket = tokio::net::UnixStream::connect(broker.socket())
            .await
            .unwrap();
        socket.write_u32_le(1).await.unwrap();
        stalled.push(socket);
    }
    let mut excess = tokio::net::UnixStream::connect(broker.socket())
        .await
        .unwrap();
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(2), excess.read(&mut [0u8; 1]))
            .await
            .unwrap()
            .unwrap(),
        0
    );
    drop(excess);
    drop(stalled);
    let client = Client::load(root.path().to_owned(), "default").unwrap();
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if client.status().await.is_ok_and(|s| s.ready) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    // An incomplete frame must not leave the service alive indefinitely on stop.
    let mut pending = tokio::net::UnixStream::connect(broker.socket())
        .await
        .unwrap();
    pending.write_u32_le(1).await.unwrap();
    tokio::time::sleep(Duration::from_millis(30)).await;
    let shutdown = Instant::now();
    broker.shutdown.cancel();
    tokio::time::timeout(Duration::from_secs(12), daemon)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(!broker.socket().exists());
    assert!(!root.path().join("bridge.sock").exists());
    println!(
        "service_resource_measurement {}",
        json!({
        "scope":"synthetic in-process broker plus test driver; not standalone daemon or browser",
        "status_requests":2000,"concurrency":4,"load_us":load_us,"idle_window_ms":2000,
        "before_idle":before_idle,"after_idle":after_idle,"after_load":after_load,
        "ipc_capacity":16,"capacity_recovered":true,"shutdown_us":shutdown.elapsed().as_micros(),
        "after_shutdown":measurements::snapshot()})
    );
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "explicit real child-tree and streaming HTTP service-shutdown qualification"]
async fn shutdown_drains_inflight_process_tree_and_http_without_replay() {
    for http in [false, true] {
        let (root, broker, daemon, reference) = setup().await;
        let _stop = broker.shutdown.clone().drop_guard();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/", listener.local_addr().unwrap());
        let executable = root.path().join("recipient");
        fs::write(&executable, "#!/bin/sh\n[ -n \"$MV_TOKEN\" ] || exit 1\n(while :; do printf x >> heartbeat; /bin/sleep 0.02; done) &\nwait\nprintf late > late-marker\n").unwrap();
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
        let destination = if http {
            json!({"kind":"http","config":{"url":url,"method":"POST","headers":[{"name":"Authorization","value":value(&reference)}],"query":[],"body":null,"timeout_secs":30}})
        } else {
            json!({"kind":"process","config":{"executable":executable,"arguments":[],"working_directory":root.path(),"environment":[{"name":"MV_TOKEN","value":value(&reference)}],"stdin":null,"timeout_secs":30}})
        };
        let profile = register(
            root.path(),
            json!({"label":"Synthetic in-flight shutdown","destination":destination}),
        )
        .await;
        let (entered, ready) = tokio::sync::oneshot::channel();
        let mut receiver = tokio::task::JoinSet::new();
        if http {
            receiver.spawn(async move {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut header = Vec::new();
                while !header.ends_with(b"\r\n\r\n") {
                    assert!(header.len() < 8192);
                    header.push(stream.read_u8().await.unwrap());
                }
                assert!(String::from_utf8_lossy(&header).contains(CANARY));
                // Deliberately unfinished body keeps the actual effect in flight.
                stream
                    .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 1024\r\n\r\nx")
                    .await
                    .unwrap();
                let _ = entered.send(());
                let result =
                    tokio::time::timeout(Duration::from_secs(5), stream.read(&mut [0u8; 1]))
                        .await
                        .unwrap();
                assert!(
                    matches!(result, Ok(0))
                        || result.is_err_and(|e| e.kind() == std::io::ErrorKind::ConnectionReset)
                );
                assert!(
                    tokio::time::timeout(Duration::from_millis(250), listener.accept())
                        .await
                        .is_err(),
                    "shutdown must not replay a request"
                );
            });
        }
        let operation = Uuid::new_v4().to_string();
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
        if http {
            tokio::time::timeout(Duration::from_secs(3), ready)
                .await
                .unwrap()
                .unwrap();
        } else {
            tokio::time::timeout(Duration::from_secs(3), async {
                while fs::metadata(root.path().join("heartbeat")).map_or(true, |m| m.len() == 0) {
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            })
            .await
            .unwrap();
        }
        assert_eq!(
            cli(
                root.path(),
                &["delivery-status", "--operation-id", &operation]
            )
            .await["data"]["state"],
            "running"
        );
        let started = std::time::Instant::now();
        broker.shutdown.cancel();
        tokio::time::timeout(Duration::from_secs(5), daemon)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        let shutdown_us = started.elapsed().as_micros();
        if http {
            receiver.join_next().await.unwrap().unwrap();
        } else {
            let count = fs::metadata(root.path().join("heartbeat")).unwrap().len();
            tokio::time::sleep(Duration::from_millis(250)).await;
            assert_eq!(
                fs::metadata(root.path().join("heartbeat")).unwrap().len(),
                count
            );
            assert!(!root.path().join("late-marker").exists());
        }
        assert!(!broker.socket().exists());
        assert!(!root.path().join("bridge.sock").exists());
        println!(
            "inflight_shutdown_measurement {}",
            json!({"kind":if http {"http"} else {"process_tree"},"shutdown_us":shutdown_us,"recipient_stopped":true,"replayed":false})
        );
    }
}
