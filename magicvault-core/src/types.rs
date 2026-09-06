use std::collections::HashMap;
use serde::{Deserialize, Serialize};
use crate::{policy::SecretPolicy, injection::SameSite};

/// A stored secret record shared by provisioned, captured, and ephemeral flows.
///
/// `Debug` is written by hand — see the impl below. Deriving it puts the
/// plaintext of every provider key, bearer header and session cookie one
/// `{:?}` away, and the entry is reachable from a `Debug` store state, so the
/// `{:?}` need not even name it.
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SecretEntry {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub fields: HashMap<String, String>,
    pub source: SecretSource,
    pub injection: InjectionTarget,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub policy: Option<SecretPolicy>,
    pub created_at: i64,
}

/// Names and lengths, never values.
///
/// `fields` holds plaintext: provisioned provider keys, and for captured
/// entries the live `Authorization` headers and session cookies of whatever the
/// browser was signed into. A derived `Debug` made a single
/// `tracing::debug!(?state)` anywhere in the crate dump the whole scope's vault
/// into the log file — no call site did, which is exactly why it would have
/// survived until one did.
///
/// Field *names* are kept because they are the diagnostic (`which cookie is
/// missing`), and lengths because a zero-length value is a real bug class. The
/// names are chosen by the capture code and the vault API's own validator, not
/// by a remote party. Values never appear, at any verbosity, in any build.
impl std::fmt::Debug for SecretEntry {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut names = self.fields.keys().cloned().collect::<Vec<_>>();
        names.sort();
        let redacted = names
            .into_iter()
            .map(|name| {
                let length = self.fields.get(&name).map_or(0, String::len);
                format!("{name}: <{length} bytes>")
            })
            .collect::<Vec<_>>();
        formatter
            .debug_struct("SecretEntry")
            .field("id", &self.id)
            .field("label", &self.label)
            .field("fields", &redacted)
            .field("source", &self.source)
            .field("injection", &self.injection)
            .field("policy", &self.policy)
            .field("created_at", &self.created_at)
            .finish()
    }
}

/// Where a secret originated and how its lifecycle should behave.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum SecretSource {
    Provisioned,
    Captured {
        origin: String,
        generation: u64,
        stale: bool,
        expires_hint: Option<i64>,
    },
    Ephemeral {
        task_id: String,
    },
}

/// Opaque handle to a secret reference or single-use grant.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum SecretRef {
    Provisioned(String),
    Grant(String),
    Session(String),
    Placeholder(String),
}

/// How secret values should be placed into an outbound action.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum InjectionTarget {
    Header {
        name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        prefix: Option<String>,
    },
    FormFields(HashMap<String, String>),
    Cookies(Vec<CookieSpec>),
    Inline,
}

/// Cookie routing metadata kept alongside captured cookie values.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CookieSpec {
    pub name: String,
    pub domain: String,
    pub path: String,
    pub secure: bool,
    pub http_only: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub same_site: Option<SameSite>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires: Option<i64>,
}

/// Resolve a cookie field value from the indexed captured-cookie layout.
pub fn cookie_field_value<'a>(
    fields: &'a HashMap<String, String>,
    index: usize,
    name: &str,
) -> Option<&'a str> {
    fields
        .get(&format!("cookie:{index}:{name}"))
        .map(String::as_str)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PLACEHOLDER_VALUE: &str = "PLACEHOLDER-NOT-A-REAL-KEY";

    fn placeholder_entry() -> SecretEntry {
        SecretEntry {
            id: "placeholder-secret".to_string(),
            label: "Placeholder".to_string(),
            fields: HashMap::from([
                ("api_key".to_string(), PLACEHOLDER_VALUE.to_string()),
                ("empty".to_string(), String::new()),
            ]),
            source: SecretSource::Provisioned,
            injection: InjectionTarget::Header {
                name: "Authorization".to_string(),
                prefix: Some("Bearer ".to_string()),
            },
            policy: None,
            created_at: 0,
        }
    }

    /// `fields` is plaintext, and the entry is reachable from a `Debug` store
    /// state — so a derived `Debug` put the whole scope's vault one
    /// `tracing::debug!(?state)` away, at any call site, in any build. No call
    /// site did it, which is exactly why it would have survived until one did.
    #[test]
    fn debug_formatting_never_prints_a_field_value() {
        let rendered = format!("{:?}", placeholder_entry());

        assert!(
            !rendered.contains(PLACEHOLDER_VALUE),
            "SecretEntry's Debug leaked a field value"
        );
        assert!(
            rendered.contains("api_key"),
            "field names are the diagnostic and should survive redaction"
        );
        assert!(
            rendered.contains(&format!("<{} bytes>", PLACEHOLDER_VALUE.len())),
            "the length is what replaces the value"
        );
        assert!(
            rendered.contains("<0 bytes>"),
            "an empty value is a real bug class and must stay visible"
        );
    }

    /// The leak path was never a direct `{:?}` on the entry — it was the entry
    /// sitting inside something else that derives `Debug`.
    #[test]
    fn debug_formatting_stays_redacted_inside_a_container() {
        let nested = HashMap::from([("placeholder-secret".to_string(), placeholder_entry())]);
        let rendered = format!("{nested:?}");

        assert!(
            !rendered.contains(PLACEHOLDER_VALUE),
            "a container's derived Debug leaked a field value"
        );
    }
}
