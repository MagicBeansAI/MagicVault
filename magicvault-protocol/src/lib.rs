//! Reference-only agent operations. Pairing capabilities are transport secrets,
//! never vault material and never an MCP tool result. Human decisions cannot be
//! submitted through this protocol. No effect is advertised before it exists.
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use zeroize::Zeroize;
mod browser;
pub use browser::*;

// Closed enums require matching standalone executables; persisted vault and
// registry formats and embedded core APIs are independent of this wire version.
pub const VERSION: u32 = 2;
pub const MAX_FRAME_BYTES: usize = 32 * 1024;
pub const MAX_REPLY_BYTES: usize = 256 * 1024;
pub const MAX_FIELDS: usize = 8;
pub const MAX_CLIENTS: usize = 32;
pub const MAX_CREDENTIALS: usize = 256;
pub const MAX_PENDING: usize = 32;
pub const CONSENT_TTL_SECS: u64 = 180;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Envelope {
    pub version: u32,
    pub request_id: Uuid,
    pub epoch: Option<Uuid>,
    pub token: Option<String>,
    pub request: Request,
}
impl Drop for Envelope {
    fn drop(&mut self) {
        if let Some(token) = &mut self.token {
            token.zeroize();
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(
    tag = "method",
    content = "params",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum Request {
    Status,
    Pair(PairRequest),
    Enroll(EnrollRequest),
    ListCredentials,
    RequestAccess(AccessRequest),
    ApprovalStatus(ApprovalQuery),
    RevokeClient(RevokeRequest),
    Shutdown,
    RegisterCdp(RegisterCdp),
    ListBrowsers,
    BrowserTargets(BrowserQuery),
    DisconnectBrowser(BrowserQuery),
    ConfigureBrowserCredential(BrowserRule),
    SecureFill(SecureFill),
    FillStatus(FillQuery),
    CancelFill(FillQuery),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PairRequest {
    pub label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnrollRequest {
    pub label: String,
    pub field_names: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AccessRequest {
    pub credential_ref: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApprovalQuery {
    pub approval_id: Uuid,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RevokeRequest {
    pub client_id: Uuid,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    InvalidRequest,
    UnsupportedVersion,
    Unauthorized,
    StaleSession,
    Busy,
    Denied,
    Unavailable,
    NotFound,
    Conflict,
    Capacity,
    Expired,
    PersistenceUncertain,
    TransportUnavailable,
    TransportUncertain,
    UnsupportedTarget,
    AmbiguousTarget,
    StaleTarget,
    PermissionDenied,
    Cancelled,
}

#[derive(Serialize, Deserialize)]
#[serde(
    tag = "status",
    content = "result",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum Reply {
    Ok(Response),
    Error(ErrorCode),
}

#[derive(Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "data",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum Response {
    Status(ServiceStatus),
    Paired(Pairing),
    Enrolled(CredentialMetadata),
    Credentials(Vec<CredentialMetadata>),
    Approval(ApprovalStatus),
    Revoked,
    Stopping,
    Browser(BrowserInfo),
    Browsers(Vec<BrowserInfo>),
    BrowserTargets(Vec<BrowserTarget>),
    BrowserDisconnected,
    BrowserCredentialConfigured,
    Fill(FillStatus),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServiceStatus {
    pub epoch: Uuid,
    pub client_id: Option<Uuid>,
    pub ready: bool,
    pub effects: Vec<String>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pairing {
    pub client_id: Uuid,
    pub token: String,
}
impl Drop for Pairing {
    fn drop(&mut self) {
        self.token.zeroize();
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CredentialMetadata {
    pub credential_ref: String,
    pub label: String,
    pub field_names: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Decision {
    Pending,
    Allowed,
    Denied,
    Expired,
    Uncertain,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApprovalStatus {
    pub approval_id: Uuid,
    pub decision: Decision,
    /// A metadata connection only; never material delivery or future-use authority.
    pub operation: String,
}

pub fn valid_label(value: &str) -> bool {
    !value.trim().is_empty()
        && value.len() <= 80
        && value.bytes().all(|b| b.is_ascii_graphic() || b == b' ')
}

pub fn valid_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
}

pub fn valid_reference(value: &str) -> bool {
    value.len() == 41
        && value
            .strip_prefix("cred_")
            .is_some_and(|id| Uuid::parse_str(id).is_ok_and(|parsed| parsed.to_string() == id))
}

impl Request {
    pub fn validate(&self) -> Result<(), ErrorCode> {
        let valid = match self {
            Self::Pair(p) => valid_label(&p.label),
            Self::Enroll(p) => {
                valid_label(&p.label)
                    && !p.field_names.is_empty()
                    && p.field_names.len() <= MAX_FIELDS
                    && p.field_names.iter().all(|f| valid_name(f))
                    && p.field_names
                        .iter()
                        .collect::<std::collections::HashSet<_>>()
                        .len()
                        == p.field_names.len()
            }
            Self::RequestAccess(p) => valid_reference(&p.credential_ref),
            Self::RegisterCdp(p) => {
                valid_label(&p.label)
                    && p.endpoint.len() <= 512
                    && !p.endpoint.chars().any(char::is_control)
            }
            Self::ConfigureBrowserCredential(p) => p.valid(),
            Self::SecureFill(p) => p.valid(),
            _ => true,
        };
        if valid {
            Ok(())
        } else {
            Err(ErrorCode::InvalidRequest)
        }
    }
}
