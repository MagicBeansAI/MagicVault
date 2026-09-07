//! Opt-in real Chrome -> shipped native host -> daemon -> shipped MCP transport.
//! Synthetic custody/consent only. Never installs an OS-wide host or modifies a
//! personal browser. A fixture manifest pregrants only the loopback test site;
//! this does NOT qualify Chrome's permission dialog or native human/keychain UI.
#![cfg(target_os = "macos")]
use async_trait::async_trait;
use magicvault_core::{store::SecretStore, InMemoryKeyProvider};
use magicvault_service::{
    broker::Broker, client::Client, human::HumanInteraction, ipc, native, protocol::*, storage,
};
use magicvault_test_support::browser::{CdpPeer, DisposableBrowser, BROWSER_CANARY};
use rmcp::{
    model::{CallToolRequestParams, CallToolResponse},
    ServiceExt,
};
use serde_json::{json, Value};
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;
use zeroize::Zeroizing;

#[derive(Default)]
struct SyntheticHuman {
    deny: AtomicBool,
    confirmations: AtomicUsize,
    hold: AtomicBool,
    entered: tokio::sync::Notify,
    release: tokio::sync::Notify,
}
#[async_trait]
impl HumanInteraction for SyntheticHuman {
    async fn confirm(&self, message: &str, cancel: CancellationToken) -> Result<bool, ErrorCode> {
        assert!(!message.contains(BROWSER_CANARY));
        self.confirmations.fetch_add(1, Ordering::SeqCst);
        if self.hold.load(Ordering::SeqCst) {
            self.entered.notify_one();
            tokio::select! {
                _ = cancel.cancelled() => return Err(ErrorCode::Cancelled),
                _ = self.release.notified() => {},
            }
        }
        Ok(!self.deny.load(Ordering::SeqCst))
    }
    async fn secret(&self, _: &str, _: CancellationToken) -> Result<Zeroizing<String>, ErrorCode> {
        Ok(Zeroizing::new(BROWSER_CANARY.into()))
    }
}
struct McpClient;
impl rmcp::ClientHandler for McpClient {}

fn private_file(path: &Path, bytes: &[u8], mode: u32) {
    fs::write(path, bytes).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(mode)).unwrap();
}

fn extension_assets(parent: &Path) -> PathBuf {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    // Test-only override qualifies the installed bundle's actual assets. The
    // one manifest change below remains explicit in either mode.
    let installed = std::env::var_os("MAGICVAULT_TEST_EXTENSION").map(PathBuf::from);
    if let Some(path) = &installed {
        assert!(path.is_absolute() && path.is_dir());
    }
    let source = installed.clone().unwrap_or_else(|| repo.join("extension"));
    let directory = parent.join("extension");
    fs::create_dir(&directory).unwrap();
    for name in ["worker.js", "options.js", "options.html", "options.css"] {
        fs::copy(source.join(name), directory.join(name)).unwrap();
    }
    fs::copy(
        if installed.is_some() {
            source.join("fill.js")
        } else {
            repo.join("magicvault-effect/src/fill.js")
        },
        directory.join("fill.js"),
    )
    .unwrap();
    let mut manifest: Value =
        serde_json::from_slice(&fs::read(source.join("manifest.json")).unwrap()).unwrap();
    // Pregranted fixture origin only, not a production permission-grant shortcut.
    manifest["host_permissions"] = json!(["http://127.0.0.1/*"]);
    private_file(
        &directory.join("manifest.json"),
        &serde_json::to_vec(&manifest).unwrap(),
        0o600,
    );
    directory
}

async fn options_eval(peer: &mut CdpPeer, session: &str, expression: &str) -> Value {
    let result = peer
        .command(
            "Runtime.evaluate",
            json!({"expression":expression,"awaitPromise":true,"returnByValue":true}),
            Some(session),
        )
        .await;
    assert!(
        result.get("exceptionDetails").is_none(),
        "extension options fixture failed"
    );
    result["result"]["value"].clone()
}
async fn connection(
    peer: &mut CdpPeer,
    session: &str,
    connected: bool,
    startup: Option<(&Path, &SyntheticHuman)>,
) -> Value {
    let mut diagnostic = json!({});
    let result = tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            let state = options_eval(
                peer,
                session,
                "chrome.runtime.sendMessage({action:'status'})",
            )
            .await;
            diagnostic = json!({"connected":state["connected"].as_bool(),
                "connecting":state["connecting"].as_bool(),"paused":state["paused"].as_bool(),
                "reason":state["reason"].as_str().filter(|s| s.len() <= 32 && s.bytes().all(|b| b.is_ascii_lowercase() || b == b'_'))});
            if state["connected"] == connected {
                return state;
            }
            tokio::time::sleep(Duration::from_millis(30)).await;
        }
    })
    .await;
    result.unwrap_or_else(|_| {
        // Fixture-owned counts only: never log a native frame, capability,
        // browser state dump, or command arguments while diagnosing startup.
        let stages = startup.map(|(marker, human)| {
            json!({"wrapper_launches":fs::metadata(marker).map(|m|m.len()).unwrap_or(0),
                "synthetic_confirmations":human.confirmations.load(Ordering::SeqCst)})
        });
        panic!("extension connection did not settle (expected {connected}): {diagnostic}; fixture stages: {stages:?}")
    })
}
async fn load(
    owner: &DisposableBrowser,
    assets: &Path,
    marker: &Path,
    human: &SyntheticHuman,
) -> (CdpPeer, String, String) {
    let started = Instant::now();
    let mut peer = owner.peer().await;
    let result = peer
        .command("Extensions.loadUnpacked", json!({"path":assets}), None)
        .await;
    assert_eq!(result["id"], native::EXTENSION_ID);
    let (_, options) = peer
        .open_page(&format!(
            "chrome-extension://{}/options.html",
            native::EXTENSION_ID
        ))
        .await;
    let (_, page) = peer.open_page(&format!("{}/login", owner.origin)).await;
    connection(&mut peer, &options, true, Some((marker, human))).await;
    eprintln!(
        "native extension startup settled in {:?}",
        started.elapsed()
    );
    (peer, options, page)
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "explicit real Chrome/native-host/MCP qualification; synthetic consent and loopback permission fixture"]
async fn real_extension_mcp_fill_denial_profiles_pause_and_reconnect() {
    let root = tempfile::Builder::new()
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir()
        .unwrap();
    let instance = storage::Instance {
        format_version: 1,
        id: Uuid::new_v4(),
    };
    private_file(
        &root.path().join("instance.json"),
        &serde_json::to_vec(&instance).unwrap(),
        0o600,
    );
    let human = Arc::new(SyntheticHuman::default());
    let broker = Broker::with_components(
        storage::open(root.path()).unwrap(),
        SecretStore::new_empty(
            Box::new(InMemoryKeyProvider::new()),
            root.path().join("vault"),
        ),
        human.clone(),
    )
    .unwrap();
    let _stop = broker.shutdown.clone().drop_guard();
    let daemon = tokio::spawn(ipc::serve(broker.clone()));
    tokio::time::timeout(Duration::from_secs(5), async {
        while !root.path().join("bridge.sock").exists() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    Client::pair(
        root.path().to_owned(),
        "qa",
        "Isolated native transport".into(),
    )
    .await
    .unwrap();
    let client = Client::load(root.path().to_owned(), "qa").unwrap();
    let Response::Enrolled(credential) = client
        .call(Request::Enroll(EnrollRequest {
            label: "Synthetic fixture".into(),
            field_names: vec!["password".into()],
        }))
        .await
        .unwrap()
    else {
        panic!("enrollment");
    };
    let executable = PathBuf::from(
        std::env::var_os("MAGICVAULT_TEST_NATIVE_HOST")
            .expect("explicit shipped native host path required"),
    );
    assert!(executable.is_absolute() && executable.is_file());
    let config = native::HostConfig {
        version: 1,
        root: root.path().to_owned(),
        instance_id: instance.id,
        profile: "qa".into(),
        extension_id: native::EXTENSION_ID.into(),
        executable,
    };
    private_file(
        &native::config_path(root.path()),
        &serde_json::to_vec(&config).unwrap(),
        0o600,
    );
    let marker = root.path().join("host-starts");
    let quoted_marker = marker.to_str().unwrap().replace('\'', "'\\''");
    // Count entry into the fixture wrapper before exec, independently of the
    // synthetic daemon. This never intercepts or records native protocol bytes.
    let wrapper = native::wrapper(&config).unwrap().replacen(
        "#!/bin/sh\n",
        &format!("#!/bin/sh\nprintf . >> '{quoted_marker}'\n"),
        1,
    );
    private_file(
        &root.path().join("native-host-launch"),
        wrapper.as_bytes(),
        0o700,
    );
    let assets = extension_assets(root.path());
    let manifest = native::manifest(&config).unwrap();
    let first = DisposableBrowser::with_native_host(true, &manifest).await;
    let (mut a, options_a, page_a) = load(&first, &assets, &marker, &human).await;
    let second = DisposableBrowser::with_native_host(true, &manifest).await;
    let (mut b, options_b, _) = load(&second, &assets, &marker, &human).await;
    let info_a = connection(&mut a, &options_a, true, None).await;
    let info_b = connection(&mut b, &options_b, true, None).await;
    assert_ne!(info_a["profile_id"], info_b["profile_id"]);
    assert_ne!(info_a["browser_handle"], info_b["browser_handle"]);
    client
        .call(Request::ConfigureBrowserCredential(BrowserRule {
            credential_ref: credential.credential_ref.clone(),
            origins: vec![first.origin.clone()],
            field_names: vec!["password".into()],
        }))
        .await
        .unwrap();

    let mcp = std::env::var_os("MAGICVAULT_TEST_MCP")
        .unwrap_or_else(|| env!("CARGO_BIN_EXE_magicvault-mcp").into());
    let mut child = tokio::process::Command::new(mcp)
        .arg("--root")
        .arg(root.path())
        .args(["--profile", "qa"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let peer = McpClient
        .serve((child.stdout.take().unwrap(), child.stdin.take().unwrap()))
        .await
        .unwrap();
    assert_eq!(peer.list_tools(None).await.unwrap().tools.len(), 14);
    let invoke = |name: &'static str, args: Value| {
        let peer = &peer;
        async move {
            let reply = tokio::time::timeout(
                Duration::from_secs(10),
                peer.call_tool_once(
                    CallToolRequestParams::new(name)
                        .with_arguments(args.as_object().unwrap().clone()),
                ),
            )
            .await
            .unwrap()
            .unwrap();
            let CallToolResponse::Complete(result) = reply else {
                panic!("complete MCP result");
            };
            assert_ne!(result.is_error, Some(true), "MCP fixture operation failed");
            let wire = serde_json::to_string(&result).unwrap();
            assert!(!wire.contains(BROWSER_CANARY));
            assert_eq!(result.content.len(), 1);
            serde_json::from_str::<Response>(
                &result.content[0].as_text().expect("JSON text receipt").text,
            )
            .unwrap()
        }
    };
    let Response::Browsers(rows) = invoke("list_browsers", json!({})).await else {
        panic!("browsers");
    };
    assert_eq!(rows.len(), 2);
    let handle: Uuid = serde_json::from_value(info_a["browser_handle"].clone()).unwrap();
    // A real profile with more than 64 unrelated tabs must still discover and
    // fill its permitted login. These empty background tabs have no site grant;
    // only this disposable browser owns them, and its Drop reaps them all.
    for _ in 0..65 {
        a.command(
            "Target.createTarget",
            json!({"url":"about:blank", "background":true}),
            None,
        )
        .await;
    }
    let mut samples = Vec::new();
    let mut denial_us = 0;
    // Below the broker's 32 retained-operation bound. These are small-sample
    // observations including status polling, not throughput or native-UI claims.
    for iteration in 0..21 {
        let deny = iteration == 1;
        human.deny.store(deny, Ordering::SeqCst);
        assert!(
            a.evaluate_bool(
                &page_a,
                "(() => { document.querySelector('#password').value = ''; return true; })()"
            )
            .await
        );
        let Response::BrowserTargets(targets) = invoke(
            "browser_targets",
            if iteration % 2 == 0 {
                json!({"browser_handle":handle,"top_origin":first.origin})
            } else {
                json!({"browser_handle":handle})
            },
        )
        .await
        else {
            panic!("targets");
        };
        let mut target = targets
            .iter()
            .find(|t| t.is_main_frame && t.origin == first.origin)
            .expect("permitted fixture target")
            .clone();
        if iteration == 0 {
            let Response::BrowserTargets(narrowed) = invoke(
                "browser_targets",
                json!({"browser_handle":handle,"top_origin":first.origin,"tab_id":target.tab_id}),
            )
            .await
            else {
                panic!("filtered targets");
            };
            assert!(narrowed
                .iter()
                .all(|t| t.tab_id == target.tab_id && t.top_origin == first.origin));
            // Rediscovery replaces handles: select only the fresh result below.
            target = narrowed.into_iter().find(|t| t.is_main_frame).unwrap();
        }
        let operation = Uuid::new_v4();
        let started = Instant::now();
        invoke("secure_fill", json!({"operation_id":operation,"browser_handle":handle,"target_handle":target.target_handle,"fields":[{"css":"#password","credential_ref":credential.credential_ref,"credential_field":"password"}]})).await;
        let status = tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let Response::Fill(status) =
                    invoke("fill_status", json!({"operation_id":operation})).await
                else {
                    panic!("fill status");
                };
                if !matches!(status.state, FillState::Pending | FillState::Filling) {
                    break status;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        assert_eq!(
            status.state,
            if deny {
                FillState::Denied
            } else {
                FillState::Filled
            }
        );
        if deny {
            denial_us = started.elapsed().as_micros();
        } else {
            samples.push(started.elapsed().as_micros());
        }
        assert!(a.evaluate_bool(&page_a, if deny {"document.querySelector('#password').value === ''"} else {"document.querySelector('#password').value === 'SYNTHETIC-CHROMIUM-FILL-CANARY' && document.body.dataset.reactive === 'ok' && document.body.dataset.submitted !== 'yes'"}).await);
    }
    human.deny.store(false, Ordering::SeqCst);
    // Navigate or block the site only after the synthetic consent gate is
    // waiting. A previously discovered handle must never authorize replacement
    // documents or bypass a newly installed human site block.
    for navigate in [true, false] {
        let Response::BrowserTargets(targets) =
            invoke("browser_targets", json!({"browser_handle":handle})).await
        else {
            panic!("targets");
        };
        let target = targets
            .iter()
            .find(|t| t.is_main_frame && t.origin == first.origin)
            .unwrap();
        assert!(
            a.evaluate_bool(
                &page_a,
                "(() => { document.querySelector('#password').value = ''; return true; })()"
            )
            .await
        );
        human.hold.store(true, Ordering::SeqCst);
        let operation = Uuid::new_v4();
        invoke("secure_fill", json!({"operation_id":operation,"browser_handle":handle,"target_handle":target.target_handle,"fields":[{"css":"#password","credential_ref":credential.credential_ref,"credential_field":"password"}]})).await;
        tokio::time::timeout(Duration::from_secs(5), human.entered.notified())
            .await
            .unwrap();
        if navigate {
            a.navigate(&page_a, &format!("{}/next", first.origin)).await;
        } else {
            let result = options_eval(
                &mut a,
                &options_a,
                &format!(
                    "chrome.runtime.sendMessage({{action:'site-block',site:{},blocked:true}})",
                    json!(first.origin)
                ),
            )
            .await;
            assert_eq!(result["ok"], true);
        }
        human.hold.store(false, Ordering::SeqCst);
        human.release.notify_one();
        let status = tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let Response::Fill(status) =
                    invoke("fill_status", json!({"operation_id":operation})).await
                else {
                    panic!("fill status");
                };
                if !matches!(status.state, FillState::Pending | FillState::Filling) {
                    break status;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        assert_eq!(
            status.state,
            if navigate {
                FillState::Failed
            } else {
                FillState::Denied
            }
        );
        assert_eq!(
            status.error,
            Some(if navigate {
                ErrorCode::StaleTarget
            } else {
                ErrorCode::PermissionDenied
            })
        );
        assert!(
            a.evaluate_bool(&page_a, "document.querySelector('#password').value === ''")
                .await
        );
        if !navigate {
            let Response::BrowserTargets(targets) =
                invoke("browser_targets", json!({"browser_handle":handle})).await
            else {
                panic!("targets");
            };
            assert!(!targets.iter().any(|t| t.origin == first.origin));
            let result = options_eval(
                &mut a,
                &options_a,
                &format!(
                    "chrome.runtime.sendMessage({{action:'site-block',site:{},blocked:false}})",
                    json!(first.origin)
                ),
            )
            .await;
            assert_eq!(result["ok"], true);
        }
        a.navigate(&page_a, &format!("{}/login", first.origin))
            .await;
    }
    let count = human.confirmations.load(Ordering::SeqCst);
    options_eval(
        &mut a,
        &options_a,
        "chrome.runtime.sendMessage({action:'disconnect'})",
    )
    .await;
    assert_eq!(
        connection(&mut a, &options_a, false, None).await["paused"],
        true
    );
    let Response::Browsers(rows) = invoke("list_browsers", json!({})).await else {
        panic!("browsers");
    };
    assert_eq!(rows.len(), 1);
    assert_eq!(json!(rows[0].browser_handle), info_b["browser_handle"]);
    options_eval(
        &mut a,
        &options_a,
        "chrome.runtime.sendMessage({action:'connect'})",
    )
    .await;
    let reconnected = connection(&mut a, &options_a, true, None).await;
    assert_ne!(reconnected["browser_handle"], info_a["browser_handle"]);
    assert_eq!(reconnected["profile_id"], info_a["profile_id"]);
    assert_eq!(human.confirmations.load(Ordering::SeqCst), count);
    assert_eq!(
        connection(&mut b, &options_b, true, None).await["browser_handle"],
        info_b["browser_handle"]
    );
    let audit = fs::read_to_string(
        root.path()
            .join("vault")
            .join(magicvault_core::store::SECRET_AUDIT_FILENAME),
    )
    .unwrap();
    assert!(!audit.contains(BROWSER_CANARY));
    samples.sort_unstable();
    println!(
        "extension_transport_measurement {}",
        json!({"synthetic_consent":true,"samples":samples.len(),"fill_us":{"min":samples[0],"median":(samples[9]+samples[10])/2,"p95":samples[18],"max":samples[19]},"denial_us":denial_us,"profiles":2})
    );
    peer.cancel().await.unwrap();
    tokio::time::timeout(Duration::from_secs(3), child.wait())
        .await
        .unwrap()
        .unwrap();
    drop(first);
    drop(second);
    broker.shutdown.cancel();
    daemon.await.unwrap().unwrap();
}
