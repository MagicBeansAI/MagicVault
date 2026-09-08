//! Explicit debug-only synthetic-test seam, absent from normal builds. This is
//! not an agent tool, production log, callback API or Cargo feature. Register
//! only a fixture's own operation before dispatch; no unregistered work is kept.
use crate::delivery::DeliveryOutcome;
use magicvault_protocol::{DeliveryState, ErrorCode};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex, OnceLock},
};
use tool_runtime_core::{
    governed_execution::{GovernedExecutionDispatch, GovernedExecutionTerminal},
    governed_execution_result::GovernedExecutionResult,
};
use uuid::Uuid;

pub use tool_runtime_core::process_test_diagnostics::SpawnMethod;

const CAPACITY: usize = 16;

/// Closed categories only. Deliberately not serializable; contains no byte
/// buffers, strings, recipient paths, raw streams, exit codes or credentials.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Snapshot {
    pub runtime_entered: bool,
    pub process: Option<tool_runtime_core::process_test_diagnostics::Snapshot>,
    pub terminal: Option<GovernedExecutionTerminal>,
    pub dispatch: Option<GovernedExecutionDispatch>,
    pub runtime_error: bool,
    pub has_exit_code: bool,
    pub stderr_empty: bool,
    pub permission_error: bool,
    pub missing_file_error: bool,
    pub adapter_returned: bool,
    pub adapter_state: Option<DeliveryState>,
    pub adapter_error: Option<ErrorCode>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum RegistrationError {
    Duplicate,
    Capacity,
}

#[derive(Default)]
struct Registry(Mutex<BTreeMap<Uuid, Arc<Mutex<Snapshot>>>>);

fn registry() -> &'static Arc<Registry> {
    static REGISTRY: OnceLock<Arc<Registry>> = OnceLock::new();
    REGISTRY.get_or_init(|| Arc::new(Registry::default()))
}

pub struct Capture {
    operation: Uuid,
    registry: Arc<Registry>,
    snapshot: Arc<Mutex<Snapshot>>,
}

impl Registry {
    fn register(self: &Arc<Self>, operation: Uuid) -> Result<Capture, RegistrationError> {
        let mut slots = self.0.lock().unwrap_or_else(|e| e.into_inner());
        if slots.contains_key(&operation) {
            return Err(RegistrationError::Duplicate);
        }
        if slots.len() >= CAPACITY {
            return Err(RegistrationError::Capacity);
        }
        let snapshot = Arc::new(Mutex::new(Snapshot::default()));
        slots.insert(operation, snapshot.clone());
        Ok(Capture {
            operation,
            registry: self.clone(),
            snapshot,
        })
    }

    fn update(&self, operation: Uuid, update: impl FnOnce(&mut Snapshot)) {
        let slot = self
            .0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&operation)
            .cloned();
        if let Some(slot) = slot {
            update(&mut slot.lock().unwrap_or_else(|e| e.into_inner()));
        }
    }
}

impl Capture {
    pub fn register(operation: Uuid) -> Result<Self, RegistrationError> {
        registry().register(operation)
    }

    pub fn snapshot(&self) -> Snapshot {
        *self.snapshot.lock().unwrap_or_else(|e| e.into_inner())
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        let mut slots = self.registry.0.lock().unwrap_or_else(|e| e.into_inner());
        if slots
            .get(&self.operation)
            .is_some_and(|slot| Arc::ptr_eq(slot, &self.snapshot))
        {
            slots.remove(&self.operation);
        }
    }
}

pub(crate) fn entered(operation: Uuid) {
    registry().update(operation, |s| s.runtime_entered = true);
}

pub(crate) fn observe_process(
    operation: Uuid,
) -> Option<tool_runtime_core::process_test_diagnostics::Capture> {
    let registered = registry()
        .0
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .contains_key(&operation);
    registered.then(|| {
        tool_runtime_core::process_test_diagnostics::Capture::start()
            .expect("synthetic invocation must own its process capture")
    })
}

pub(crate) fn process_observed(
    operation: Uuid,
    observation: tool_runtime_core::process_test_diagnostics::Capture,
) {
    registry().update(operation, |s| s.process = Some(observation.snapshot()));
    // Capture is dropped on this exact blocking invocation thread.
}

pub(crate) fn settled(operation: Uuid, result: &GovernedExecutionResult) {
    registry().update(operation, |s| {
        let terminal = result.terminal();
        s.terminal = Some(terminal.terminal());
        s.dispatch = Some(terminal.dispatch());
        s.has_exit_code = result.exit_code().is_some();
        s.stderr_empty = result.stderr().is_empty();
        // Borrow bounded stream bytes; do not create an unzeroized text copy.
        let contains = |needle: &[u8]| result.stderr().windows(needle.len()).any(|w| w == needle);
        s.permission_error = contains(b"Permission denied") || contains(b"Operation not permitted");
        s.missing_file_error = contains(b"No such file or directory") || contains(b": not found");
    });
}

pub(crate) fn runtime_error(operation: Uuid, dispatch: GovernedExecutionDispatch) {
    registry().update(operation, |s| {
        s.runtime_error = true;
        s.dispatch = Some(dispatch);
    });
}

pub(crate) fn returned(operation: Uuid, outcome: &Result<DeliveryOutcome, ErrorCode>) {
    registry().update(operation, |s| {
        s.adapter_returned = true;
        match outcome {
            Ok(outcome) => {
                s.adapter_state = Some(outcome.state);
                s.adapter_error = outcome.error;
            }
            Err(error) => s.adapter_error = Some(*error),
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn process_capture_requires_registration_and_releases_thread_slot() {
        let operation = Uuid::new_v4();
        assert!(observe_process(operation).is_none());
        let capture = Capture::register(operation).unwrap();
        let process = observe_process(operation).unwrap();
        assert!(matches!(
            tool_runtime_core::process_test_diagnostics::Capture::start(),
            Err(tool_runtime_core::process_test_diagnostics::StartError::AlreadyActive)
        ));
        process_observed(operation, process);
        assert_eq!(capture.snapshot().process, Some(Default::default()));
        drop(tool_runtime_core::process_test_diagnostics::Capture::start().unwrap());
        drop(capture);
        assert!(observe_process(operation).is_none());
    }

    #[test]
    fn bounded_registration_refuses_duplicates_and_releases_capacity() {
        let registry = Arc::new(Registry::default());
        let ids: Vec<_> = (0..CAPACITY).map(|_| Uuid::new_v4()).collect();
        let mut captures: Vec<_> = ids
            .iter()
            .map(|id| registry.register(*id).unwrap())
            .collect();
        assert!(matches!(
            registry.register(ids[0]),
            Err(RegistrationError::Duplicate)
        ));
        assert!(matches!(
            registry.register(Uuid::new_v4()),
            Err(RegistrationError::Capacity)
        ));
        captures.pop();
        let next = registry.register(Uuid::new_v4()).unwrap();
        drop(captures);
        drop(next);
        assert!(registry.0.lock().unwrap().is_empty());
    }

    #[test]
    fn cross_thread_observation_is_operation_scoped_and_does_not_retain_unregistered_work() {
        let registry = Arc::new(Registry::default());
        let first = registry.register(Uuid::new_v4()).unwrap();
        let second = registry.register(Uuid::new_v4()).unwrap();
        std::thread::scope(|scope| {
            scope.spawn(|| registry.update(first.operation, |s| s.runtime_entered = true));
            scope.spawn(|| registry.update(Uuid::new_v4(), |s| s.runtime_entered = true));
        });
        assert!(first.snapshot().runtime_entered);
        assert_eq!(second.snapshot(), Snapshot::default());
        assert_eq!(registry.0.lock().unwrap().len(), 2);
        let old_id = first.operation;
        drop(first);
        assert_eq!(
            registry.register(old_id).unwrap().snapshot(),
            Snapshot::default()
        );
    }
}
