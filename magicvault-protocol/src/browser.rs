//! Browser requests contain references and locators, never credential values.
use crate::{valid_name, valid_reference, ErrorCode, MAX_FIELDS};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const MAX_BROWSERS: usize = 8;
pub const MAX_TARGETS: usize = 128;
pub const MAX_FILL_JOBS: usize = 32;
pub const MAX_ORIGINS: usize = 16;
pub const TARGET_TTL_SECS: u64 = 180;
pub const FILL_TIMEOUT_SECS: u64 = 30;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RegisterCdp {
    pub label: String,
    /// Explicit local browser websocket endpoint; never arbitrary HTTP fetch.
    pub endpoint: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct BrowserQuery {
    pub browser_handle: Uuid,
}

/// Optional discovery narrowing, never permission to deliver a credential.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TargetFilter {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top_origin: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tab_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct BrowserTargetsQuery {
    pub browser_handle: Uuid,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top_origin: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tab_id: Option<String>,
}

impl From<BrowserQuery> for BrowserTargetsQuery {
    fn from(query: BrowserQuery) -> Self {
        Self {
            browser_handle: query.browser_handle,
            top_origin: None,
            tab_id: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct BrowserRule {
    pub credential_ref: String,
    /// Exact canonical origins. Empty removes the client's browser permission.
    pub origins: Vec<String>,
    pub field_names: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct FillField {
    pub css: String,
    pub credential_ref: String,
    pub credential_field: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SecureFill {
    /// Choose once, retain for status lookup. Never retry with a new ID after
    /// an uncertain reply. Both this ID and the target handle are single-use.
    pub operation_id: Uuid,
    pub browser_handle: Uuid,
    pub target_handle: Uuid,
    pub fields: Vec<FillField>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct FillQuery {
    pub operation_id: Uuid,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BrowserBackend {
    Cdp,
    Extension,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct BrowserInfo {
    pub browser_handle: Uuid,
    pub label: String,
    pub backend: BrowserBackend,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct BrowserTarget {
    pub target_handle: Uuid,
    pub browser_handle: Uuid,
    /// Backend-issued identifiers for explicit mapping by a cooperating tool.
    /// These are not snapshot references or connection-local DOM node IDs.
    pub tab_id: String,
    pub frame_id: String,
    pub origin: String,
    pub top_origin: String,
    pub is_main_frame: bool,
    pub expires_in_seconds: u64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FillState {
    Pending,
    Filling,
    Filled,
    Denied,
    Cancelled,
    Expired,
    Failed,
    Partial,
    Uncertain,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FieldState {
    Filled,
    NotFilled,
    Uncertain,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct FillStatus {
    pub operation_id: Uuid,
    pub state: FillState,
    /// Order matches the original fields; never echo selectors or values.
    pub fields: Vec<FieldState>,
    pub error: Option<ErrorCode>,
}

pub fn valid_css(css: &str) -> bool {
    // Printable ASCII prevents bidi/control spoofing in the native exact-use
    // prompt. Unicode identifiers can use ordinary CSS escape sequences.
    !css.trim().is_empty() && css.len() <= 512 && css.bytes().all(|b| (b' '..=b'~').contains(&b))
}

impl SecureFill {
    pub fn valid(&self) -> bool {
        !self.operation_id.is_nil()
            && !self.fields.is_empty()
            && self.fields.len() <= MAX_FIELDS
            && self.fields.iter().all(|f| {
                valid_css(&f.css)
                    && valid_reference(&f.credential_ref)
                    && valid_name(&f.credential_field)
            })
            && self
                .fields
                .iter()
                .map(|f| &f.css)
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                == self.fields.len()
    }
}

impl BrowserRule {
    pub fn valid(&self) -> bool {
        valid_reference(&self.credential_ref)
            && self.origins.len() <= MAX_ORIGINS
            && self
                .origins
                .iter()
                .all(|s| s.len() <= 256 && !s.chars().any(char::is_control))
            && !self.field_names.is_empty()
            && self.field_names.len() <= MAX_FIELDS
            && self.field_names.iter().all(|s| valid_name(s))
            && self
                .field_names
                .iter()
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                == self.field_names.len()
            && self
                .origins
                .iter()
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                == self.origins.len()
    }
}
