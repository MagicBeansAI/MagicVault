//! Credential custody for trusted, in-process integrations.
//!
//! This crate does not expose a model-facing material-read tool. Trusted callers
//! can receive plaintext and are responsible for policy, destination authority,
//! and preventing material from reaching their model-facing output channels.
pub mod encryption;
pub mod injection;
pub mod policy;
pub mod session;
pub mod store;
mod types;
pub use types::*;
pub use encryption::{InMemoryKeyProvider, MasterKeyProvider};
pub use injection::{CookieWithMetadata, SameSite};
pub use magicvault_primitives::json_traversal;
