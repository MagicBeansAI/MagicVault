//! Trusted standalone host. This is not a new dependency of Magician/core.
pub mod broker;
pub mod client;
pub mod human;
pub mod ipc;
pub mod installation;
pub mod launch_agent;
pub mod native;
pub mod storage;
pub use magicvault_protocol as protocol;
