//! Closed, failure-only fixture progress. Never records browser inputs/outputs.
use std::cell::Cell;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Phase {
    Preparation,
    FirstProfile,
    SecondProfile,
    VerifyFirstProfile,
    VerifySecondProfile,
    Configure,
    McpSetup,
    LargeTabFixture,
    ClearField,
    Discovery,
    NarrowedDiscovery,
    FillDispatch,
    FillSettlement,
    VerifyField,
    NavigationRace,
    BlockRace,
    BlockRequest,
    BlockVerify,
    UnblockRequest,
    ReturnToLogin,
    Pause,
    Resume,
    Idle,
    AuditAndCleanup,
}

pub(super) fn tool_label(name: &str) -> &'static str {
    match name {
        "list_browsers" => "list_browsers",
        "browser_targets" => "browser_targets",
        "secure_fill" => "secure_fill",
        "fill_status" => "fill_status",
        _ => "other",
    }
}

#[derive(Debug, PartialEq)]
pub(super) enum Health {
    Ready,
    NotReady,
    TimedOut,
}

// Only used after the original failure. No response bodies or errors escape.
pub(super) async fn observe(work: impl std::future::Future<Output = bool>) -> Health {
    match tokio::time::timeout(std::time::Duration::from_secs(2), work).await {
        Ok(true) => Health::Ready,
        Ok(false) => Health::NotReady,
        Err(_) => Health::TimedOut,
    }
}

pub(super) async fn finish_observed<T>(
    result: std::thread::Result<T>,
    observation: impl std::future::Future<Output = ()>,
) -> T {
    match result {
        Ok(value) => value,
        Err(failure) => {
            observation.await;
            std::panic::resume_unwind(failure)
        }
    }
}

#[tokio::test]
async fn successful_scenario_never_polls_failure_observations() {
    assert_eq!(
        finish_observed(Ok(7), async { panic!("must not observe success") }).await,
        7
    );
}

#[tokio::test]
async fn observations_cannot_rescue_or_replace_the_original_failure() {
    use futures_util::FutureExt;
    let observed = Cell::new(false);
    let original = Box::new(41_u32);
    let identity = &*original as *const u32;
    let result = std::panic::AssertUnwindSafe(finish_observed::<()>(Err(original), async {
        observed.set(true);
    }))
    .catch_unwind()
    .await;
    assert!(observed.get());
    let failure = result.expect_err("failure must remain a failure");
    assert_eq!(
        failure.downcast_ref::<u32>().unwrap() as *const u32,
        identity
    );
}

#[tokio::test]
async fn health_observation_has_closed_results_and_a_separate_bound() {
    assert_eq!(observe(async { true }).await, Health::Ready);
    assert_eq!(observe(async { false }).await, Health::NotReady);
    assert_eq!(observe(std::future::pending()).await, Health::TimedOut);
}

#[test]
fn tool_labels_never_echo_arbitrary_input() {
    for name in [
        "list_browsers",
        "browser_targets",
        "secure_fill",
        "fill_status",
    ] {
        assert_eq!(tool_label(name), name);
    }
    assert_eq!(tool_label("SYNTHETIC-PRIVATE-INPUT"), "other");
}

pub(super) struct Progress {
    phase: Cell<Phase>,
    iteration: Cell<Option<u8>>,
}

impl Progress {
    pub(super) fn new() -> Self {
        Self {
            phase: Cell::new(Phase::Preparation),
            iteration: Cell::new(None),
        }
    }
    pub(super) fn at(&self, phase: Phase) {
        self.phase.set(phase);
    }
    pub(super) fn iteration(&self, iteration: u8) {
        assert!(iteration <= 20);
        self.iteration.set(Some(iteration));
    }
    pub(super) fn end_iterations(&self) {
        self.iteration.set(None);
    }
}

impl Drop for Progress {
    fn drop(&mut self) {
        if std::thread::panicking() {
            eprintln!(
                "extension fixture failure phase: {:?}; iteration: {:?}",
                self.phase.get(),
                self.iteration.get()
            );
        }
    }
}

#[test]
fn progress_keeps_only_latest_closed_phase_and_bounded_iteration() {
    let progress = Progress::new();
    progress.at(Phase::ClearField);
    progress.iteration(20);
    assert_eq!(progress.phase.get(), Phase::ClearField);
    assert_eq!(progress.iteration.get(), Some(20));
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| progress.iteration(21))).is_err()
    );
    assert_eq!(progress.iteration.get(), Some(20));
    progress.end_iterations();
    progress.at(Phase::Pause);
    assert_eq!(progress.phase.get(), Phase::Pause);
    assert_eq!(progress.iteration.get(), None);
}
