//! Trusted execution, not custody or authorization. Callers must authenticate,
//! authorize the exact destination/fields, and obtain consent before `fill`.
//! Never expose this material-bearing API as a model tool or generic dispatcher.
//! Embedders must suppress websocket dependency payload logging for the full
//! connection lifetime. Shipped executables compile the `log` facade out; this
//! reusable crate intentionally does not change an embedding host's global logs.
pub mod bridge;
pub mod cdp;
use async_trait::async_trait;
use magicvault_protocol::{ErrorCode, FieldState, MAX_FIELDS};
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;
use zeroize::Zeroize;

/// Internal backend identity, never a caller-authored destination authority.
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Target {
    pub tab: String,
    pub frame: String,
    pub document: String,
    pub top_document: String,
    pub origin: String,
    pub top_origin: String,
    pub is_main_frame: bool,
}

impl Target {
    pub fn valid(&self) -> bool {
        [&self.tab, &self.frame, &self.document, &self.top_document]
            .iter()
            .all(|v| !v.is_empty() && v.len() <= 256 && !v.chars().any(char::is_control))
            && canonical_origin(&self.origin).as_deref() == Ok(self.origin.as_str())
            && canonical_origin(&self.top_origin).as_deref() == Ok(self.top_origin.as_str())
    }
}

/// Deliberately neither Clone nor Debug. Serialization is only for a trusted
/// browser transport, never the daemon's model-facing request/reply protocol.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MaterialField {
    pub css: String,
    pub value: String,
}
impl Drop for MaterialField {
    fn drop(&mut self) {
        self.value.zeroize();
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Outcome {
    pub fields: Vec<FieldState>,
    pub error: Option<ErrorCode>,
}
impl Outcome {
    pub fn failed(count: usize, error: ErrorCode) -> Self {
        Self {
            fields: vec![FieldState::NotFilled; count],
            error: Some(error),
        }
    }
    pub fn uncertain(count: usize) -> Self {
        Self {
            fields: vec![FieldState::Uncertain; count],
            error: Some(ErrorCode::TransportUncertain),
        }
    }
    pub fn valid(&self, count: usize) -> bool {
        count <= MAX_FIELDS
            && self.fields.len() == count
            && (self.error.is_some() || self.fields.iter().all(|s| *s == FieldState::Filled))
    }
}

#[async_trait]
pub trait BrowserAdapter: Send + Sync {
    async fn targets(&self, cancel: CancellationToken) -> Result<Vec<Target>, ErrorCode>;
    async fn fill(
        &self,
        target: &Target,
        fields: Vec<MaterialField>,
        cancel: CancellationToken,
    ) -> Outcome;
    /// Disconnect only this integration, never close the user's browser.
    fn disconnect(&self);
    fn connected(&self) -> bool;
}

/// Exact origin policy, not wildcard/domain suffix matching. HTTPS is required
/// except for explicitly selected loopback fixture/development sites.
pub fn canonical_origin(input: &str) -> Result<String, ErrorCode> {
    let url = url::Url::parse(input).map_err(|_| ErrorCode::InvalidRequest)?;
    if !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.path() != "/"
        || url.host_str().is_none()
        || !(url.scheme() == "https" || (url.scheme() == "http" && loopback_host(&url)))
    {
        return Err(ErrorCode::InvalidRequest);
    }
    if matches!(url.host(),Some(url::Host::Domain(name)) if !name.bytes().all(|b|b.is_ascii_alphanumeric()||b==b'.'||b==b'-'))
    {
        return Err(ErrorCode::InvalidRequest);
    }
    let origin = url.origin().ascii_serialization();
    if origin.len() > 256 {
        return Err(ErrorCode::InvalidRequest);
    }
    Ok(origin)
}

pub fn origin_from_url(input: &str) -> Result<String, ErrorCode> {
    let url = url::Url::parse(input).map_err(|_| ErrorCode::UnsupportedTarget)?;
    if !url.username().is_empty() || url.password().is_some() {
        return Err(ErrorCode::UnsupportedTarget);
    }
    canonical_origin(&url.origin().ascii_serialization()).map_err(|_| ErrorCode::UnsupportedTarget)
}

fn loopback_host(url: &url::Url) -> bool {
    match url.host() {
        Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
        Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
        // Website origin policy may name localhost; CDP transport separately
        // requires an IP literal and never resolves attacker-controlled DNS.
        Some(url::Host::Domain(name)) => name == "localhost",
        None => false,
    }
}

/// Shared fixed function, executed only in an isolated browser world. Its
/// result is a closed status tuple; no page text, values or exception details.
pub const FILL_FUNCTION: &str = include_str!("fill.js");
