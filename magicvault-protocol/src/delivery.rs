//! Reference-only, human-authored destinations. Agents may invoke a registered
//! profile, never replace its command, URL, arguments or credential placements.
use crate::{valid_label, valid_name, valid_reference, MAX_FIELDS};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use uuid::Uuid;

// Reserve room alongside metadata ACLs, browser permissions and remembered
// consent. The service tests aggregate capacity against its durable state cap.
pub const MAX_DELIVERY_PROFILES: usize = 16;
pub const MAX_DELIVERY_JOBS: usize = 32;
pub const MAX_PROFILE_BYTES: usize = 12 * 1024;
pub const MAX_DELIVERY_OUTPUT_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum InputValue {
    Literal {
        value: String,
    },
    Credential {
        credential_ref: String,
        credential_field: String,
        #[serde(default)]
        prefix: String,
        #[serde(default)]
        suffix: String,
    },
}
impl InputValue {
    pub fn credential(&self) -> Option<(&str, &str)> {
        match self {
            Self::Credential {
                credential_ref,
                credential_field,
                ..
            } => Some((credential_ref, credential_field)),
            Self::Literal { .. } => None,
        }
    }
    fn valid(&self) -> bool {
        match self {
            Self::Literal { value } => text(value, 4096, true),
            Self::Credential {
                credential_ref,
                credential_field,
                prefix,
                suffix,
            } => {
                valid_reference(credential_ref)
                    && valid_name(credential_field)
                    && text(prefix, 256, true)
                    && text(suffix, 256, true)
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct NamedValue {
    pub name: String,
    pub value: InputValue,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum HttpBody {
    Text(InputValue),
    Form(Vec<NamedValue>),
    Json(Vec<NamedValue>),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ProcessDestination {
    pub executable: String,
    #[serde(default)]
    pub arguments: Vec<String>,
    pub working_directory: String,
    #[serde(default)]
    pub environment: Vec<NamedValue>,
    pub stdin: Option<InputValue>,
    pub timeout_secs: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct HttpDestination {
    pub url: String,
    pub method: String,
    #[serde(default)]
    pub headers: Vec<NamedValue>,
    #[serde(default)]
    pub query: Vec<NamedValue>,
    pub body: Option<HttpBody>,
    pub timeout_secs: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(
    tag = "kind",
    content = "config",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum DeliveryDestination {
    Process(ProcessDestination),
    Http(HttpDestination),
}
impl DeliveryDestination {
    pub fn kind(&self) -> DeliveryKind {
        match self {
            Self::Process(_) => DeliveryKind::Process,
            Self::Http(_) => DeliveryKind::Http,
        }
    }
    pub fn timeout_secs(&self) -> u64 {
        match self {
            Self::Process(p) => p.timeout_secs,
            Self::Http(p) => p.timeout_secs,
        }
    }
    pub fn values(&self) -> Vec<&InputValue> {
        match self {
            Self::Process(p) => p
                .environment
                .iter()
                .map(|v| &v.value)
                .chain(p.stdin.iter())
                .collect(),
            Self::Http(p) => {
                let mut values = p
                    .headers
                    .iter()
                    .chain(&p.query)
                    .map(|v| &v.value)
                    .collect::<Vec<_>>();
                match &p.body {
                    Some(HttpBody::Text(v)) => values.push(v),
                    Some(HttpBody::Form(rows) | HttpBody::Json(rows)) => {
                        values.extend(rows.iter().map(|v| &v.value))
                    }
                    None => {}
                }
                values
            }
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryKind {
    Process,
    Http,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct DeliveryProfile {
    pub label: String,
    pub destination: DeliveryDestination,
}
impl DeliveryProfile {
    pub fn valid(&self) -> bool {
        if !valid_label(&self.label) || !(1..=120).contains(&self.destination.timeout_secs()) {
            return false;
        }
        let valid = match &self.destination {
            DeliveryDestination::Process(p) => {
                absolute_path(&p.executable)
                    && absolute_path(&p.working_directory)
                    && p.arguments.len() <= 32
                    && p.arguments.iter().all(|a| text(a, 512, false))
                    && named(&p.environment, 8)
                    && p.environment.iter().all(|v| valid_name(&v.name))
            }
            DeliveryDestination::Http(p) => {
                text(&p.url, 2048, false)
                    && !p.url.is_empty()
                    && !p.method.is_empty()
                    && p.method.len() <= 16
                    && p.method
                        .bytes()
                        .all(|b| b.is_ascii_uppercase() || b == b'-')
                    && p.method != "CONNECT"
                    && named(&p.headers, 16)
                    && named(&p.query, 16)
                    && match &p.body {
                        Some(HttpBody::Form(rows) | HttpBody::Json(rows)) => named(rows, 16),
                        _ => true,
                    }
            }
        };
        let values = self.destination.values();
        let credentials = values
            .iter()
            .filter_map(|v| v.credential())
            .collect::<BTreeSet<_>>();
        valid
            && values.iter().all(|v| v.valid())
            && !credentials.is_empty()
            && credentials.len() <= MAX_FIELDS
            && serde_json::to_vec(self).is_ok_and(|v| v.len() <= MAX_PROFILE_BYTES)
    }
}

fn text(value: &str, max: usize, multiline: bool) -> bool {
    value.len() <= max
        && value.bytes().all(|b| {
            b.is_ascii_graphic() || b == b' ' || (multiline && matches!(b, b'\n' | b'\r' | b'\t'))
        })
}
fn absolute_path(value: &str) -> bool {
    value.starts_with('/')
        && value != "/"
        && text(value, 1024, false)
        && !value.split('/').any(|part| matches!(part, "." | ".."))
}
fn named(rows: &[NamedValue], max: usize) -> bool {
    rows.len() <= max
        && rows
            .iter()
            .all(|v| !v.name.is_empty() && text(&v.name, 64, false))
        && rows.iter().map(|v| &v.name).collect::<BTreeSet<_>>().len() == rows.len()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct DeliveryProfileInfo {
    pub profile_id: Uuid,
    pub label: String,
    pub kind: DeliveryKind,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ProfileQuery {
    pub profile_id: Uuid,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SecureDelivery {
    pub operation_id: Uuid,
    pub profile_id: Uuid,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryState {
    Pending,
    Running,
    Completed,
    Denied,
    Cancelled,
    Expired,
    Failed,
    Uncertain,
}

/// No recipient-controlled body, headers, stdout, stderr, exit code or exception
/// enters this projection. Completion is not a claim of remote business success.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct DeliveryStatus {
    pub operation_id: Uuid,
    pub kind: DeliveryKind,
    pub state: DeliveryState,
    /// Conservative dispatch evidence. A failed/cancelled operation may already
    /// have affected a recipient; a missing result supplies no such evidence.
    pub may_have_run: bool,
    pub error: Option<crate::ErrorCode>,
}
