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
        let root = tempfile::Builder::new()
            .permissions(fs::Permissions::from_mode(0o700))
            .tempdir()
            .unwrap();
        let executable = root.path().join("recipient");
        fs::write(&executable, "#!/bin/sh\nprintf 'entered' > entered\n[ -n \"$MV_TOKEN\" ] || exit 1\nprintf 'present' > material-present\nprintf '%s' \"$MV_TOKEN\"\nprintf 'done' > marker\n").unwrap();
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
            "trial {round}; closed error {:?}; observed {:?}; entered {}; material present {}; completed {}",
            outcome.error, observed, root.path().join("entered").exists(),
            root.path().join("material-present").exists(), root.path().join("marker").exists());
        let observed = observed.unwrap();
        assert_eq!(observed.terminal, GovernedExecutionTerminal::Success);
        assert!(observed.has_exit_code && observed.stderr_empty);
        assert!(!observed.permission_error && !observed.missing_file_error);
        assert_eq!(fs::read(root.path().join("marker")).unwrap(), b"done");
    }
    eprintln!(
        "200 synthetic process launches completed in {:?}",
        started.elapsed()
    );
}
