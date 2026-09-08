//! Real synthetic recipients validate the diagnostic classifications. No
//! production binary or default test run enables this observer.
#![cfg(all(unix, magicvault_test_diagnostics))]
use magicvault_effect::{delivery::DeliveryMaterial, process, test_diagnostics::Capture};
use magicvault_protocol::{DeliveryState, InputValue, NamedValue, ProcessDestination};
use std::{fs, os::unix::fs::PermissionsExt};
use tokio_util::sync::CancellationToken;
use tool_runtime_core::governed_execution::GovernedExecutionTerminal;
use tool_runtime_core::process_test_diagnostics::Signal;
use uuid::Uuid;
use zeroize::Zeroizing;

const CANARY: &str = "SYNTHETIC-DIAGNOSTIC-CANARY";

#[tokio::test]
async fn observed_terminals_distinguish_exit_signal_and_known_errors_without_material() {
    for (body, terminal, expected_signal, exit_code, permission, missing) in [
        (
            "printf '%s' \"$MV_TOKEN\" >&2; exit 17",
            GovernedExecutionTerminal::NonZeroExit,
            None,
            true,
            false,
            false,
        ),
        (
            "kill -TERM $$",
            GovernedExecutionTerminal::RuntimeFailure,
            Some(Signal::Terminate),
            false,
            false,
            false,
        ),
        (
            "kill -KILL $$",
            GovernedExecutionTerminal::RuntimeFailure,
            Some(Signal::Kill),
            false,
            false,
            false,
        ),
        (
            "./not-executable",
            GovernedExecutionTerminal::NonZeroExit,
            None,
            true,
            true,
            false,
        ),
        (
            "./absent",
            GovernedExecutionTerminal::NonZeroExit,
            None,
            true,
            false,
            true,
        ),
    ] {
        let root = tempfile::Builder::new()
            .permissions(fs::Permissions::from_mode(0o700))
            .tempdir()
            .unwrap();
        let recipient = root.path().join("recipient");
        fs::write(&recipient, format!("#!/bin/sh\n{body}\n")).unwrap();
        fs::set_permissions(&recipient, fs::Permissions::from_mode(0o700)).unwrap();
        fs::write(root.path().join("not-executable"), b"fixture").unwrap();
        fs::set_permissions(
            root.path().join("not-executable"),
            fs::Permissions::from_mode(0o600),
        )
        .unwrap();
        let reference = format!("cred_{}", Uuid::new_v4());
        let config = ProcessDestination {
            executable: recipient.to_str().unwrap().into(),
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
        let digest = process::inspect(&config).unwrap();
        let mut material = DeliveryMaterial::default();
        material.insert(reference, "token".into(), Zeroizing::new(CANARY.into()));
        let id = Uuid::new_v4();
        let observation = Capture::register(id).unwrap();
        let result = process::execute(config, digest, material, id, CancellationToken::new()).await;
        assert_eq!(result.state, DeliveryState::Uncertain);
        let snapshot = observation.snapshot();
        assert!(!format!("{snapshot:?}").contains(CANARY));
        assert!(snapshot.runtime_entered && snapshot.adapter_returned);
        let child = snapshot
            .process
            .expect("runtime signal observer must be wired to this operation");
        assert_eq!(child.spawned_children, 1);
        #[cfg(target_os = "macos")]
        assert_eq!(
            child.spawn_method,
            Some(tool_runtime_core::process_test_diagnostics::SpawnMethod::MacosPosixSpawn)
        );
        #[cfg(target_os = "macos")]
        assert_eq!(child.pre_exec_stage, None);
        assert!(child.cleanup_before_reap && !child.termination_cleanup);
        assert_eq!(child.reaped_signal, expected_signal);
        #[cfg(target_os = "macos")]
        assert_eq!(
            child.os_exit_reason,
            expected_signal.map(|signal| {
                use tool_runtime_core::process_test_diagnostics::{ExitReason, OsReason};
                ExitReason::Observed(OsReason::Signal(signal))
            })
        );
        #[cfg(not(target_os = "macos"))]
        assert_eq!(
            child.os_exit_reason,
            expected_signal
                .map(|_| { tool_runtime_core::process_test_diagnostics::ExitReason::Unsupported })
        );
        assert!(child.last_wait.unwrap().owned_child);
        assert_eq!(snapshot.terminal, Some(terminal));
        assert_eq!(snapshot.has_exit_code, exit_code);
        assert_eq!(snapshot.permission_error, permission);
        assert_eq!(snapshot.missing_file_error, missing);
    }
}
