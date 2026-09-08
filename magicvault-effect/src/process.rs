//! Human-profile-bound batch execution through MagicRun's public coordinator.
//! No shell parsing, ambient environment, raw output or exit-code projection.
use crate::delivery::{DeliveryMaterial, DeliveryOutcome};
use magicvault_protocol::{
    DeliveryDestination, DeliveryProfile, ErrorCode, InputValue, ProcessDestination,
};
use std::{collections::BTreeSet, fs, io::Read, path::Path, sync::Arc, time::Instant};
use tokio_util::sync::CancellationToken;
use tool_runtime_core::{
    credential_injection::{
        ChildEnvironmentBaseline, ChildEnvironmentVariable, CredentialCallId,
        CredentialInjectionPlan,
    },
    credential_materialization::ChildEnvironmentValues,
    credential_preparation::{
        CredentialMaterialBindingName, CredentialMaterialKind, CredentialMaterialResolver,
        CredentialMaterialSink, CredentialPreparationBinding, CredentialPreparationError,
        CredentialPreparationPlan, CredentialResolutionFailure,
    },
    credential_profiles::{self as profiles, CredentialProfileBinding, CredentialScope},
    governed_batch_process::GovernedBatchCancellation,
    governed_execution::{
        GovernedExecutionContract, GovernedExecutionDispatch, GovernedExecutionPolicy,
        GovernedExecutionRequest, GovernedExecutionTerminal,
    },
    governed_execution_authority::{
        GovernedExpectedExecutableDigest, GovernedWorkingDirectoryRoot,
    },
    governed_execution_coordinator::{
        GovernedAuthorizationDecision, GovernedAuthorizationEvidence, GovernedAuthorizationRequest,
        GovernedExecutionAuditError, GovernedExecutionAuditReceipt, GovernedExecutionAuditSink,
        GovernedExecutionAuthorizer, GovernedExecutionCallContext, GovernedExecutionInvocation,
    },
    manifest::*,
    manifest_validation::validate_skill_runtime_contract,
    profile_selection::{select_credential_profile, CredentialProfileSelectionRequest},
};
use uuid::Uuid;

const OUTPUT_LIMIT: u64 = 64 * 1024;
const INPUT_LIMIT: u64 = 64 * 1024;
const MAX_EXECUTABLE_BYTES: u64 = 64 * 1024 * 1024;

fn invalid<T>(_: T) -> ErrorCode {
    ErrorCode::UnsupportedTarget
}

/// Capture the content constraint before requesting human profile approval.
/// MagicRun rechecks bytes from its own opened executable at the dispatch fence.
pub fn inspect(config: &ProcessDestination) -> Result<[u8; 32], ErrorCode> {
    let contract = contract(config)?;
    validate_skill_runtime_contract(&contract).map_err(invalid)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
        let mut options = fs::OpenOptions::new();
        options
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK);
        let mut file = options.open(&config.executable).map_err(invalid)?;
        let metadata = file.metadata().map_err(invalid)?;
        if !metadata.is_file()
            || metadata.len() > MAX_EXECUTABLE_BYTES
            || metadata.permissions().mode() & 0o111 == 0
            || metadata.permissions().mode() & 0o022 != 0
            || !matches!(metadata.uid(), 0) && metadata.uid() != unsafe { libc::geteuid() }
        {
            return Err(ErrorCode::UnsupportedTarget);
        }
        let cwd = fs::symlink_metadata(&config.working_directory).map_err(invalid)?;
        if !cwd.is_dir() || cwd.file_type().is_symlink() {
            return Err(ErrorCode::UnsupportedTarget);
        }
        let mut hasher = blake3::Hasher::new();
        let mut buffer = [0u8; 16 * 1024];
        let mut length = 0u64;
        loop {
            let count = file.read(&mut buffer).map_err(invalid)?;
            if count == 0 {
                break;
            }
            length += count as u64;
            if length > MAX_EXECUTABLE_BYTES {
                return Err(ErrorCode::Capacity);
            }
            hasher.update(&buffer[..count]);
        }
        Ok(*hasher.finalize().as_bytes())
    }
    #[cfg(not(unix))]
    {
        Err(ErrorCode::UnsupportedTarget)
    }
}

fn slots(config: &ProcessDestination) -> Vec<(&InputValue, InjectionTarget)> {
    let mut result = config
        .environment
        .iter()
        .map(|row| {
            (
                &row.value,
                InjectionTarget::Environment {
                    name: row.name.clone(),
                },
            )
        })
        .collect::<Vec<_>>();
    if let Some(stdin) = &config.stdin {
        result.push((stdin, InjectionTarget::Stdin));
    }
    result
}

fn contract(config: &ProcessDestination) -> Result<SkillRuntimeContract, ErrorCode> {
    if !(DeliveryProfile {
        label: "Process adapter".into(),
        destination: DeliveryDestination::Process(config.clone()),
    })
    .valid()
    {
        return Err(ErrorCode::InvalidRequest);
    }
    let timeout = u32::try_from(config.timeout_secs).map_err(invalid)?;
    if !(1..=120).contains(&timeout) {
        return Err(ErrorCode::InvalidRequest);
    }
    let executable = Path::new(&config.executable)
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or(ErrorCode::UnsupportedTarget)?
        .to_owned();
    let slots = slots(config);
    Ok(SkillRuntimeContract {
        schema_version: SkillRuntimeContractVersion::v1(),
        requires: RuntimeRequirements {
            bins: BTreeSet::from([executable]),
            ..Default::default()
        },
        runtime: RuntimeProtocol::Cli {
            command_prefix: vec![],
            interaction: CliInteraction::Batch,
            // Agent stdin is denied. Any configured stdin comes exclusively
            // through the credential materialization contract below.
            stdin: StdinContract {
                mode: StdinMode::Denied,
                sensitivity: DataSensitivity::Public,
            },
            working_directory: WorkingDirectoryContract {
                mode: WorkingDirectoryMode::Workspace,
            },
            limits: RuntimeLimits {
                timeout_secs: Some(timeout),
                stdin_bytes: None,
                stdout_bytes: Some(OUTPUT_LIMIT),
                stderr_bytes: Some(OUTPUT_LIMIT),
                memory_bytes: None,
            },
        },
        auth: AuthContract {
            kind: AuthKind::Secrets,
            requirement: AuthRequirement::Required,
            secret_bindings: slots
                .iter()
                .enumerate()
                .map(|(index, _)| SecretBindingRef {
                    name: format!("slot_{index}"),
                    secret_ref: format!("MAGICVAULT_SLOT_{index}"),
                })
                .collect(),
            injections: slots
                .into_iter()
                .enumerate()
                .map(|(index, (_, target))| InjectionBinding {
                    source: InjectionSource::Secret {
                        binding: format!("slot_{index}"),
                    },
                    target,
                })
                .collect(),
            ..Default::default()
        },
        policy_floor: PolicyFloor {
            approval: ApprovalClass::ConditionalExternalSideEffect,
            ..Default::default()
        },
    })
}

struct Resolver {
    values: Vec<InputValue>,
    material: DeliveryMaterial,
}

// This adapter uses explicit secret bindings, not profile-owned directories.
// The public `None` selection path never calls this registry; all methods fail
// closed if a future contract accidentally starts requesting profile authority.
struct NoProfiles;
fn no_profiles() -> profiles::CredentialProfileError {
    profiles::CredentialProfileError::registry_unavailable()
}
impl profiles::CredentialProfileRegistry for NoProfiles {
    fn snapshot(
        &self,
        _: &CredentialScope,
    ) -> Result<profiles::CredentialProfileRegistrySnapshot, profiles::CredentialProfileError> {
        Err(no_profiles())
    }
    fn status(
        &self,
        _: &profiles::CredentialProfileKey,
    ) -> Result<Option<profiles::CredentialProfileStatus>, profiles::CredentialProfileError> {
        Err(no_profiles())
    }
    fn create_reference(
        &self,
        _: profiles::CreateCredentialProfileReference,
    ) -> Result<profiles::CredentialProfileStatus, profiles::CredentialProfileError> {
        Err(no_profiles())
    }
    fn update_metadata(
        &self,
        _: profiles::UpdateCredentialProfileMetadata,
    ) -> Result<profiles::CredentialProfileStatus, profiles::CredentialProfileError> {
        Err(no_profiles())
    }
    fn set_disabled(
        &self,
        _: profiles::SetCredentialProfileDisabled,
    ) -> Result<profiles::CredentialProfileStatus, profiles::CredentialProfileError> {
        Err(no_profiles())
    }
}
impl CredentialMaterialResolver for Resolver {
    fn resolve_once(
        &mut self,
        _: &CredentialPreparationPlan,
        sink: &mut CredentialMaterialSink<'_>,
    ) -> Result<(), CredentialPreparationError> {
        for (index, value) in self.values.iter().enumerate() {
            let value = self.material.render(value).map_err(|_| {
                CredentialPreparationError::resolution(CredentialResolutionFailure::Unavailable)
            })?;
            sink.provide(
                &CredentialMaterialBindingName::new(format!("slot_{index}"))?,
                value.as_bytes().to_vec(),
            )?;
        }
        Ok(())
    }
}

struct ApprovedProfile {
    arguments: Vec<String>,
    operation_id: String,
    cancel: Arc<GovernedBatchCancellation>,
}
impl GovernedExecutionAuthorizer for ApprovedProfile {
    fn authorize(
        &mut self,
        request: &GovernedAuthorizationRequest<'_>,
    ) -> GovernedAuthorizationDecision {
        if self.cancel.is_cancelled()
            || Instant::now() >= request.deadline()
            || request.arguments() != self.arguments
            || request.has_stdin()
        {
            return GovernedAuthorizationDecision::Denied;
        }
        match GovernedAuthorizationEvidence::new(
            request.request_digest(),
            "magicvault-profile-v1",
            ApprovalClass::ConditionalExternalSideEffect,
            Some(self.operation_id.clone()),
            BTreeSet::new(),
            BTreeSet::new(),
            BTreeSet::new(),
        ) {
            Ok(evidence) => GovernedAuthorizationDecision::Approved(evidence),
            Err(_) => GovernedAuthorizationDecision::Denied,
        }
    }
}

// The broker durably records authorization before invoking this adapter, then
// persists the closed completion receipt. A completion persistence failure
// poisons standalone custody and returns uncertainty, never successful rollback.
struct SettlementAudit;
impl GovernedExecutionAuditSink for SettlementAudit {
    fn record(
        &mut self,
        _: &GovernedExecutionAuditReceipt,
    ) -> Result<(), GovernedExecutionAuditError> {
        Ok(())
    }
}

fn run(
    config: ProcessDestination,
    digest: [u8; 32],
    material: DeliveryMaterial,
    operation_id: Uuid,
    cancellation: Arc<GovernedBatchCancellation>,
) -> Result<DeliveryOutcome, ErrorCode> {
    if cancellation.is_cancelled() {
        return Err(ErrorCode::Cancelled);
    }
    let contract = contract(&config)?;
    let validated = validate_skill_runtime_contract(&contract).map_err(invalid)?;
    let scope = CredentialScope::new("magicvault", operation_id.to_string()).map_err(invalid)?;
    let selection = select_credential_profile(
        &NoProfiles,
        &CredentialProfileSelectionRequest::new(
            scope.clone(),
            None,
            CredentialProfileBinding::Provider,
            &ProfileSelection::None,
            None,
        )
        .map_err(invalid)?,
    )
    .map_err(invalid)?;
    let values = slots(&config)
        .into_iter()
        .map(|(v, _)| v.clone())
        .collect::<Vec<_>>();
    let bindings = values
        .iter()
        .enumerate()
        .map(|(index, _)| {
            CredentialPreparationBinding::new(
                CredentialMaterialBindingName::new(format!("slot_{index}"))?,
                CredentialMaterialKind::SecretBinding,
                4608,
            )
        })
        .collect::<Result<Vec<_>, _>>()
        .map_err(invalid)?;
    let preparation =
        CredentialPreparationPlan::new(scope, AuthKind::Secrets, &selection, bindings)
            .map_err(invalid)?;
    let injection = CredentialInjectionPlan::compile(
        validated,
        &preparation,
        ChildEnvironmentBaseline::portable_cli(),
    )
    .map_err(invalid)?;
    let timeout = u32::try_from(config.timeout_secs).map_err(invalid)?;
    let intent = GovernedExecutionContract::compile(
        validated,
        GovernedExecutionPolicy::new(timeout, timeout, INPUT_LIMIT, OUTPUT_LIMIT, OUTPUT_LIMIT)
            .map_err(invalid)?,
    )
    .map_err(invalid)?
    .admit(GovernedExecutionRequest::new(
        config.arguments.clone(),
        None,
        None,
        Some(timeout),
    ))
    .map_err(invalid)?;
    let mut environment = ChildEnvironmentValues::new(injection.baseline());
    let directory = Path::new(&config.executable)
        .parent()
        .and_then(|p| p.to_str())
        .ok_or(ErrorCode::UnsupportedTarget)?;
    // The sole resolution directory is the human-approved executable's parent;
    // no inherited PATH or environment can choose a different command.
    environment
        .provide(
            ChildEnvironmentVariable::Path,
            directory.as_bytes().to_vec(),
        )
        .map_err(invalid)?;
    environment
        .provide(ChildEnvironmentVariable::Lang, b"C.UTF-8".to_vec())
        .map_err(invalid)?;
    let invocation = GovernedExecutionInvocation::new(
        GovernedExecutionCallContext::new(
            CredentialCallId::new(operation_id.to_string()).map_err(invalid)?,
            "magicvault",
            "secure_new_process",
        )
        .map_err(invalid)?,
        validated,
        intent,
        &preparation,
        &injection,
        environment,
        Some(
            GovernedWorkingDirectoryRoot::open(
                WorkingDirectoryMode::Workspace,
                Path::new(&config.working_directory),
            )
            .map_err(invalid)?,
        ),
        None,
        None,
    )
    .map_err(invalid)?
    .with_expected_executable_digest(GovernedExpectedExecutableDigest::from_blake3(digest));
    let mut resolver = Resolver { values, material };
    let mut authorizer = ApprovedProfile {
        arguments: config.arguments,
        operation_id: operation_id.to_string(),
        cancel: cancellation.clone(),
    };
    #[cfg(magicvault_test_diagnostics)]
    crate::test_diagnostics::entered(operation_id);
    let result = invocation.execute_batch(
        &mut authorizer,
        &mut resolver,
        &mut SettlementAudit,
        &cancellation,
    );
    Ok(match result {
        Ok(settlement) => {
            let terminal = settlement.result().terminal();
            #[cfg(all(test, unix))]
            reliability::observe(settlement.result());
            #[cfg(magicvault_test_diagnostics)]
            crate::test_diagnostics::settled(operation_id, settlement.result());
            // No sealed stream, raw exit code or runtime diagnostic is returned.
            match terminal.terminal() {
                GovernedExecutionTerminal::Success => DeliveryOutcome::completed(),
                GovernedExecutionTerminal::Cancelled => {
                    DeliveryOutcome::uncertain(ErrorCode::Cancelled)
                }
                GovernedExecutionTerminal::TimedOut => {
                    DeliveryOutcome::uncertain(ErrorCode::Expired)
                }
                GovernedExecutionTerminal::OutputLimitExceeded => {
                    DeliveryOutcome::uncertain(ErrorCode::Capacity)
                }
                _ if terminal.dispatch() == GovernedExecutionDispatch::NotDispatched => {
                    DeliveryOutcome::failed(ErrorCode::Unavailable)
                }
                _ => DeliveryOutcome::uncertain(ErrorCode::Unavailable),
            }
        }
        Err(error) if error.dispatch() == GovernedExecutionDispatch::NotDispatched => {
            #[cfg(magicvault_test_diagnostics)]
            crate::test_diagnostics::runtime_error(operation_id, error.dispatch());
            DeliveryOutcome::failed(ErrorCode::Unavailable)
        }
        Err(error) => {
            #[cfg(magicvault_test_diagnostics)]
            crate::test_diagnostics::runtime_error(operation_id, error.dispatch());
            let _ = error;
            DeliveryOutcome::uncertain(ErrorCode::TransportUncertain)
        }
    })
}

#[cfg(all(test, unix))]
#[path = "../tests/support/process_reliability.rs"]
mod reliability;

pub async fn execute(
    config: ProcessDestination,
    digest: [u8; 32],
    material: DeliveryMaterial,
    operation_id: Uuid,
    cancel: CancellationToken,
) -> DeliveryOutcome {
    if cancel.is_cancelled() {
        return DeliveryOutcome::failed(ErrorCode::Cancelled);
    }
    let cancellation = Arc::new(GovernedBatchCancellation::new());
    // Embedders may drop this future. Still ask the owned worker to reap its
    // child; the broker's normal cancellation path below also awaits cleanup.
    struct CancelOnDrop(Arc<GovernedBatchCancellation>);
    impl Drop for CancelOnDrop {
        fn drop(&mut self) {
            self.0.cancel();
        }
    }
    let _guard = CancelOnDrop(cancellation.clone());
    let child_cancel = cancellation.clone();
    let mut worker = tokio::task::spawn_blocking(move || {
        let result = run(config, digest, material, operation_id, child_cancel);
        #[cfg(magicvault_test_diagnostics)]
        crate::test_diagnostics::returned(operation_id, &result);
        result
    });
    // Never detach a credential-bearing process worker when cancellation wins.
    // Await MagicRun's owned-child cleanup before releasing the broker job gate.
    let result = tokio::select! {
        result = &mut worker => result,
        _ = cancel.cancelled() => { cancellation.cancel(); worker.await },
    };
    match result {
        Ok(Ok(outcome)) => outcome,
        Ok(Err(error)) => DeliveryOutcome::failed(error),
        Err(_) => DeliveryOutcome::uncertain(ErrorCode::TransportUncertain),
    }
}
