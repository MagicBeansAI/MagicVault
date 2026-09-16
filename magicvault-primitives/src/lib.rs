//! Neutral helpers: no credential backend, Magician, or MagicRun dependency.
pub mod durable_io;
pub mod json_traversal;
pub mod local_ipc;
pub mod private_fs;
#[cfg(windows)]
pub mod windows;

/// Actual compiled JSON implementation bytes for host-owned source attestations.
/// Never replace with a frozen digest: upgrades must still change host identity.
pub const JSON_TRAVERSAL_SOURCE: &[u8] = include_bytes!("json_traversal.rs");
