//! One-time credential custody: a code is held for one bound authentication
//! attempt and then it is gone.
//!
//! The lifecycle the shared core arbitrates:
//!
//! ```text
//! register ──▶ Available ──reserve──▶ Reserved ──consume──▶ Consumed
//!                 ▲                      │
//!                 └────── release ───────┘   (proven pre-dispatch failure only)
//!   Available | Reserved ──deadline──▶ Expired
//!   Available | Reserved ──cancel────▶ Cancelled
//!   any ──register again──▶ the previous entry is superseded
//! ```
//!
//! What is deliberately *not* here: persistence (a restart loses the code and
//! the run fails closed to a fresh ask), a placeholder read (one-time material
//! is only reachable through [`SecretStore::reserve_one_time`]), and any
//! release path other than a caller-asserted [`PreDispatchFailure`] — rejection,
//! a timeout after submission, an uncertain delivery, or a lost worker must
//! consume, because no local ledger can prove an external service did not
//! accept the code.
//!
//! Every transition happens under the store's state lock and reads the store's
//! injected clock, so a queued attempt is re-checked against the deadline at
//! the moment it consumes, not at the moment it was planned.
//!
//! [`SecretStore::reserve_one_time`]: crate::store::SecretStore::reserve_one_time

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use uuid::Uuid;
use zeroize::Zeroizing;

/// The longest a code is retained after registration, whatever deadline the
/// caller asked for. Issuer validity is not knowable here; this bounds local
/// exposure, it does not extend validity.
pub const ONE_TIME_MAX_RETENTION_MS: i64 = 10 * 60 * 1000;

/// Trusted bindings the registering application attaches to a code.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OneTimeBinding {
    /// The authentication challenge this code answers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub challenge_id: Option<String>,
    /// The only destination a claim may name. `None` binds no destination.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub destination: Option<String>,
}

/// The exact operation claiming one use.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OneTimeClaim {
    /// The adapter operation, in the store's `tool:action` audit vocabulary.
    pub operation: String,
    /// Where the code will be submitted. Must equal the registered destination
    /// when one was bound.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub destination: Option<String>,
    /// The challenge this attempt answers. Must equal the registered challenge
    /// when both are known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub challenge_id: Option<String>,
}

impl OneTimeBinding {
    /// Whether `claim` is an exact match for what was registered. A bound
    /// destination must be named exactly; a challenge is compared only when
    /// both sides carry one.
    fn admits(&self, claim: &OneTimeClaim) -> bool {
        let destination_ok =
            self.destination.is_none() || self.destination == claim.destination;
        let challenge_ok = match (&self.challenge_id, &claim.challenge_id) {
            (Some(bound), Some(claimed)) => bound == claimed,
            _ => true,
        };
        destination_ok && challenge_ok
    }
}

/// The only reason a reservation returns to `Available`. Naming the type at
/// the call site is the point: an adapter that releases must be able to say
/// the code never left the process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreDispatchFailure {
    pub reason: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OneTimeState {
    Available,
    Reserved,
    Consumed,
    Expired,
    Cancelled,
}

impl OneTimeState {
    /// Whether the value is still held in memory.
    pub fn is_live(self) -> bool {
        matches!(self, Self::Available | Self::Reserved)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OneTimeTransition {
    Registered,
    Reserved,
    Consumed,
    Released,
    Expired,
    Cancelled,
    Superseded,
}

impl OneTimeTransition {
    pub fn audit_event(self) -> &'static str {
        match self {
            Self::Registered => "one_time_registered",
            Self::Reserved => "one_time_reserved",
            Self::Consumed => "one_time_consumed",
            Self::Released => "one_time_released",
            Self::Expired => "one_time_expired",
            Self::Cancelled => "one_time_cancelled",
            Self::Superseded => "one_time_superseded",
        }
    }
}

/// Value-free record of one transition. Safe to log and serialize. The
/// reservation id is a capability the reserving adapter keeps: only the
/// receipt `reserve` returns carries it, never a state read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OneTimeReceipt {
    pub scope_id: String,
    pub input_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub challenge_id: Option<String>,
    /// The destination the registration bound, if any: an adapter that is
    /// about to deliver to exactly that destination can treat the binding as
    /// the user's consent for it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub destination: Option<String>,
    /// The transition that produced `state` — for a state read, the last one.
    pub transition: OneTimeTransition,
    pub state: OneTimeState,
    /// The live reservation after this transition, if the entry is `Reserved`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reservation_id: Option<String>,
    /// The effective deadline after clamping.
    pub deadline_ms: i64,
    pub at_ms: i64,
}

/// One bound attempt's hold on the value. The value zeroizes on drop; the
/// receipt is what the caller keeps.
pub struct OneTimeReservation {
    pub receipt: OneTimeReceipt,
    pub value: Zeroizing<String>,
}

impl std::fmt::Debug for OneTimeReservation {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("OneTimeReservation")
            .field("receipt", &self.receipt)
            .field("value", &format_args!("<{} bytes>", self.value.len()))
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum OneTimeError {
    #[error("no one-time material is registered for this input")]
    NotFound,
    #[error("the one-time material expired")]
    Expired,
    #[error("the one-time material is reserved by another attempt")]
    AlreadyReserved,
    #[error("the one-time material was consumed")]
    Consumed,
    #[error("the one-time material was cancelled")]
    Cancelled,
    #[error("the one-time material was superseded by a newer registration")]
    Superseded,
    #[error("the claim does not match the registered binding")]
    BindingMismatch,
    #[error("the reservation is not held")]
    NotReserved,
    #[error("ephemeral secret storage is unavailable: {0}")]
    FeatureUnavailable(String),
}

struct OneTimeEntry {
    scope_id: String,
    input_id: String,
    binding: OneTimeBinding,
    deadline_ms: i64,
    generation: u64,
    state: OneTimeState,
    /// The transition that put the entry in `state`.
    last: OneTimeTransition,
    /// `Some` exactly while `state.is_live()`.
    value: Option<Zeroizing<String>>,
    reservation_id: Option<String>,
    claim: Option<OneTimeClaim>,
}

impl OneTimeEntry {
    /// Record `transition` as the one that produced the current state and
    /// describe it. Every state write goes through here.
    fn transitioned(&mut self, transition: OneTimeTransition, at_ms: i64) -> OneTimeReceipt {
        self.last = transition;
        self.receipt(transition, at_ms)
    }

    fn receipt(&self, transition: OneTimeTransition, at_ms: i64) -> OneTimeReceipt {
        OneTimeReceipt {
            scope_id: self.scope_id.clone(),
            input_id: self.input_id.clone(),
            challenge_id: self.binding.challenge_id.clone(),
            destination: self.binding.destination.clone(),
            transition,
            state: self.state,
            reservation_id: self.reservation_id.clone(),
            deadline_ms: self.deadline_ms,
            at_ms,
        }
    }

    fn terminal_error(&self) -> Option<OneTimeError> {
        match self.state {
            OneTimeState::Available | OneTimeState::Reserved => None,
            OneTimeState::Consumed => Some(OneTimeError::Consumed),
            OneTimeState::Expired => Some(OneTimeError::Expired),
            OneTimeState::Cancelled => Some(OneTimeError::Cancelled),
        }
    }

    /// Apply the deadline. Returns the expiry line when this call is the one
    /// that expired the entry, so the caller journals it exactly once.
    fn expire_if_due(&mut self, now_ms: i64) -> Option<JournalLine> {
        if !self.state.is_live() || now_ms < self.deadline_ms {
            return None;
        }
        self.state = OneTimeState::Expired;
        self.value = None;
        self.reservation_id = None;
        let claim = self.claim.take();
        Some(line(self.transitioned(OneTimeTransition::Expired, now_ms), claim.as_ref()))
    }
}

/// Every entry lives inside the store's state and is only touched under its
/// write lock. A reservation id resolves to `(key, generation)` so an id
/// issued against a superseded registration answers `Superseded` instead of
/// acting on the newer code.
///
/// Terminal entries and their reservation ids are kept (value-free) so a late
/// caller gets the right refusal, and are dropped by [`Self::retire_scope`]
/// when the run ends — the same lifetime as plain ephemeral entries.
#[derive(Default)]
pub(crate) struct OneTimeTable {
    entries: HashMap<String, OneTimeEntry>,
    reservations: HashMap<String, (String, u64)>,
    next_generation: u64,
}

/// What a transition returns to the store: the lines to journal — the
/// requested transition plus any expiry the same call surfaced first — and
/// the outcome. Journal lines are produced even when the outcome is an error,
/// because the error may be the expiry the call discovered.
pub(crate) struct Transitioned<T> {
    pub outcome: Result<T, OneTimeError>,
    pub journal: Vec<JournalLine>,
}

/// One value-free audit line: the receipt and the claim that held the entry
/// at that moment, so `tool`/`action`/`domain` can be filled without the value.
pub(crate) struct JournalLine {
    pub receipt: OneTimeReceipt,
    pub claim: Option<OneTimeClaim>,
}

fn line(receipt: OneTimeReceipt, claim: Option<&OneTimeClaim>) -> JournalLine {
    JournalLine { receipt, claim: claim.cloned() }
}

fn key(scope_id: &str, input_id: &str) -> String {
    format!("{scope_id}::{input_id}")
}

impl OneTimeTable {
    pub(crate) fn register(
        &mut self,
        scope_id: &str,
        input_id: &str,
        value: String,
        deadline_ms: i64,
        binding: OneTimeBinding,
        now_ms: i64,
    ) -> Transitioned<OneTimeReceipt> {
        let mut journal = Vec::new();
        // A registration attempt always retires its predecessor, even when it
        // is itself refused: a new challenge invalidates the previous code.
        let key = key(scope_id, input_id);
        if let Some(previous) = self.entries.remove(&key) {
            if previous.state.is_live() {
                let mut superseded = previous;
                superseded.state = OneTimeState::Cancelled;
                superseded.value = None;
                superseded.reservation_id = None;
                let claim = superseded.claim.take();
                journal.push(line(
                    superseded.transitioned(OneTimeTransition::Superseded, now_ms),
                    claim.as_ref(),
                ));
            }
            // A reservation id issued against the removed generation now
            // answers `Superseded`; the index entry is kept so it can.
        }
        if deadline_ms <= now_ms {
            drop(Zeroizing::new(value));
            return Transitioned { outcome: Err(OneTimeError::Expired), journal };
        }
        self.next_generation += 1;
        let entry = OneTimeEntry {
            scope_id: scope_id.to_string(),
            input_id: input_id.to_string(),
            binding,
            deadline_ms: deadline_ms.min(now_ms + ONE_TIME_MAX_RETENTION_MS),
            generation: self.next_generation,
            state: OneTimeState::Available,
            last: OneTimeTransition::Registered,
            value: Some(Zeroizing::new(value)),
            reservation_id: None,
            claim: None,
        };
        let receipt = entry.receipt(OneTimeTransition::Registered, now_ms);
        self.entries.insert(key, entry);
        journal.push(line(receipt.clone(), None));
        Transitioned { outcome: Ok(receipt), journal }
    }

    pub(crate) fn reserve(
        &mut self,
        scope_id: &str,
        input_id: &str,
        claim: OneTimeClaim,
        now_ms: i64,
    ) -> Transitioned<OneTimeReservation> {
        let mut journal = Vec::new();
        let Some(entry) = self.entries.get_mut(&key(scope_id, input_id)) else {
            return Transitioned { outcome: Err(OneTimeError::NotFound), journal };
        };
        journal.extend(entry.expire_if_due(now_ms));
        if let Some(error) = entry.terminal_error() {
            return Transitioned { outcome: Err(error), journal };
        }
        if entry.state == OneTimeState::Reserved {
            return Transitioned { outcome: Err(OneTimeError::AlreadyReserved), journal };
        }
        if !entry.binding.admits(&claim) {
            return Transitioned { outcome: Err(OneTimeError::BindingMismatch), journal };
        }
        let Some(value) = entry.value.as_ref().map(|value| Zeroizing::new(value.to_string())) else {
            return Transitioned { outcome: Err(OneTimeError::NotFound), journal };
        };
        let reservation_id = Uuid::new_v4().simple().to_string();
        entry.state = OneTimeState::Reserved;
        entry.reservation_id = Some(reservation_id.clone());
        entry.claim = Some(claim);
        let receipt = entry.transitioned(OneTimeTransition::Reserved, now_ms);
        journal.push(line(receipt.clone(), entry.claim.as_ref()));
        self.reservations
            .insert(reservation_id, (key(scope_id, input_id), entry.generation));
        Transitioned { outcome: Ok(OneTimeReservation { receipt, value }), journal }
    }

    fn reserved_entry(&mut self, reservation_id: &str) -> Result<&mut OneTimeEntry, OneTimeError> {
        let Some((key, generation)) = self.reservations.get(reservation_id).cloned() else {
            return Err(OneTimeError::NotReserved);
        };
        let Some(entry) = self.entries.get_mut(&key) else {
            return Err(OneTimeError::NotFound);
        };
        if entry.generation != generation {
            return Err(OneTimeError::Superseded);
        }
        Ok(entry)
    }

    pub(crate) fn consume(&mut self, reservation_id: &str, now_ms: i64) -> Transitioned<OneTimeReceipt> {
        let mut journal = Vec::new();
        let entry = match self.reserved_entry(reservation_id) {
            Ok(entry) => entry,
            Err(error) => return Transitioned { outcome: Err(error), journal },
        };
        journal.extend(entry.expire_if_due(now_ms));
        if let Some(error) = entry.terminal_error() {
            return Transitioned { outcome: Err(error), journal };
        }
        if entry.reservation_id.as_deref() != Some(reservation_id) {
            return Transitioned { outcome: Err(OneTimeError::NotReserved), journal };
        }
        entry.state = OneTimeState::Consumed;
        entry.value = None;
        entry.reservation_id = None;
        let claim = entry.claim.take();
        let receipt = entry.transitioned(OneTimeTransition::Consumed, now_ms);
        journal.push(line(receipt.clone(), claim.as_ref()));
        Transitioned { outcome: Ok(receipt), journal }
    }

    pub(crate) fn release(&mut self, reservation_id: &str, now_ms: i64) -> Transitioned<OneTimeReceipt> {
        let mut journal = Vec::new();
        let entry = match self.reserved_entry(reservation_id) {
            Ok(entry) => entry,
            Err(error) => return Transitioned { outcome: Err(error), journal },
        };
        journal.extend(entry.expire_if_due(now_ms));
        if let Some(error) = entry.terminal_error() {
            return Transitioned { outcome: Err(error), journal };
        }
        if entry.reservation_id.as_deref() != Some(reservation_id) {
            return Transitioned { outcome: Err(OneTimeError::NotReserved), journal };
        }
        entry.state = OneTimeState::Available;
        entry.reservation_id = None;
        let claim = entry.claim.take();
        let receipt = entry.transitioned(OneTimeTransition::Released, now_ms);
        journal.push(line(receipt.clone(), claim.as_ref()));
        self.reservations.remove(reservation_id);
        Transitioned { outcome: Ok(receipt), journal }
    }

    pub(crate) fn cancel(&mut self, scope_id: &str, input_id: &str, now_ms: i64) -> Transitioned<OneTimeReceipt> {
        let mut journal = Vec::new();
        let Some(entry) = self.entries.get_mut(&key(scope_id, input_id)) else {
            return Transitioned { outcome: Err(OneTimeError::NotFound), journal };
        };
        journal.extend(entry.expire_if_due(now_ms));
        if let Some(error) = entry.terminal_error() {
            return Transitioned { outcome: Err(error), journal };
        }
        entry.state = OneTimeState::Cancelled;
        entry.value = None;
        entry.reservation_id = None;
        let claim = entry.claim.take();
        let receipt = entry.transitioned(OneTimeTransition::Cancelled, now_ms);
        journal.push(line(receipt.clone(), claim.as_ref()));
        Transitioned { outcome: Ok(receipt), journal }
    }

    /// Value-free state, applying the deadline first.
    pub(crate) fn state(&mut self, scope_id: &str, input_id: &str, now_ms: i64) -> (Option<OneTimeReceipt>, Vec<JournalLine>) {
        let Some(entry) = self.entries.get_mut(&key(scope_id, input_id)) else {
            return (None, Vec::new());
        };
        let journal: Vec<_> = entry.expire_if_due(now_ms).into_iter().collect();
        let mut receipt = entry.receipt(entry.last, now_ms);
        receipt.reservation_id = None;
        (Some(receipt), journal)
    }

    /// Expire every due entry now rather than on its next touch.
    pub(crate) fn sweep(&mut self, now_ms: i64) -> Vec<JournalLine> {
        self.entries
            .values_mut()
            .filter_map(|entry| entry.expire_if_due(now_ms))
            .collect()
    }

    /// Drop every entry of a scope, whatever its state. Returns how many held
    /// a value.
    pub(crate) fn retire_scope(&mut self, scope_id: &str) -> usize {
        let mut live = 0;
        self.entries.retain(|_, entry| {
            if entry.scope_id != scope_id {
                return true;
            }
            if entry.state.is_live() {
                live += 1;
            }
            false
        });
        self.reservations
            .retain(|_, (key, _)| self.entries.contains_key(key));
        live
    }

    /// Whether the scope holds a code that is live *now*: an entry nobody has
    /// touched since its deadline is expired by the spec, not by the next call.
    pub(crate) fn scope_holds_live(&self, scope_id: &str, now_ms: i64) -> bool {
        self.entries.values().any(|entry| {
            entry.scope_id == scope_id && entry.state.is_live() && now_ms < entry.deadline_ms
        })
    }

    pub(crate) fn live_values(&self) -> impl Iterator<Item = &str> {
        self.entries
            .values()
            .filter_map(|entry| entry.value.as_deref().map(String::as_str))
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::{Arc, Barrier};
    use std::thread;

    use super::*;
    use crate::store::{ManualClock, SecretAuditEvent, SecretRuntimeCapabilities, SecretStore, SecretStoreResolver, SecretScopeLayout, SECRET_AUDIT_FILENAME};
    use crate::InMemoryKeyProvider;

    const CANARY: &str = "p2-one-time-canary-041771";
    const T0: i64 = 1_700_000_000_000;

    #[derive(Clone)]
    struct FixtureScopeLayout(PathBuf);
    impl SecretScopeLayout for FixtureScopeLayout {
        fn from_base_root(base_root: &std::path::Path) -> Self {
            Self(base_root.to_path_buf())
        }
        fn secrets_root(&self, principal: &str, workspace: &str) -> PathBuf {
            self.0.join("scopes").join(principal).join(workspace).join("secrets")
        }
    }

    fn resolver(root: &std::path::Path, clock: &Arc<ManualClock>) -> SecretStoreResolver<FixtureScopeLayout> {
        SecretStoreResolver::new_with_capabilities(
            Box::new(InMemoryKeyProvider::new()),
            root.to_path_buf(),
            SecretRuntimeCapabilities::fully_available("in_memory"),
        )
        .with_clock(clock.clone())
    }

    fn store(root: &std::path::Path, clock: &Arc<ManualClock>) -> Arc<SecretStore> {
        resolver(root, clock).resolve_for_scope("owner", "default").unwrap()
    }

    fn binding() -> OneTimeBinding {
        OneTimeBinding {
            challenge_id: Some("challenge-1".into()),
            destination: Some("https://login.example.test".into()),
        }
    }

    fn claim() -> OneTimeClaim {
        OneTimeClaim {
            operation: "browser:submit_login".into(),
            destination: Some("https://login.example.test".into()),
            challenge_id: Some("challenge-1".into()),
        }
    }

    fn register(store: &SecretStore) -> OneTimeReceipt {
        store
            .register_one_time("execution:run-1", "otp", CANARY.to_string(), T0 + 60_000, binding())
            .unwrap()
    }

    fn journal(root: &std::path::Path) -> Vec<SecretAuditEvent> {
        let path = root.join("scopes/owner/default/secrets").join(SECRET_AUDIT_FILENAME);
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        assert!(!text.contains(CANARY), "the audit journal must never carry the value");
        text.lines().map(|line| serde_json::from_str(line).unwrap()).collect()
    }

    #[test]
    fn a_registered_code_is_reserved_once_and_consumed_once() {
        let temp = tempfile::tempdir().unwrap();
        let clock = Arc::new(ManualClock::new(T0));
        let store = store(temp.path(), &clock);

        let registered = register(&store);
        assert_eq!(registered.state, OneTimeState::Available);
        assert_eq!(registered.transition, OneTimeTransition::Registered);
        assert_eq!(registered.deadline_ms, T0 + 60_000);
        assert_eq!(registered.challenge_id.as_deref(), Some("challenge-1"));

        let reservation = store.reserve_one_time("execution:run-1", "otp", claim()).unwrap();
        assert_eq!(reservation.value.as_str(), CANARY, "the bound attempt receives the exact string");
        assert_eq!(reservation.receipt.state, OneTimeState::Reserved);
        let reservation_id = reservation.receipt.reservation_id.clone().expect("a reservation id");
        assert!(!format!("{reservation:?}").contains(CANARY), "Debug never prints the value");

        assert_eq!(
            store.reserve_one_time("execution:run-1", "otp", claim()).unwrap_err(),
            OneTimeError::AlreadyReserved,
            "a second attempt cannot claim a reserved code"
        );

        let consumed = store.consume_one_time(&reservation_id).unwrap();
        assert_eq!(consumed.state, OneTimeState::Consumed);
        assert_eq!(consumed.transition, OneTimeTransition::Consumed);

        assert_eq!(store.consume_one_time(&reservation_id).unwrap_err(), OneTimeError::Consumed);
        assert_eq!(
            store.release_one_time(&reservation_id, PreDispatchFailure { reason: "late".into() }).unwrap_err(),
            OneTimeError::Consumed,
            "an ambiguous outcome after submission can never put the code back"
        );
        assert_eq!(
            store.reserve_one_time("execution:run-1", "otp", claim()).unwrap_err(),
            OneTimeError::Consumed
        );
        let state = store.one_time_state("execution:run-1", "otp").unwrap();
        assert_eq!(state.state, OneTimeState::Consumed);
        assert!(state.reservation_id.is_none(), "a receipt after consumption names no live reservation");

        let events: Vec<String> = journal(temp.path()).into_iter().map(|event| event.event).collect();
        assert_eq!(events, ["one_time_registered", "one_time_reserved", "one_time_consumed"]);
    }

    #[test]
    fn a_proven_pre_dispatch_failure_releases_the_reservation_for_one_more_attempt() {
        let temp = tempfile::tempdir().unwrap();
        let clock = Arc::new(ManualClock::new(T0));
        let store = store(temp.path(), &clock);
        register(&store);

        let first = store.reserve_one_time("execution:run-1", "otp", claim()).unwrap();
        let first_id = first.receipt.reservation_id.clone().unwrap();
        let released = store
            .release_one_time(&first_id, PreDispatchFailure { reason: "field not found".into() })
            .unwrap();
        assert_eq!(released.state, OneTimeState::Available);
        assert_eq!(released.transition, OneTimeTransition::Released);
        let read_back = store.one_time_state("execution:run-1", "otp").unwrap();
        assert_eq!((read_back.state, read_back.transition), (OneTimeState::Available, OneTimeTransition::Released));

        assert_eq!(
            store.consume_one_time(&first_id).unwrap_err(),
            OneTimeError::NotReserved,
            "a released reservation id cannot consume later"
        );

        let second = store.reserve_one_time("execution:run-1", "otp", claim()).unwrap();
        assert_eq!(second.value.as_str(), CANARY);
        assert_ne!(second.receipt.reservation_id, Some(first_id));
        let second_id = second.receipt.reservation_id.clone().unwrap();
        store.consume_one_time(&second_id).unwrap();

        let events = journal(temp.path());
        let released = events.iter().find(|event| event.event == "one_time_released").unwrap();
        assert_eq!(released.detail.as_deref(), Some("execution:run-1: field not found"));
        assert_eq!(released.tool.as_deref(), Some("browser"));
        assert_eq!(released.action.as_deref(), Some("submit_login"));
        assert_eq!(released.domain.as_deref(), Some("https://login.example.test"));
        assert_eq!(released.challenge_id.as_deref(), Some("challenge-1"));
    }

    #[test]
    fn a_code_expires_while_available_and_while_reserved() {
        let temp = tempfile::tempdir().unwrap();
        let clock = Arc::new(ManualClock::new(T0));
        let store = store(temp.path(), &clock);

        register(&store);
        clock.set(T0 + 60_000);
        assert_eq!(
            store.reserve_one_time("execution:run-1", "otp", claim()).unwrap_err(),
            OneTimeError::Expired,
            "at the deadline the code is unusable"
        );
        let state = store.one_time_state("execution:run-1", "otp").unwrap();
        assert_eq!(state.state, OneTimeState::Expired);

        clock.set(T0);
        store
            .register_one_time("execution:run-1", "otp2", CANARY.to_string(), T0 + 30_000, binding())
            .unwrap();
        let reservation = store.reserve_one_time("execution:run-1", "otp2", claim()).unwrap();
        let id = reservation.receipt.reservation_id.clone().unwrap();
        clock.set(T0 + 30_000);
        assert_eq!(
            store.consume_one_time(&id).unwrap_err(),
            OneTimeError::Expired,
            "the pre-dispatch recheck refuses a code that expired while queued"
        );
        assert_eq!(
            store.release_one_time(&id, PreDispatchFailure { reason: "queue".into() }).unwrap_err(),
            OneTimeError::Expired
        );
        assert_eq!(store.one_time_state("execution:run-1", "otp2").unwrap().state, OneTimeState::Expired);

        let events: Vec<String> = journal(temp.path()).into_iter().map(|event| event.event).collect();
        assert_eq!(events.iter().filter(|event| *event == "one_time_expired").count(), 2);
    }

    #[test]
    fn the_retention_cap_bounds_a_deadline_the_caller_did_not_earn() {
        let temp = tempfile::tempdir().unwrap();
        let clock = Arc::new(ManualClock::new(T0));
        let store = store(temp.path(), &clock);
        let receipt = store
            .register_one_time("execution:run-1", "otp", CANARY.to_string(), T0 + 24 * 60 * 60 * 1000, binding())
            .unwrap();
        assert_eq!(receipt.deadline_ms, T0 + ONE_TIME_MAX_RETENTION_MS);
        assert_eq!(
            store
                .register_one_time("execution:run-1", "late", CANARY.to_string(), T0 - 1, binding())
                .unwrap_err(),
            OneTimeError::Expired,
            "a deadline already in the past registers nothing"
        );
        assert!(store.one_time_state("execution:run-1", "late").is_none());
    }

    #[test]
    fn a_claim_for_another_destination_is_refused_without_changing_state() {
        let temp = tempfile::tempdir().unwrap();
        let clock = Arc::new(ManualClock::new(T0));
        let store = store(temp.path(), &clock);
        register(&store);

        let elsewhere = OneTimeClaim {
            destination: Some("https://attacker.example.test".into()),
            ..claim()
        };
        assert_eq!(
            store.reserve_one_time("execution:run-1", "otp", elsewhere).unwrap_err(),
            OneTimeError::BindingMismatch
        );
        let unclaimed = OneTimeClaim { operation: "browser:submit_login".into(), destination: None, challenge_id: None };
        assert_eq!(
            store.reserve_one_time("execution:run-1", "otp", unclaimed).unwrap_err(),
            OneTimeError::BindingMismatch,
            "a registration bound to a destination is not claimable without one"
        );
        let other_challenge = OneTimeClaim { challenge_id: Some("challenge-2".into()), ..claim() };
        assert_eq!(
            store.reserve_one_time("execution:run-1", "otp", other_challenge).unwrap_err(),
            OneTimeError::BindingMismatch,
            "a code answers the challenge it was registered for"
        );
        assert_eq!(store.one_time_state("execution:run-1", "otp").unwrap().state, OneTimeState::Available);
        let unnamed_challenge = OneTimeClaim { challenge_id: None, ..claim() };
        assert!(
            store.reserve_one_time("execution:run-1", "otp", unnamed_challenge).is_ok(),
            "a claim that names no challenge is admitted; the challenge is compared only when both sides carry one"
        );

        let events: Vec<String> = journal(temp.path()).into_iter().map(|event| event.event).collect();
        assert_eq!(events, ["one_time_registered", "one_time_reserved"], "a refused claim is not a transition");
    }

    #[test]
    fn a_new_registration_supersedes_the_previous_code_and_its_reservation() {
        let temp = tempfile::tempdir().unwrap();
        let clock = Arc::new(ManualClock::new(T0));
        let store = store(temp.path(), &clock);
        register(&store);
        let stale = store.reserve_one_time("execution:run-1", "otp", claim()).unwrap();
        let stale_id = stale.receipt.reservation_id.clone().unwrap();

        let fresh = store
            .register_one_time("execution:run-1", "otp", "fresh-code-2".to_string(), T0 + 60_000, binding())
            .unwrap();
        assert_eq!(fresh.state, OneTimeState::Available);
        assert_eq!(store.consume_one_time(&stale_id).unwrap_err(), OneTimeError::Superseded);
        assert_eq!(
            store.release_one_time(&stale_id, PreDispatchFailure { reason: "x".into() }).unwrap_err(),
            OneTimeError::Superseded
        );
        let reservation = store.reserve_one_time("execution:run-1", "otp", claim()).unwrap();
        assert_eq!(reservation.value.as_str(), "fresh-code-2");

        let events: Vec<String> = journal(temp.path()).into_iter().map(|event| event.event).collect();
        assert!(events.contains(&"one_time_superseded".to_string()));
    }

    #[test]
    fn a_refused_registration_still_retires_the_previous_code() {
        let temp = tempfile::tempdir().unwrap();
        let clock = Arc::new(ManualClock::new(T0));
        let store = store(temp.path(), &clock);
        register(&store);
        assert_eq!(
            store
                .register_one_time("execution:run-1", "otp", "stale-challenge".to_string(), T0 - 1, binding())
                .unwrap_err(),
            OneTimeError::Expired
        );
        assert!(
            store.one_time_state("execution:run-1", "otp").is_none(),
            "a new challenge invalidates the previous code even when its own deadline has passed"
        );
        assert_eq!(
            store.reserve_one_time("execution:run-1", "otp", claim()).unwrap_err(),
            OneTimeError::NotFound
        );
    }

    #[test]
    fn a_state_read_never_hands_out_the_reservation() {
        let temp = tempfile::tempdir().unwrap();
        let clock = Arc::new(ManualClock::new(T0));
        let store = store(temp.path(), &clock);
        register(&store);
        let reservation = store.reserve_one_time("execution:run-1", "otp", claim()).unwrap();
        assert!(reservation.receipt.reservation_id.is_some());
        let read = store.one_time_state("execution:run-1", "otp").unwrap();
        assert_eq!(read.state, OneTimeState::Reserved);
        assert!(read.reservation_id.is_none(), "the id is a capability only the reserving attempt holds");
    }

    #[test]
    fn cancellation_revokes_an_unspent_code_and_cannot_undo_a_consumed_one() {
        let temp = tempfile::tempdir().unwrap();
        let clock = Arc::new(ManualClock::new(T0));
        let store = store(temp.path(), &clock);

        register(&store);
        let cancelled = store.cancel_one_time("execution:run-1", "otp").unwrap();
        assert_eq!(cancelled.state, OneTimeState::Cancelled);
        assert_eq!(
            store.reserve_one_time("execution:run-1", "otp", claim()).unwrap_err(),
            OneTimeError::Cancelled
        );

        store
            .register_one_time("execution:run-1", "reserved", CANARY.to_string(), T0 + 60_000, binding())
            .unwrap();
        let reservation = store.reserve_one_time("execution:run-1", "reserved", claim()).unwrap();
        let id = reservation.receipt.reservation_id.clone().unwrap();
        assert_eq!(store.cancel_one_time("execution:run-1", "reserved").unwrap().state, OneTimeState::Cancelled);
        assert_eq!(store.consume_one_time(&id).unwrap_err(), OneTimeError::Cancelled);

        store
            .register_one_time("execution:run-1", "spent", CANARY.to_string(), T0 + 60_000, binding())
            .unwrap();
        let spent = store.reserve_one_time("execution:run-1", "spent", claim()).unwrap();
        store.consume_one_time(spent.receipt.reservation_id.as_deref().unwrap()).unwrap();
        assert_eq!(
            store.cancel_one_time("execution:run-1", "spent").unwrap_err(),
            OneTimeError::Consumed,
            "cancellation after submission cannot undo the external effect"
        );
        assert_eq!(store.cancel_one_time("execution:run-1", "missing").unwrap_err(), OneTimeError::NotFound);
    }

    #[test]
    fn the_sweep_drops_expired_material_and_reports_each_expiry_once() {
        let temp = tempfile::tempdir().unwrap();
        let clock = Arc::new(ManualClock::new(T0));
        let store = store(temp.path(), &clock);
        store
            .register_one_time("execution:run-1", "a", CANARY.to_string(), T0 + 1_000, binding())
            .unwrap();
        store
            .register_one_time("execution:run-1", "b", CANARY.to_string(), T0 + 5_000, binding())
            .unwrap();
        assert_eq!(store.sweep_expired_one_time(), 0);
        clock.set(T0 + 1_000);
        assert_eq!(store.sweep_expired_one_time(), 1);
        assert_eq!(store.sweep_expired_one_time(), 0, "an expiry is reported once");
        assert_eq!(store.one_time_state("execution:run-1", "a").unwrap().state, OneTimeState::Expired);
        assert_eq!(store.one_time_state("execution:run-1", "b").unwrap().state, OneTimeState::Available);
        assert_eq!(
            store.redaction_values().iter().filter(|value| value.as_str() == CANARY).count(),
            1,
            "the expired value is dropped, the live one is still redacted"
        );
    }

    #[test]
    fn one_time_material_is_scoped_and_retired_with_the_scope() {
        let temp = tempfile::tempdir().unwrap();
        let clock = Arc::new(ManualClock::new(T0));
        let store = store(temp.path(), &clock);
        register(&store);

        assert_eq!(
            store.reserve_one_time("execution:run-2", "otp", claim()).unwrap_err(),
            OneTimeError::NotFound,
            "another run cannot claim this run's code"
        );
        assert!(store.ephemeral_scope_holds_user_typed_secret("execution:run-1"));
        assert!(!store.ephemeral_scope_holds_user_typed_secret("execution:run-2"));
        assert!(
            store.redaction_values().iter().any(|value| value.as_str() == CANARY),
            "a held code is redacted wherever the redaction pass runs"
        );
        assert_eq!(
            store.get_ephemeral_scoped("execution:run-1", "otp"),
            None,
            "one-time material is never a plain placeholder read"
        );

        assert_eq!(store.clear_ephemeral("execution:run-1"), 1);
        assert!(store.one_time_state("execution:run-1", "otp").is_none());
        assert!(!store.ephemeral_scope_holds_user_typed_secret("execution:run-1"));
        assert!(!store.redaction_values().iter().any(|value| value.as_str() == CANARY));
    }

    #[test]
    fn a_spent_code_no_longer_pins_the_run() {
        let temp = tempfile::tempdir().unwrap();
        let clock = Arc::new(ManualClock::new(T0));
        let store = store(temp.path(), &clock);
        register(&store);
        let reservation = store.reserve_one_time("execution:run-1", "otp", claim()).unwrap();
        assert!(store.ephemeral_scope_holds_user_typed_secret("execution:run-1"), "reserved material still pins");
        store.consume_one_time(reservation.receipt.reservation_id.as_deref().unwrap()).unwrap();
        assert!(!store.ephemeral_scope_holds_user_typed_secret("execution:run-1"));

        store
            .register_one_time("execution:run-2", "otp", CANARY.to_string(), T0 + 1_000, binding())
            .unwrap();
        assert!(store.ephemeral_scope_holds_user_typed_secret("execution:run-2"));
        clock.set(T0 + 1_000);
        assert!(
            !store.ephemeral_scope_holds_user_typed_secret("execution:run-2"),
            "an untouched code past its deadline is expired, not merely not-yet-noticed"
        );

        clock.set(T0);
        store
            .register_one_time("execution:run-3", "otp", CANARY.to_string(), T0 + 60_000, binding())
            .unwrap();
        store.cancel_one_time("execution:run-3", "otp").unwrap();
        assert!(!store.ephemeral_scope_holds_user_typed_secret("execution:run-3"));
    }

    #[test]
    fn concurrent_attempts_reserve_exactly_once() {
        let temp = tempfile::tempdir().unwrap();
        let clock = Arc::new(ManualClock::new(T0));
        let store = store(temp.path(), &clock);
        register(&store);

        let racers = 16;
        let barrier = Arc::new(Barrier::new(racers));
        let handles: Vec<_> = (0..racers)
            .map(|_| {
                let store = Arc::clone(&store);
                let barrier = Arc::clone(&barrier);
                thread::spawn(move || {
                    barrier.wait();
                    store.reserve_one_time("execution:run-1", "otp", claim()).map(|_| ())
                })
            })
            .collect();
        let outcomes: Vec<_> = handles.into_iter().map(|handle| handle.join().unwrap()).collect();
        assert_eq!(outcomes.iter().filter(|outcome| outcome.is_ok()).count(), 1);
        assert_eq!(
            outcomes.iter().filter(|outcome| **outcome == Err(OneTimeError::AlreadyReserved)).count(),
            racers - 1,
            "every loser saw the winner's reservation, not a missing entry"
        );
    }

    #[test]
    fn a_restart_loses_the_code_and_fails_closed() {
        let temp = tempfile::tempdir().unwrap();
        let clock = Arc::new(ManualClock::new(T0));
        {
            let store = store(temp.path(), &clock);
            register(&store);
            store.reserve_one_time("execution:run-1", "otp", claim()).unwrap();
        }
        let reopened = store(temp.path(), &clock);
        assert_eq!(
            reopened.reserve_one_time("execution:run-1", "otp", claim()).unwrap_err(),
            OneTimeError::NotFound
        );
        assert!(reopened.one_time_state("execution:run-1", "otp").is_none());
        for entry in std::fs::read_dir(temp.path().join("scopes/owner/default/secrets")).unwrap() {
            let path = entry.unwrap().path();
            let bytes = std::fs::read(&path).unwrap();
            assert!(!String::from_utf8_lossy(&bytes).contains(CANARY), "{} carries the value", path.display());
        }
    }

    #[test]
    fn a_failed_journal_append_still_leaves_spent_material_spent() {
        let temp = tempfile::tempdir().unwrap();
        let clock = Arc::new(ManualClock::new(T0));
        let store = store(temp.path(), &clock);
        register(&store);
        let reservation = store.reserve_one_time("execution:run-1", "otp", claim()).unwrap();
        let id = reservation.receipt.reservation_id.clone().unwrap();

        // Make the journal unwritable: a directory where the file should be.
        let journal_path = temp.path().join("scopes/owner/default/secrets").join(SECRET_AUDIT_FILENAME);
        std::fs::remove_file(&journal_path).unwrap();
        std::fs::create_dir(&journal_path).unwrap();
        assert!(
            store.try_audit_event(SecretAuditEvent::new_at("probe", 0)).is_err(),
            "the sabotage must actually break the journal"
        );

        let consumed = store.consume_one_time(&id).unwrap();
        assert_eq!(consumed.state, OneTimeState::Consumed);
        assert_eq!(store.consume_one_time(&id).unwrap_err(), OneTimeError::Consumed);
        assert_eq!(
            store.reserve_one_time("execution:run-1", "otp", claim()).unwrap_err(),
            OneTimeError::Consumed
        );
    }

    #[test]
    fn one_time_custody_is_gated_on_the_ephemeral_feature() {
        let temp = tempfile::tempdir().unwrap();
        let mut capabilities = SecretRuntimeCapabilities::fully_available("in_memory");
        capabilities.ephemeral = crate::store::SecretFeatureStatus::disabled("test");
        let store = SecretStore::new_empty_with_capabilities(
            Arc::new(InMemoryKeyProvider::new()),
            temp.path().join("vault"),
            capabilities,
        );
        assert!(matches!(
            store.register_one_time("execution:run-1", "otp", CANARY.to_string(), T0 + 1, binding()),
            Err(OneTimeError::FeatureUnavailable(_))
        ));
    }
}
