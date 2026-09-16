//! Windows process delivery remains closed until MagicRun has a qualified
//! Windows governed-spawn backend. Browser and HTTP delivery are independent.
use crate::delivery::{DeliveryMaterial, DeliveryOutcome};
use magicvault_protocol::{ErrorCode, ProcessDestination};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;
pub fn inspect(_: &ProcessDestination) -> Result<[u8; 32], ErrorCode> {
    Err(ErrorCode::UnsupportedTarget)
}
pub async fn execute(
    _: ProcessDestination,
    _: [u8; 32],
    _: DeliveryMaterial,
    _: Uuid,
    _: CancellationToken,
) -> DeliveryOutcome {
    DeliveryOutcome::failed(ErrorCode::UnsupportedTarget)
}
