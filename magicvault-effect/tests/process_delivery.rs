#![cfg(unix)]
use magicvault_effect::{delivery::DeliveryMaterial, process};
use magicvault_protocol::*;
use std::{fs, os::unix::fs::PermissionsExt, time::Duration};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;
use zeroize::Zeroizing;

const CANARY: &str = "SYNTHETIC-DELIVERY-DO-NOT-USE";
fn fixture(script: &str) -> (tempfile::TempDir, ProcessDestination, DeliveryMaterial) {
    let root = tempfile::tempdir().unwrap();
    let executable = root.path().join("fixture-command");
    fs::write(&executable, script).unwrap();
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
    let reference = format!("cred_{}", Uuid::new_v4());
    let value = InputValue::Credential {
        credential_ref: reference.clone(),
        credential_field: "token".into(),
        prefix: String::new(),
        suffix: String::new(),
    };
    let config = ProcessDestination {
        executable: executable.to_str().unwrap().into(),
        arguments: vec![],
        working_directory: root.path().to_str().unwrap().into(),
        environment: vec![NamedValue {
            name: "MV_TOKEN".into(),
            value: value.clone(),
        }],
        stdin: Some(value),
        timeout_secs: 3,
    };
    let mut material = DeliveryMaterial::default();
    material.insert(reference, "token".into(), Zeroizing::new(CANARY.into()));
    (root, config, material)
}

#[tokio::test(flavor = "multi_thread")]
async fn real_child_receives_env_and_stdin_while_output_and_diagnostics_are_withheld() {
    let script = format!("#!/bin/sh\n[ \"$MV_TOKEN\" = '{CANARY}' ] || exit 8\ninput=$(/bin/cat)\n[ \"$input\" = '{CANARY}' ] || exit 9\nprintf '%s' \"$MV_TOKEN\"\nprintf '%s' \"$MV_TOKEN\" >&2\n");
    let (_root, config, material) = fixture(&script);
    let digest = process::inspect(&config).unwrap();
    let outcome = process::execute(
        config,
        digest,
        material,
        Uuid::new_v4(),
        CancellationToken::new(),
    )
    .await;
    assert_eq!(
        outcome.state,
        DeliveryState::Completed,
        "closed error: {:?}",
        outcome.error
    );
    assert!(outcome.may_have_run);
    let status = DeliveryStatus {
        operation_id: Uuid::new_v4(),
        kind: DeliveryKind::Process,
        state: outcome.state,
        may_have_run: outcome.may_have_run,
        error: outcome.error,
    };
    assert!(!serde_json::to_string(&status).unwrap().contains(CANARY));
}

#[tokio::test(flavor = "multi_thread")]
async fn executable_change_and_pre_cancel_refuse_dispatch() {
    let (root, config, material) = fixture("#!/bin/sh\nexit 0\n");
    let digest = process::inspect(&config).unwrap();
    fs::write(&config.executable, "#!/bin/sh\nprintf 'changed'\n").unwrap();
    let result = process::execute(
        config,
        digest,
        material,
        Uuid::new_v4(),
        CancellationToken::new(),
    )
    .await;
    assert!(!result.may_have_run);
    assert_ne!(result.state, DeliveryState::Completed);
    drop(root);
    let (_root, config, material) = fixture("#!/bin/sh\nexit 0\n");
    let digest = process::inspect(&config).unwrap();
    let cancel = CancellationToken::new();
    cancel.cancel();
    let result = process::execute(config, digest, material, Uuid::new_v4(), cancel).await;
    assert_eq!(result.state, DeliveryState::Cancelled);
    assert!(!result.may_have_run);
}

#[tokio::test(flavor = "multi_thread")]
async fn cancellation_and_output_flood_are_bounded_and_never_report_success() {
    let (_root, config, material) = fixture("#!/bin/sh\n/bin/sleep 30\n");
    let digest = process::inspect(&config).unwrap();
    let cancel = CancellationToken::new();
    let signal = cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(150)).await;
        signal.cancel();
    });
    let result = tokio::time::timeout(
        Duration::from_secs(5),
        process::execute(config, digest, material, Uuid::new_v4(), cancel),
    )
    .await
    .unwrap();
    assert_ne!(result.state, DeliveryState::Completed);
    let (_root, config, material) =
        fixture("#!/bin/sh\nwhile :; do printf '%s' \"$MV_TOKEN\"; done\n");
    let digest = process::inspect(&config).unwrap();
    let result = tokio::time::timeout(
        Duration::from_secs(5),
        process::execute(
            config,
            digest,
            material,
            Uuid::new_v4(),
            CancellationToken::new(),
        ),
    )
    .await
    .unwrap();
    assert_ne!(result.state, DeliveryState::Completed);
    assert!(result.may_have_run);
}

#[test]
fn symlink_and_unsafe_environment_destinations_are_refused() {
    let (_root, mut config, _material) = fixture("#!/bin/sh\nexit 0\n");
    config.environment[0].name = "LD_PRELOAD".into();
    assert!(process::inspect(&config).is_err());
    config.environment[0].name = "MV_TOKEN".into();
    let link = PathBuf::from(&config.executable).with_extension("link");
    std::os::unix::fs::symlink(&config.executable, &link).unwrap();
    config.executable = link.to_str().unwrap().into();
    assert!(process::inspect(&config).is_err());
}
use std::path::PathBuf;

#[tokio::test(flavor = "multi_thread")]
async fn dropping_an_embedded_call_still_requests_owned_child_cleanup() {
    let (root, config, material) = fixture(
        "#!/bin/sh\nprintf '%s' $$ > child-pid\n/bin/sleep 30\nprintf 'late' > late-marker\n",
    );
    let digest = process::inspect(&config).unwrap();
    let worker = tokio::spawn(process::execute(
        config,
        digest,
        material,
        Uuid::new_v4(),
        CancellationToken::new(),
    ));
    let pid_file = root.path().join("child-pid");
    let pid: i32 = tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if let Ok(value) = fs::read_to_string(&pid_file) {
                if let Ok(pid) = value.parse() {
                    break pid;
                }
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    worker.abort();
    assert!(matches!(worker.await, Err(error) if error.is_cancelled()));
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            // Read-only existence check of the fixture's own recorded child.
            if unsafe { libc::kill(pid, 0) } == -1 {
                assert_eq!(
                    std::io::Error::last_os_error().raw_os_error(),
                    Some(libc::ESRCH)
                );
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert!(!root.path().join("late-marker").exists());
}
