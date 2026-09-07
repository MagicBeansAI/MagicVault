//! Shipped CLI -> real daemon/IPC -> real Chrome. Only custody key and human
//! prompts are synthetic test components; no production auto-approval mode.
#![cfg(unix)]
use async_trait::async_trait;
use magicvault_core::{store::SecretStore, InMemoryKeyProvider};
use magicvault_service::{broker::Broker, human::HumanInteraction, ipc, protocol::*, storage};
use magicvault_test_support::browser::{DisposableBrowser, BROWSER_CANARY};
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;
use zeroize::Zeroizing;

struct SyntheticHuman {
    deny: AtomicBool,
}
#[async_trait]
impl HumanInteraction for SyntheticHuman {
    async fn confirm(&self, message: &str, _: CancellationToken) -> Result<bool, ErrorCode> {
        assert!(!message.contains(BROWSER_CANARY));
        Ok(!(message.starts_with("Allow ONE") && self.deny.load(Ordering::SeqCst)))
    }
    async fn secret(&self, _: &str, _: CancellationToken) -> Result<Zeroizing<String>, ErrorCode> {
        Ok(Zeroizing::new(BROWSER_CANARY.into()))
    }
}
async fn cli(root: &Path, args: &[&str]) -> serde_json::Value {
    let output = tokio::time::timeout(
        Duration::from_secs(15),
        tokio::process::Command::new(env!("CARGO_BIN_EXE_magicvault"))
            .arg("--root")
            .arg(root)
            .args(args)
            .kill_on_drop(true)
            .output(),
    )
    .await
    .expect("test CLI timed out")
    .unwrap();
    assert!(
        output.status.success(),
        "test CLI failed with closed diagnostic"
    );
    assert!(output.stderr.is_empty());
    assert!(!String::from_utf8_lossy(&output.stdout).contains(BROWSER_CANARY));
    serde_json::from_slice(&output.stdout).expect("test CLI must return valid JSON")
}
async fn response(root: &Path, args: &[&str]) -> Response {
    serde_json::from_value(cli(root, args).await).unwrap()
}
async fn settled(root: &Path, operation: Uuid) -> FillStatus {
    tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            let Response::Fill(status) = response(
                root,
                &["fill-status", "--operation-id", &operation.to_string()],
            )
            .await
            else {
                panic!("status");
            };
            if !matches!(status.state, FillState::Pending | FillState::Filling) {
                return status;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .unwrap()
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "explicit disposable real-browser CLI qualification; no live keychain/native prompts"]
async fn shipped_cli_pairs_enrolls_configures_and_fills_real_chrome_without_value_output() {
    let owner = DisposableBrowser::start(true).await;
    let mut peer = owner.peer().await;
    let (tab, session) = peer.open_page(&format!("{}/login", owner.origin)).await;
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
    let human = Arc::new(SyntheticHuman {
        deny: AtomicBool::new(false),
    });
    let broker = Broker::with_components(lease, store, human.clone()).unwrap();
    // Cancellation guard runs even when an assertion panics. The browser has
    // its own independent RAII owner that kills/reaps only its fresh child.
    let _stop = broker.shutdown.clone().drop_guard();
    let mut daemon = tokio::spawn(ipc::serve(Arc::clone(&broker)));
    tokio::select! {
        result = &mut daemon => panic!("test daemon exited before readiness: {result:?}"),
        _ = async { tokio::time::timeout(Duration::from_secs(5), async {
            while !broker.socket().exists() { tokio::time::sleep(Duration::from_millis(10)).await; }
        }).await.unwrap(); } => {},
    }
    cli(
        root.path(),
        &["pair", "--label", "Real browser CLI fixture"],
    )
    .await;
    let Response::Enrolled(credential) = response(
        root.path(),
        &["enroll", "--label", "Synthetic only", "--field", "password"],
    )
    .await
    else {
        panic!("enroll");
    };
    response(
        root.path(),
        &[
            "configure-browser-credential",
            "--credential-ref",
            &credential.credential_ref,
            "--field",
            "password",
            "--origin",
            &owner.origin,
        ],
    )
    .await;
    let Response::Browser(browser) = response(
        root.path(),
        &[
            "register-cdp",
            "--label",
            "Disposable Chrome",
            "--endpoint",
            &owner.endpoint,
        ],
    )
    .await
    else {
        panic!("browser");
    };
    let handle = browser.browser_handle.to_string();
    for deny in [false, true] {
        human.deny.store(deny, Ordering::SeqCst);
        assert!(
            peer.evaluate_bool(
                &session,
                "(() => { document.querySelector('#password').value = ''; return true; })()"
            )
            .await
        );
        let Response::BrowserTargets(targets) = response(
            root.path(),
            &[
                "browser-targets",
                "--browser-handle",
                &handle,
                "--top-origin",
                &owner.origin,
                "--tab-id",
                &tab,
            ],
        )
        .await
        else {
            panic!("targets");
        };
        let target = targets
            .into_iter()
            .find(|target| target.tab_id == tab && target.is_main_frame)
            .unwrap();
        let request = SecureFill {
            operation_id: Uuid::new_v4(),
            browser_handle: browser.browser_handle,
            target_handle: target.target_handle,
            fields: vec![FillField {
                css: "#password".into(),
                credential_ref: credential.credential_ref.clone(),
                credential_field: "password".into(),
            }],
        };
        let file = root.path().join("reference-only-fill.json");
        fs::write(&file, serde_json::to_vec(&request).unwrap()).unwrap();
        response(
            root.path(),
            &["secure-fill", "--request-file", file.to_str().unwrap()],
        )
        .await;
        let status = settled(root.path(), request.operation_id).await;
        assert_eq!(
            status.state,
            if deny {
                FillState::Denied
            } else {
                FillState::Filled
            }
        );
        assert!(peer.evaluate_bool(&session, if deny {"document.querySelector('#password').value === ''"} else {
            "document.querySelector('#password').value === 'SYNTHETIC-CHROMIUM-FILL-CANARY' && document.body.dataset.reactive === 'ok' && document.body.dataset.submitted !== 'yes'"
        }).await);
    }
    response(
        root.path(),
        &["disconnect-browser", "--browser-handle", &handle],
    )
    .await;
    assert!(
        peer.evaluate_bool(&session, "document.readyState === 'complete'")
            .await
    );
    let audit = fs::read_to_string(
        root.path()
            .join("vault")
            .join(magicvault_core::store::SECRET_AUDIT_FILENAME),
    )
    .unwrap();
    assert!(audit.contains("standalone_fill_completed"));
    assert!(!audit.contains(BROWSER_CANARY));
    broker.shutdown.cancel();
    daemon.await.unwrap().unwrap();
}
