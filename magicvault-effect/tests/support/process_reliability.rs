//! Synthetic-only diagnostics. This module is absent from shipped binaries.
use super::*;
use magicvault_protocol::{DeliveryState, NamedValue};
use std::{cell::RefCell, os::unix::fs::PermissionsExt, time::Duration};
use tool_runtime_core::governed_execution_result::GovernedExecutionResult;
use zeroize::Zeroizing;

#[derive(Debug)]
struct Observation {
    terminal: GovernedExecutionTerminal,
    has_exit_code: bool,
    stderr_empty: bool,
    permission_error: bool,
    missing_file_error: bool,
}
thread_local! {
    static OBSERVATION: RefCell<Option<Observation>> = const { RefCell::new(None) };
}

pub(super) fn observe(result: &GovernedExecutionResult) {
    // Classify only known diagnostics; never print arbitrary streams or codes.
    let stderr = String::from_utf8_lossy(result.stderr());
    OBSERVATION.set(Some(Observation {
        terminal: result.terminal().terminal(),
        has_exit_code: result.exit_code().is_some(),
        stderr_empty: stderr.is_empty(),
        permission_error: stderr.contains("Permission denied")
            || stderr.contains("Operation not permitted"),
        missing_file_error: stderr.contains("No such file or directory"),
    }));
}

#[test]
#[ignore = "bounded synthetic process stress; explicit reliability qualification"]
fn repeated_process_launches_preserve_delivery_and_success() {
    let started = Instant::now();
    for round in 0..200 {
        assert!(
            started.elapsed() < Duration::from_secs(90),
            "stress budget exhausted at trial {round}"
        );
        one_launch(round, true);
    }
    eprintln!(
        "200 synthetic process launches completed in {:?}",
        started.elapsed()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "focused async launch/SIGCHLD diagnostic; synthetic material only"]
async fn async_process_launches_with_child_churn() {
    let started = Instant::now();
    // Match the failing topology: a blocking governed executor under Tokio,
    // while a separate async child owner repeatedly receives exit notifications.
    // --list starts this trusted test executable without recursively running it.
    let executable = std::env::current_exe().unwrap();
    let mut noise_children = 0;
    for round in 0..200 {
        assert!(
            started.elapsed() < Duration::from_secs(90),
            "async stress budget exhausted at trial {round}"
        );
        // Preserve the original minimal script on alternate trials: stage writes
        // can change scheduling and must not be assumed to reproduce its timing.
        let delivery = tokio::task::spawn_blocking(move || one_launch(round, round % 2 == 0));
        let noise = async {
            // Distribute the noise across every trial instead of exhausting all
            // child exits during the first few deliveries. Await both owners
            // before propagating an assertion failure from the governed worker.
            for _ in 0..10 {
                let mut child = tokio::process::Command::new(&executable)
                    .arg("--list")
                    .stdin(std::process::Stdio::null())
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .kill_on_drop(true)
                    .spawn()
                    .unwrap();
                match tokio::time::timeout(Duration::from_secs(5), child.wait()).await {
                    Ok(status) => assert!(status.unwrap().success()),
                    Err(_) => {
                        let _ = child.kill().await;
                        panic!("synthetic noise child exceeded launch bound");
                    }
                }
                noise_children += 1;
            }
        };
        let (delivered, ()) = tokio::join!(delivery, noise);
        delivered.unwrap();
    }
    assert_eq!(noise_children, 2000);
    eprintln!(
        "200 async synthetic launches and {noise_children} noise children completed in {:?}",
        started.elapsed()
    );
}

fn one_launch(round: usize, staged: bool) {
    let root = tempfile::Builder::new()
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir()
        .unwrap();
    let executable = root.path().join("recipient");
    fs::write(&executable, if staged {
            "#!/bin/sh\nprintf 'entered' > entered\n[ -n \"$MV_TOKEN\" ] || exit 1\nprintf 'present' > material-present\nprintf '%s' \"$MV_TOKEN\"\nprintf 'done' > marker\n"
        } else {
            "#!/bin/sh\n[ -n \"$MV_TOKEN\" ] || exit 1\nprintf '%s' \"$MV_TOKEN\"\nprintf 'done' > marker\n"
        }).unwrap();
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
    let reference = format!("cred_{}", Uuid::new_v4());
    let config = ProcessDestination {
        executable: executable.to_str().unwrap().into(),
        arguments: vec![],
        working_directory: root.path().to_str().unwrap().into(),
        environment: vec![NamedValue {
            name: "MV_TOKEN".into(),
            value: InputValue::Credential {
                credential_ref: reference.clone(),
                credential_field: "token".into(),
                prefix: String::new(),
                suffix: String::new(),
            },
        }],
        stdin: None,
        timeout_secs: 2,
    };
    let digest = inspect(&config).unwrap();
    let mut material = DeliveryMaterial::default();
    material.insert(
        reference,
        "token".into(),
        Zeroizing::new("SYNTHETIC-PROCESS-RELIABILITY".into()),
    );
    OBSERVATION.set(None);
    let outcome = run(
        config,
        digest,
        material,
        Uuid::new_v4(),
        Arc::new(GovernedBatchCancellation::new()),
    )
    .unwrap();
    let observed = OBSERVATION.take();
    assert_eq!(outcome.state, DeliveryState::Completed,
            "trial {round}; staged {staged}; closed error {:?}; observed {:?}; entered {}; material present {}; completed {}",
            outcome.error, observed, root.path().join("entered").exists(),
            root.path().join("material-present").exists(), root.path().join("marker").exists());
    let observed = observed.unwrap();
    assert_eq!(observed.terminal, GovernedExecutionTerminal::Success);
    assert!(observed.has_exit_code && observed.stderr_empty);
    assert!(!observed.permission_error && !observed.missing_file_error);
    assert_eq!(fs::read(root.path().join("marker")).unwrap(), b"done");
}
