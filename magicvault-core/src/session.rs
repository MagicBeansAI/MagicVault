use std::collections::HashMap;
use serde::{Deserialize, Serialize};

/// Session context assembled for replaying an API capability.
///
/// Gathered from browser state (cookies, storage) and the capability's
/// `auth_requirements`.
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionCookie {
    pub name: String,
    pub value: String,
}

impl std::fmt::Debug for SessionCookie {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SessionCookie")
            .field("name", &self.name)
            .field("value", &format_args!("<{} bytes>", self.value.len()))
            .finish()
    }
}

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct SessionContext {
    /// Trusted, in-memory routing metadata for cookies selected by SecretStore.
    /// Never accepted from or copied into serialized session payloads. Recipe
    /// replay needs the identity (name/domain/path) when applying Set-Cookie.
    #[serde(skip)]
    pub cookie_metadata: Vec<crate::CookieWithMetadata>,

    /// Ordered cookie pairs used to serialize the outbound `Cookie` header.
    ///
    /// This preserves duplicate cookie names for browser-equivalent replay.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cookie_header_values: Vec<SessionCookie>,

    /// Cookie name→value pairs extracted from browser state.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub cookies: HashMap<String, String>,

    /// Auth headers extracted from browser state or session.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub auth_headers: HashMap<String, String>,

    /// Auth query parameters extracted from observed browser traffic.
    ///
    /// Persisted traces redact these values; replay resolves them from the
    /// captured session instead of storing them in capability JSON.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub auth_query_params: HashMap<String, String>,

    /// localStorage key→value pairs.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub local_storage: HashMap<String, String>,

    /// sessionStorage key→value pairs.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub session_storage: HashMap<String, String>,
}

/// Sorted `name: <n bytes>` projection of a plaintext map, for `Debug` output
/// that keeps the diagnostic (which keys exist) and drops the values.
fn redacted_map_entries(map: &HashMap<String, String>) -> Vec<String> {
    let mut names = map.keys().cloned().collect::<Vec<_>>();
    names.sort();
    names
        .into_iter()
        .map(|name| {
            let length = map.get(&name).map_or(0, String::len);
            format!("{name}: <{length} bytes>")
        })
        .collect()
}

impl std::fmt::Debug for SessionContext {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SessionContext")
            .field("cookie_metadata_count", &self.cookie_metadata.len())
            .field("cookie_header_values", &self.cookie_header_values)
            .field("cookies", &redacted_map_entries(&self.cookies))
            .field("auth_headers", &redacted_map_entries(&self.auth_headers))
            .field(
                "auth_query_params",
                &redacted_map_entries(&self.auth_query_params),
            )
            .field("local_storage", &redacted_map_entries(&self.local_storage))
            .field(
                "session_storage",
                &redacted_map_entries(&self.session_storage),
            )
            .finish()
    }
}

impl SessionContext {
    pub fn has_cookie_name(&self, name: &str) -> bool {
        if !self.cookie_header_values.is_empty() {
            self.cookie_header_values
                .iter()
                .any(|cookie| cookie.name == name)
        } else {
            self.cookies.contains_key(name)
        }
    }

    pub fn available_cookie_names(&self) -> Vec<String> {
        if !self.cookie_header_values.is_empty() {
            let mut names = Vec::new();
            for cookie in &self.cookie_header_values {
                if !names.iter().any(|name| name == &cookie.name) {
                    names.push(cookie.name.clone());
                }
            }
            names
        } else {
            let mut names: Vec<String> = self.cookies.keys().cloned().collect();
            names.sort();
            names
        }
    }

    pub fn has_auth_query_param(&self, name: &str) -> bool {
        self.auth_query_params
            .keys()
            .any(|key| key.eq_ignore_ascii_case(name))
    }

    pub fn available_auth_query_param_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.auth_query_params.keys().cloned().collect();
        names.sort();
        names
    }

    pub fn cookie_header_string(&self) -> Option<String> {
        if !self.cookie_header_values.is_empty() {
            Some(
                self.cookie_header_values
                    .iter()
                    .map(|cookie| format!("{}={}", cookie.name, cookie.value))
                    .collect::<Vec<_>>()
                    .join("; "),
            )
        } else if !self.cookies.is_empty() {
            let mut cookies: Vec<_> = self.cookies.iter().collect();
            cookies.sort_by(|(left, _), (right, _)| left.cmp(right));
            Some(
                cookies
                    .into_iter()
                    .map(|(name, value)| format!("{name}={value}"))
                    .collect::<Vec<_>>()
                    .join("; "),
            )
        } else {
            None
        }
    }

    pub fn has_local_storage_key(&self, key: &str) -> bool {
        self.local_storage.contains_key(key)
    }

    pub fn has_session_storage_key(&self, key: &str) -> bool {
        self.session_storage.contains_key(key)
    }

    pub fn available_local_storage_keys(&self) -> Vec<String> {
        let mut keys: Vec<String> = self.local_storage.keys().cloned().collect();
        keys.sort();
        keys
    }

    pub fn available_session_storage_keys(&self) -> Vec<String> {
        let mut keys: Vec<String> = self.session_storage.keys().cloned().collect();
        keys.sort();
        keys
    }
}
