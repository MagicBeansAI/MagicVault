use std::{collections::HashMap, sync::OnceLock};
use crate::json_traversal::{
    discard_json_iteratively, exact_json_encoded_len, inspect_json_bounded,
    json_bytes_depth_is_bounded, json_bytes_nodes_are_bounded, map_json_strings_borrowed_canonical,
    map_json_strings_owned, write_json, MAX_RETAINED_JSON_DEPTH,
};
use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use crate::store::SecretStore;
use zeroize::Zeroize;

#[doc(hidden)]
pub const REDACTED_PREFIX: &str = "[REDACTED:";
#[doc(hidden)]
pub const REF_PREFIX: &str = "[REF:";
const PLACEHOLDER_SUFFIX: &str = "]";
const MAX_PROVIDER_SANITIZER_JSON_NODES: usize = 1_000_000;
const MAX_PROVIDER_BOUND_TEXT_BYTES: usize = 4 * 1024 * 1024;
const MAX_JWT_TOKEN_BYTES: usize = 512 * 1024;
const MAX_JWT_PAYLOAD_B64_BYTES: usize = 256 * 1024;
const MAX_JWT_CLAIMS_NODES: usize = 16_384;
const PROVIDER_JSON_SAFETY_OMISSION: &str =
    "[OMITTED: provider-bound JSON exceeds byte, depth, or node safety limits]";
const PROVIDER_TEXT_SAFETY_OMISSION: &str =
    "[OMITTED: provider-bound text exceeds byte safety limit]";
/// Values shorter than this keep exact-match redaction only. Their hex or
/// base64 forms are short enough to collide with ordinary text, and the
/// values themselves (an OTP, a PIN) are not what a transformed exfiltration
/// targets.
const KNOWN_VALUE_VARIANT_MIN_BYTES: usize = 8;

/// Live plaintext values resolved during injection, retained only so results
/// can be redacted before they re-enter model-visible context.
///
/// Values zeroize on drop. `Debug` prints names and lengths only. There is
/// deliberately no `Clone` and no `Serialize`: this table exists to scrub
/// output, never to be carried, logged, or persisted. It dereferences to the
/// underlying map so call sites read and insert as before.
#[derive(Default)]
pub struct KnownSecretValues {
    inner: HashMap<String, String>,
    #[cfg(test)]
    drop_probe: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
}

impl KnownSecretValues {
    pub fn new() -> Self {
        Self::default()
    }

    #[cfg(test)]
    fn install_drop_probe(&mut self, probe: std::sync::Arc<std::sync::atomic::AtomicBool>) {
        self.drop_probe = Some(probe);
    }
}

impl From<HashMap<String, String>> for KnownSecretValues {
    fn from(inner: HashMap<String, String>) -> Self {
        Self {
            inner,
            #[cfg(test)]
            drop_probe: None,
        }
    }
}

impl FromIterator<(String, String)> for KnownSecretValues {
    fn from_iter<I: IntoIterator<Item = (String, String)>>(iter: I) -> Self {
        Self::from(iter.into_iter().collect::<HashMap<_, _>>())
    }
}

impl IntoIterator for KnownSecretValues {
    type Item = (String, String);
    type IntoIter = std::collections::hash_map::IntoIter<String, String>;

    fn into_iter(mut self) -> Self::IntoIter {
        std::mem::take(&mut self.inner).into_iter()
    }
}

impl std::ops::Deref for KnownSecretValues {
    type Target = HashMap<String, String>;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl std::ops::DerefMut for KnownSecretValues {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}

impl std::fmt::Debug for KnownSecretValues {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut names = self.inner.keys().cloned().collect::<Vec<_>>();
        names.sort();
        let redacted = names
            .into_iter()
            .map(|name| {
                let length = self.inner.get(&name).map_or(0, String::len);
                format!("{name}: <{length} bytes>")
            })
            .collect::<Vec<_>>();
        formatter
            .debug_struct("KnownSecretValues")
            .field("values", &redacted)
            .finish()
    }
}

impl Drop for KnownSecretValues {
    fn drop(&mut self) {
        for value in self.inner.values_mut() {
            value.zeroize();
        }
        #[cfg(test)]
        if let Some(probe) = &self.drop_probe {
            use std::sync::atomic::Ordering;
            probe.store(
                self.inner
                    .values()
                    .all(|value| value.as_bytes().iter().all(|byte| *byte == 0)),
                Ordering::SeqCst,
            );
        }
    }
}

/// Every string that must be redacted for a set of known values: the exact
/// values, plus — for values long enough not to collide with ordinary text —
/// the encodings a model-authored command can trivially produce: base64 in
/// its four common forms, hex in both cases, percent-encoding, and JSON string
/// escaping. Longest first, so a longer match is never split by a shorter one.
///
/// This is the single source of replacements for every result sanitizer in
/// the crate. Exact matching is what it always was; the encoded variants are
/// what stop `echo $SECRET | base64` from walking a value past the redactor.
pub fn known_value_replacements(known_values: &HashMap<String, String>) -> Vec<String> {
    let mut replacements = Vec::new();
    for value in known_values.values().filter(|value| !value.is_empty()) {
        replacements.push(value.clone());
        if value.len() >= KNOWN_VALUE_VARIANT_MIN_BYTES {
            replacements.extend(encoded_variants(value));
        }
    }
    replacements.sort_by(|left, right| right.len().cmp(&left.len()).then_with(|| left.cmp(right)));
    replacements.dedup();
    replacements
}

fn encoded_variants(value: &str) -> Vec<String> {
    use base64::engine::general_purpose::{STANDARD, STANDARD_NO_PAD, URL_SAFE, URL_SAFE_NO_PAD};
    let bytes = value.as_bytes();
    let mut variants = vec![
        STANDARD.encode(bytes),
        STANDARD_NO_PAD.encode(bytes),
        URL_SAFE.encode(bytes),
        URL_SAFE_NO_PAD.encode(bytes),
        hex::encode(bytes),
        hex::encode_upper(bytes),
        urlencoding::encode(value).into_owned(),
    ];
    if let Ok(json) = serde_json::to_string(value) {
        variants.push(json[1..json.len() - 1].to_string());
    }
    variants.retain(|variant| !variant.is_empty() && variant != value);
    variants
}

/// `SameSite` attribute for cookies.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SameSite {
    Strict,
    Lax,
    None,
}

/// A captured cookie with routing metadata and value.
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CookieWithMetadata {
    pub name: String,
    pub value: String,
    pub domain: String,
    pub path: String,
    pub secure: bool,
    pub http_only: bool,
    pub same_site: SameSite,
    pub expires: Option<i64>,
}

impl std::fmt::Debug for CookieWithMetadata {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CookieWithMetadata")
            .field("name", &self.name)
            .field("value", &format_args!("<{} bytes>", self.value.len()))
            .field("domain", &self.domain)
            .field("path", &self.path)
            .field("secure", &self.secure)
            .field("http_only", &self.http_only)
            .field("same_site", &self.same_site)
            .field("expires", &self.expires)
            .finish()
    }
}

/// Errors produced while injecting secrets into an action.
#[derive(Debug, thiserror::Error)]
pub enum InjectionError {
    #[error("action type does not support this injection target")]
    UnsupportedTarget,

    #[error("inline injection must use inject_inline()")]
    InlineRequiresStore,

    #[error("browser cookie restore requires a bound runtime domain")]
    MissingBoundDomain,

    #[error("secret field '{0}' was not found")]
    MissingField(String),

    #[error("form field injection requires a JSON object body")]
    NonJsonBody,

    #[error("cookie domain '{cookie_domain}' is not compatible with bound runtime domain '{bound_domain}'")]
    CookieDomainMismatch {
        cookie_domain: String,
        bound_domain: String,
    },
}

/// Detect and decode the `exp` claim from a JWT-like token.
pub fn detect_jwt_expiry(token: &str) -> Option<i64> {
    if token.len() > MAX_JWT_TOKEN_BYTES || !token.starts_with("eyJ") {
        return None;
    }

    let mut parts = token.split('.');
    let _header = parts.next()?;
    let payload_b64 = parts.next()?;
    let _signature = parts.next()?;
    if parts.next().is_some() || payload_b64.len() > MAX_JWT_PAYLOAD_B64_BYTES {
        return None;
    }

    let standard = payload_b64.replace('-', "+").replace('_', "/");
    let padded = match standard.len() % 4 {
        2 => format!("{standard}=="),
        3 => format!("{standard}="),
        _ => standard,
    };

    let decoded = base64::engine::general_purpose::STANDARD
        .decode(padded)
        .ok()?;
    if decoded.len() > MAX_JWT_PAYLOAD_B64_BYTES
        || !json_bytes_depth_is_bounded(&decoded, MAX_RETAINED_JSON_DEPTH)
        || !json_bytes_nodes_are_bounded(&decoded, MAX_JWT_CLAIMS_NODES)
    {
        return None;
    }
    #[derive(Deserialize)]
    struct JwtExpiryClaims {
        exp: Option<i64>,
    }
    serde_json::from_slice::<JwtExpiryClaims>(&decoded)
        .ok()?
        .exp
}

pub fn cookie_domain_matches_host(cookie_domain: &str, host: &str) -> bool {
    let cookie_domain = cookie_domain
        .trim()
        .trim_end_matches('.')
        .to_ascii_lowercase();
    let host = host.trim().trim_end_matches('.').to_ascii_lowercase();
    if cookie_domain.is_empty() || host.is_empty() {
        return false;
    }

    if let Some(bare) = cookie_domain.strip_prefix('.') {
        !bare.is_empty() && (host == bare || host.ends_with(&format!(".{bare}")))
    } else {
        host == cookie_domain
    }
}

/// Filter a list of cookies to those applicable for the given URL.
pub fn filter_cookies_for_url(
    cookies: &[CookieWithMetadata],
    url: &url::Url,
) -> Vec<CookieWithMetadata> {
    let now = chrono::Utc::now().timestamp();
    let url_host = url.host_str().unwrap_or("");
    let url_path = url.path();
    let is_secure = url.scheme() == "https";

    let mut applicable = Vec::new();
    for (index, cookie) in cookies.iter().enumerate() {
        if let Some(exp) = cookie.expires {
            if exp < now {
                continue;
            }
        }
        if cookie.secure && !is_secure {
            continue;
        }

        if !cookie_domain_matches_host(&cookie.domain, url_host) {
            continue;
        }

        if !cookie_path_matches_request(url_path, &cookie.path) {
            continue;
        }

        applicable.push((index, cookie.clone()));
    }

    applicable.sort_by(|(left_index, left_cookie), (right_index, right_cookie)| {
        right_cookie
            .path
            .len()
            .cmp(&left_cookie.path.len())
            .then_with(|| {
                right_cookie
                    .domain
                    .trim_start_matches('.')
                    .len()
                    .cmp(&left_cookie.domain.trim_start_matches('.').len())
            })
            .then_with(|| left_index.cmp(right_index))
    });

    applicable.into_iter().map(|(_, cookie)| cookie).collect()
}

fn cookie_path_matches_request(request_path: &str, cookie_path: &str) -> bool {
    let request_path = if request_path.is_empty() {
        "/"
    } else {
        request_path
    };
    let cookie_path = match cookie_path {
        "" => "/",
        path if path.starts_with('/') => path,
        _ => "/",
    };

    if request_path == cookie_path {
        return true;
    }

    request_path
        .strip_prefix(cookie_path)
        .is_some_and(|remaining| cookie_path.ends_with('/') || remaining.starts_with('/'))
}


/// Last-mile sanitizer for JSON crossing a provider, telemetry, or legacy
/// transcript boundary.
///
/// This is intentionally not a canonical-result redactor. Pattern matching
/// cannot distinguish real credentials from legitimate domain data named
/// `token`, `secret`, `cookie`, or `authorization`, so it must never mutate
/// durable canonical evidence. Canonical dispatch paths use exact injected
/// value replacement and capability-owned policies before this boundary.
pub fn sanitize_json_for_provider(value: &Value) -> Value {
    sanitize_json_for_provider_with_limits(
        value,
        MAX_PROVIDER_SANITIZER_JSON_NODES,
        MAX_PROVIDER_BOUND_TEXT_BYTES,
    )
}

fn sanitize_json_for_provider_with_limits(
    value: &Value,
    max_nodes: usize,
    max_bytes: usize,
) -> Value {
    if !provider_json_is_admitted(value, max_nodes, max_bytes) {
        return Value::String(PROVIDER_JSON_SAFETY_OMISSION.to_string());
    }
    // Copy only the bounded retained prefix while mapping it. Building a full
    // canonical clone before truncating would transiently duplicate an entire
    // multi-megabyte provider payload, including branches that cannot survive
    // the retained-depth contract.
    enforce_sanitized_provider_byte_limit(sanitize_provider_json_value_borrowed(value), max_bytes)
}

/// Owned provider-boundary variant. Callers discarding their source should
/// transfer it here so the sanitizer can reuse its existing allocations rather
/// than cloning an admitted multi-megabyte tree.
pub fn sanitize_json_for_provider_owned(value: Value) -> Value {
    sanitize_json_for_provider_owned_with_limits(
        value,
        MAX_PROVIDER_SANITIZER_JSON_NODES,
        MAX_PROVIDER_BOUND_TEXT_BYTES,
    )
}

fn sanitize_json_for_provider_owned_with_limits(
    mut value: Value,
    max_nodes: usize,
    max_bytes: usize,
) -> Value {
    if !provider_json_is_admitted(&value, max_nodes, max_bytes) {
        discard_json_iteratively(std::mem::take(&mut value));
        return Value::String(PROVIDER_JSON_SAFETY_OMISSION.to_string());
    }
    enforce_sanitized_provider_byte_limit(sanitize_provider_json_value(value), max_bytes)
}

fn enforce_sanitized_provider_byte_limit(mut value: Value, max_bytes: usize) -> Value {
    if exact_json_encoded_len(&value) <= max_bytes {
        return value;
    }
    // Exact credential redaction can expand short source values (for example,
    // an empty password becomes `[REDACTED]`). Re-apply the same authoritative
    // encoded ceiling after mapping and drain the rejected tree without
    // recursive `Value` destruction.
    discard_json_iteratively(std::mem::take(&mut value));
    Value::String(PROVIDER_JSON_SAFETY_OMISSION.to_string())
}

fn provider_json_is_admitted(value: &Value, max_nodes: usize, max_bytes: usize) -> bool {
    inspect_json_bounded(value, max_nodes).is_some() && exact_json_encoded_len(value) <= max_bytes
}

/// Text counterpart for provider-bound narration and legacy observations.
pub fn sanitize_text_for_provider(value: &str) -> String {
    sanitize_text_for_provider_with_byte_limit(value, MAX_PROVIDER_BOUND_TEXT_BYTES)
}

fn sanitize_text_for_provider_with_byte_limit(value: &str, max_bytes: usize) -> String {
    let json_looking = value
        .trim_start()
        .as_bytes()
        .first()
        .is_some_and(|byte| matches!(byte, b'{' | b'['));
    if value.len() > max_bytes {
        // A structurally sensitive JSON document must never fall through to
        // the weaker prose regexes after byte admission rejects it. Plain text
        // is omitted as well: truncating before redaction could split a
        // credential and accidentally expose the retained half.
        return if json_looking {
            PROVIDER_JSON_SAFETY_OMISSION
        } else {
            PROVIDER_TEXT_SAFETY_OMISSION
        }
        .to_string();
    }

    // JSON-looking tool output is untrusted encoded input. Establish the same
    // retained byte/depth/node contract before typed Serde. Malformed content
    // inside that admission boundary retains the established text-redaction
    // fallback instead of growing a recursive Value tree on a chat/execution
    // worker stack.
    let depth_is_bounded = json_bytes_depth_is_bounded(value.as_bytes(), MAX_RETAINED_JSON_DEPTH);
    let nodes_are_bounded =
        json_bytes_nodes_are_bounded(value.as_bytes(), MAX_PROVIDER_SANITIZER_JSON_NODES);
    if depth_is_bounded && nodes_are_bounded {
        if let Ok(json) = serde_json::from_str::<Value>(value) {
            let sanitized = sanitize_provider_json_value(json);
            let encoded_len = exact_json_encoded_len(&sanitized);
            let serialized = if encoded_len <= max_bytes {
                let mut bytes = Vec::with_capacity(encoded_len);
                write_json(&sanitized, &mut bytes)
                    .ok()
                    .and_then(|()| String::from_utf8(bytes).ok())
            } else {
                None
            };
            discard_json_iteratively(sanitized);
            if let Some(serialized) = serialized {
                return serialized;
            }
            if json_looking {
                return PROVIDER_JSON_SAFETY_OMISSION.to_string();
            }
        }
    } else if json_looking {
        // Do not fall back to weaker pattern-only redaction for an encoded JSON
        // document we deliberately refused to parse: it may contain secret
        // fields that are recognizable only structurally.
        return PROVIDER_JSON_SAFETY_OMISSION.to_string();
    }
    sanitize_provider_text(value)
}

/// Compile provider-boundary redaction matchers from a shallow runtime root.
///
/// Regex construction is intentionally lazy, but its compiler uses meaningful
/// native stack. If the first provider-bound value happens to be produced deep
/// inside an agentic poll chain, initializing these matchers there can combine
/// with that caller's stack footprint. The execution runtime invokes this
/// idempotent warm-up before constructing each top-level job; after the first
/// call every `OnceLock` lookup is effectively free.
pub fn warm_provider_sanitizer() {
    let _ = sanitize_provider_text("");
}

#[doc(hidden)]
pub fn replace_placeholders(
    text: &str,
    store: &SecretStore,
    ephemeral_scope_id: Option<&str>,
    known_values: &mut KnownSecretValues,
) -> String {
    let mut replaced = text.to_string();
    loop {
        let Some((marker, placeholder_id)) = next_placeholder(&replaced) else {
            break;
        };
        let value = ephemeral_scope_id
            .and_then(|scope_id| store.get_ephemeral_scoped(scope_id, &placeholder_id))
            .or_else(|| {
                if ephemeral_scope_id.is_none() {
                    store.get_ephemeral(&placeholder_id)
                } else {
                    None
                }
            });
        let Some(value) = value else {
            break;
        };
        known_values.insert(placeholder_id, value.clone());
        replaced = replaced.replacen(&marker, &value, 1);
    }
    replaced
}

/// Resolve inline `[REDACTED:<id>]` / `[REF:<id>]` placeholders in raw text.
///
/// This is the narrow text-level helper for trusted runtimes that do not flow
/// through a host application's action enum, such as a browser inner-loop bridge.
/// Returned `known_values` must be used to sanitize command output before it
/// re-enters model-visible context.
pub fn resolve_inline_placeholders(
    text: &str,
    store: &SecretStore,
    ephemeral_scope_id: Option<&str>,
) -> (String, KnownSecretValues) {
    let mut known_values = KnownSecretValues::new();
    let resolved = replace_placeholders(text, store, ephemeral_scope_id, &mut known_values);
    (resolved, known_values)
}

fn next_placeholder(text: &str) -> Option<(String, String)> {
    [REDACTED_PREFIX, REF_PREFIX]
        .into_iter()
        .filter_map(|prefix| {
            let start = text.find(prefix)?;
            let rest = &text[start + prefix.len()..];
            let end = rest.find(PLACEHOLDER_SUFFIX)?;
            let id = rest[..end].to_string();
            let marker = format!("{prefix}{id}{PLACEHOLDER_SUFFIX}");
            Some((start, marker, id))
        })
        .min_by_key(|(start, _, _)| *start)
        .map(|(_, marker, id)| (marker, id))
}

#[doc(hidden)]
pub fn replace_known_values(input: &str, replacements: &[String]) -> String {
    replacements.iter().fold(input.to_string(), |acc, value| {
        acc.replace(value, "[REDACTED]")
    })
}

#[doc(hidden)]
pub fn sanitize_json_value(value: Value, replacements: &[String]) -> Value {
    map_json_strings_owned(
        value,
        |text| replace_known_values(&text, replacements),
        |_, _| None,
    )
}

fn sanitize_provider_json_value(value: Value) -> Value {
    map_json_strings_owned(
        value,
        |text| sanitize_provider_text(&text),
        |key, value| {
            (is_explicit_provider_credential_field(key) && !value.is_null())
                .then(|| Value::String("[REDACTED]".to_string()))
        },
    )
}

fn sanitize_provider_json_value_borrowed(value: &Value) -> Value {
    map_json_strings_borrowed_canonical(value, sanitize_provider_text, |key, value| {
        (is_explicit_provider_credential_field(key) && !value.is_null())
            .then(|| Value::String("[REDACTED]".to_string()))
    })
}

/// Exact protocol credential fields whose value is credential material by
/// definition. This deliberately is not a fuzzy key-name detector: generic
/// domain fields such as `token`, `secret`, `cookie`, and `authorization`
/// remain data and are protected only by exact-value replacement or strong
/// credential syntax in the value itself.
fn is_explicit_provider_credential_field(key: &str) -> bool {
    matches!(
        key.to_ascii_lowercase().as_str(),
        "password"
            | "passwd"
            | "pwd"
            | "api_key"
            | "api-key"
            | "apikey"
            | "client_secret"
            | "client-secret"
            | "clientsecret"
            | "access_token"
            | "access-token"
            | "accesstoken"
            | "refresh_token"
            | "refresh-token"
            | "refreshtoken"
            | "id_token"
            | "id-token"
            | "idtoken"
            | "auth_token"
            | "auth-token"
            | "authtoken"
            | "credential_token"
            | "credential-token"
            | "credentialtoken"
            | "private_key"
            | "private-key"
            | "privatekey"
            | "csrf_token"
            | "csrf-token"
            | "csrftoken"
            | "xsrf_token"
            | "xsrf-token"
            | "xsrftoken"
    )
}

fn sanitize_provider_text(input: &str) -> String {
    static PRIVATE_KEY_RE: OnceLock<regex::Regex> = OnceLock::new();
    static BEARER_VALUE_RE: OnceLock<regex::Regex> = OnceLock::new();
    static LABELED_BEARER_RE: OnceLock<regex::Regex> = OnceLock::new();
    static BASIC_VALUE_RE: OnceLock<regex::Regex> = OnceLock::new();
    static LABELED_BASIC_RE: OnceLock<regex::Regex> = OnceLock::new();
    static KEY_VALUE_SECRET_RE: OnceLock<regex::Regex> = OnceLock::new();
    static URL_SECRET_RE: OnceLock<regex::Regex> = OnceLock::new();
    static COMMON_TOKEN_RE: OnceLock<regex::Regex> = OnceLock::new();
    static CREDENTIAL_CONNECTION_STRING_RE: OnceLock<regex::Regex> = OnceLock::new();

    let redacted = PRIVATE_KEY_RE
        .get_or_init(|| {
            regex::Regex::new(
                r"(?s)-----BEGIN [A-Z ]*PRIVATE KEY-----.*?-----END [A-Z ]*PRIVATE KEY-----",
            )
            .expect("valid private-key redaction regex")
        })
        .replace_all(input, "[REDACTED_PRIVATE_KEY]")
        .into_owned();
    let redacted = BEARER_VALUE_RE
        .get_or_init(|| {
            regex::Regex::new(r"(?i)^\s*(bearer)\s+[A-Za-z0-9._~+/=-]{8,}\s*$")
                .expect("valid bearer-value redaction regex")
        })
        .replace_all(&redacted, "$1 [REDACTED]")
        .into_owned();
    let redacted = LABELED_BEARER_RE
        .get_or_init(|| {
            regex::Regex::new(
                r#"(?i)([\"']?authorization[\"']?\s*[:=]\s*[\"']?bearer)\s+[A-Za-z0-9._~+/=-]{8,}"#,
            )
            .expect("valid labeled-bearer redaction regex")
        })
        .replace_all(&redacted, "$1 [REDACTED]")
        .into_owned();
    let redacted = BASIC_VALUE_RE
        .get_or_init(|| {
            regex::Regex::new(r"(?i)^\s*basic\s+([A-Za-z0-9+/]+={0,2})\s*$")
                .expect("valid basic-value redaction regex")
        })
        .replace_all(&redacted, |captures: &regex::Captures<'_>| {
            if is_valid_basic_auth(captures.get(1).map_or("", |value| value.as_str())) {
                "Basic [REDACTED]".to_string()
            } else {
                captures[0].to_string()
            }
        })
        .into_owned();
    let redacted = LABELED_BASIC_RE
        .get_or_init(|| {
            regex::Regex::new(
                r#"(?i)([\"']?authorization[\"']?\s*[:=]\s*[\"']?basic)\s+([A-Za-z0-9+/]+={0,2})"#,
            )
            .expect("valid labeled-basic redaction regex")
        })
        .replace_all(&redacted, |captures: &regex::Captures<'_>| {
            if is_valid_basic_auth(captures.get(2).map_or("", |value| value.as_str())) {
                format!("{} [REDACTED]", &captures[1])
            } else {
                captures[0].to_string()
            }
        })
        .into_owned();
    let redacted = KEY_VALUE_SECRET_RE
        .get_or_init(|| {
            regex::Regex::new(
                r#"(?i)\b(api[_-]?key|client[_-]?secret|access[_-]?token|refresh[_-]?token|id[_-]?token|auth[_-]?token|credential[_-]?token|private[_ -]?key|csrf[_-]?token|xsrf[_-]?token)\b\s*[:=]\s*[\"']?[^\s\"',;&}\]]+"#,
            )
            .expect("valid key-value secret redaction regex")
        })
        .replace_all(&redacted, "$1=[REDACTED]")
        .into_owned();
    let redacted = URL_SECRET_RE
        .get_or_init(|| {
            regex::Regex::new(
                r"(?i)([?&](?:api[_-]?key|access[_-]?token|refresh[_-]?token|auth[_-]?token|password|client[_-]?secret)=)[^&#\s]+",
            )
            .expect("valid URL secret redaction regex")
        })
        .replace_all(&redacted, "$1[REDACTED]")
        .into_owned();
    let redacted = COMMON_TOKEN_RE
        .get_or_init(|| {
            regex::Regex::new(
                r"\b(sk-[A-Za-z0-9_-]{10,}|gh[pousr]_[A-Za-z0-9_]{20,}|xox[baprs]-[A-Za-z0-9-]{10,})\b",
            )
            .expect("valid common-token redaction regex")
        })
        .replace_all(&redacted, "[REDACTED_TOKEN]")
        .into_owned();
    CREDENTIAL_CONNECTION_STRING_RE
        .get_or_init(|| {
            regex::Regex::new(
                r#"(?i)\b(?:postgres|postgresql|mysql|mongodb|redis)://[^:/\s"']+:[^@\s"']+@[^\s"']+"#,
            )
            .expect("valid credential-bearing connection-string redaction regex")
        })
        .replace_all(&redacted, "[REDACTED_CONNECTION_STRING]")
        .into_owned()
}

fn is_valid_basic_auth(encoded: &str) -> bool {
    base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .is_ok_and(|decoded| decoded.contains(&b':'))
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jwt_expiry_detection_preserves_valid_claims_and_rejects_deep_payloads_before_serde() {
        let encode = |payload: &[u8]| {
            format!(
                "eyJhbGciOiJub25lIn0.{}.signature",
                base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(payload)
            )
        };
        assert_eq!(
            detect_jwt_expiry(&encode(br#"{"exp":1777777777}"#)),
            Some(1_777_777_777)
        );

        std::thread::Builder::new()
            .name("jwt-json-admission-small-stack".to_string())
            .stack_size(256 * 1024)
            .spawn(move || {
                let depth = 20_000;
                let mut payload = "[".repeat(depth);
                payload.push_str("null");
                payload.push_str(&"]".repeat(depth));
                assert_eq!(detect_jwt_expiry(&encode(payload.as_bytes())), None);
            })
            .expect("spawn JWT admission regression")
            .join()
            .expect("JWT admission must remain stack safe");

        let oversized_payload = "A".repeat(MAX_JWT_PAYLOAD_B64_BYTES + 1);
        let oversized = format!("eyJhbGciOiJub25lIn0.{oversized_payload}.signature");
        assert_eq!(detect_jwt_expiry(&oversized), None);

        let oversized_token = format!(
            "eyJhbGciOiJub25lIn0.e30.{}",
            "s".repeat(MAX_JWT_TOKEN_BYTES)
        );
        assert_eq!(detect_jwt_expiry(&oversized_token), None);
    }

    #[test]
    fn cookie_domain_matches_host_uses_browser_cookie_rules() {
        assert!(cookie_domain_matches_host(
            "merchant.example.com",
            "merchant.example.com"
        ));
        assert!(!cookie_domain_matches_host(
            "example.com",
            "merchant.example.com"
        ));
        assert!(cookie_domain_matches_host(
            ".example.com",
            "merchant.example.com"
        ));
        assert!(cookie_domain_matches_host(".example.com", "example.com"));
        assert!(!cookie_domain_matches_host(
            ".example.com",
            "merchant.other.com"
        ));
    }

    #[test]
    fn filter_cookies_for_url_respects_rfc_path_boundaries() {
        let cookies = vec![
            CookieWithMetadata {
                name: "session".to_string(),
                value: "admin-cookie".to_string(),
                domain: "api.example.com".to_string(),
                path: "/admin".to_string(),
                secure: true,
                http_only: true,
                same_site: SameSite::Lax,
                expires: None,
            },
            CookieWithMetadata {
                name: "session".to_string(),
                value: "root-cookie".to_string(),
                domain: "api.example.com".to_string(),
                path: "/".to_string(),
                secure: true,
                http_only: true,
                same_site: SameSite::Lax,
                expires: None,
            },
        ];

        let admin = filter_cookies_for_url(
            &cookies,
            &url::Url::parse("https://api.example.com/admin/dashboard").unwrap(),
        );
        assert_eq!(
            admin
                .into_iter()
                .map(|cookie| cookie.value)
                .collect::<Vec<_>>(),
            vec!["admin-cookie".to_string(), "root-cookie".to_string()]
        );

        let sibling = filter_cookies_for_url(
            &cookies,
            &url::Url::parse("https://api.example.com/administrator").unwrap(),
        );
        assert_eq!(
            sibling
                .into_iter()
                .map(|cookie| cookie.value)
                .collect::<Vec<_>>(),
            vec!["root-cookie".to_string()]
        );
    }

    #[test]
    fn provider_guard_redacts_strong_credential_syntax_without_key_name_guessing() {
        let guarded = sanitize_json_for_provider(&serde_json::json!({
            "records": [{
                "relationship": "wife",
                "value": "14 September",
                "authorization": "Bearer abcdefghijklmnop",
                "link": "https://example.test/item?access_token=secret-value&view=full",
                "diagnostic": "client_secret=super-secret-value",
            }],
            "completion_tokens": 42,
            "result_key": "ordinary-domain-key",
            "token": "pagination-cursor-27",
            "access_token": "opaque-provider-credential",
            "secret": "surprise party",
            "cookie": "chocolate chip",
            "public_database": "postgres://localhost/example",
            "spoken": "Completed with token=domain-value",
            "basic_auth": "Basic dXNlcjpwYXNz"
        }));

        assert_eq!(guarded["records"][0]["relationship"], "wife");
        assert_eq!(guarded["records"][0]["value"], "14 September");
        assert_eq!(guarded["records"][0]["authorization"], "Bearer [REDACTED]");
        assert_eq!(
            guarded["records"][0]["link"],
            "https://example.test/item?access_token=[REDACTED]&view=full"
        );
        assert_eq!(
            guarded["records"][0]["diagnostic"],
            "client_secret=[REDACTED]"
        );
        assert_eq!(guarded["completion_tokens"], 42);
        assert_eq!(guarded["result_key"], "ordinary-domain-key");
        assert_eq!(guarded["token"], "pagination-cursor-27");
        assert_eq!(guarded["access_token"], "[REDACTED]");
        assert_eq!(guarded["secret"], "surprise party");
        assert_eq!(guarded["cookie"], "chocolate chip");
        assert_eq!(guarded["public_database"], "postgres://localhost/example");
        assert_eq!(guarded["spoken"], "Completed with token=domain-value");
        assert_eq!(guarded["basic_auth"], "Basic [REDACTED]");
    }

    #[test]
    fn default_stack_provider_guard_handles_adversarial_json_depth_without_recursive_clone() {
        let mut source = Value::String("Bearer abcdefghijklmnop".to_string());
        for _ in 0..2_048 {
            source = Value::Array(vec![source]);
        }

        let guarded = sanitize_json_for_provider(&source);
        assert!(
            crate::json_traversal::inspect_json(&guarded).max_depth
                <= crate::json_traversal::MAX_RETAINED_JSON_DEPTH
        );
        assert!(
            crate::json_traversal::canonical_json_bytes(&guarded)
                .unwrap()
                .windows(crate::json_traversal::DEPTH_LIMIT_SENTINEL.len())
                .any(|window| {
                    window == crate::json_traversal::DEPTH_LIMIT_SENTINEL.as_bytes()
                })
        );
        crate::json_traversal::discard_json_iteratively(source);
    }

    #[test]
    fn provider_json_guard_enforces_exact_node_and_encoded_byte_admission() {
        let source = Value::Array(vec![Value::Null; 8]);
        let encoded_bytes = exact_json_encoded_len(&source);
        assert!(provider_json_is_admitted(&source, 9, encoded_bytes));
        assert!(!provider_json_is_admitted(&source, 8, encoded_bytes));
        assert!(!provider_json_is_admitted(
            &source,
            9,
            encoded_bytes.saturating_sub(1)
        ));

        assert_eq!(
            sanitize_json_for_provider_with_limits(&source, 8, encoded_bytes),
            Value::String(PROVIDER_JSON_SAFETY_OMISSION.to_string())
        );
        assert_eq!(
            sanitize_json_for_provider_owned_with_limits(source, 9, encoded_bytes - 1),
            Value::String(PROVIDER_JSON_SAFETY_OMISSION.to_string())
        );
    }

    #[test]
    fn provider_json_guard_reapplies_the_byte_ceiling_after_redaction_expands_values() {
        let source = serde_json::json!({"password": ""});
        let source_bytes = exact_json_encoded_len(&source);
        assert_eq!(
            sanitize_json_for_provider_with_limits(&source, 8, source_bytes),
            Value::String(PROVIDER_JSON_SAFETY_OMISSION.to_string()),
            "borrowed sanitation must not expand beyond its provider boundary",
        );
        assert_eq!(
            sanitize_json_for_provider_owned_with_limits(source, 8, source_bytes),
            Value::String(PROVIDER_JSON_SAFETY_OMISSION.to_string()),
            "owned sanitation must enforce the same post-redaction ceiling",
        );
    }

    #[test]
    fn owned_provider_json_guard_reuses_the_admitted_array_allocation() {
        let source = Value::Array(vec![Value::Null; 64]);
        let source_ptr = source.as_array().expect("array fixture").as_ptr();
        let guarded = sanitize_json_for_provider_owned(source);
        assert_eq!(
            guarded.as_array().expect("sanitized array").as_ptr(),
            source_ptr,
            "owned sanitation must not clone an admitted provider payload"
        );
    }

    #[test]
    fn provider_credential_field_allowlist_is_exact_not_fuzzy() {
        let guarded = sanitize_json_for_provider(&serde_json::json!({
            "access_token": "credential",
            "accessToken": "credential",
            "refresh-token": "credential",
            "password": { "nested": "credential" },
            "token": "domain-token",
            "access_token_label": "shown to the user",
            "password_policy": "minimum 12 characters",
            "secret": "surprise party",
            "authorization": "approved by the owner",
            "cookie": "chocolate chip"
        }));

        assert_eq!(guarded["access_token"], "[REDACTED]");
        assert_eq!(guarded["accessToken"], "[REDACTED]");
        assert_eq!(guarded["refresh-token"], "[REDACTED]");
        assert_eq!(guarded["password"], "[REDACTED]");
        assert_eq!(guarded["token"], "domain-token");
        assert_eq!(guarded["access_token_label"], "shown to the user");
        assert_eq!(guarded["password_policy"], "minimum 12 characters");
        assert_eq!(guarded["secret"], "surprise party");
        assert_eq!(guarded["authorization"], "approved by the owner");
        assert_eq!(guarded["cookie"], "chocolate chip");
    }

    #[test]
    fn provider_protocol_guard_preserves_credential_like_prose() {
        for prose in [
            "A basic authentication tutorial",
            "The bearer instrument is negotiable",
            "password: requirements are documented",
            "authorization: approved by the owner",
            "token=domain-value",
        ] {
            assert_eq!(sanitize_text_for_provider(prose), prose);
        }

        let legacy_json = sanitize_text_for_provider(
            r#"{"password":"plain-secret","authorization":"Bearer abcdefghijklmnop","token":"cursor-4"}"#,
        );
        let legacy_json: Value = serde_json::from_str(&legacy_json).unwrap();
        assert_eq!(legacy_json["password"], "[REDACTED]");
        assert_eq!(legacy_json["authorization"], "Bearer [REDACTED]");
        assert_eq!(legacy_json["token"], "cursor-4");
    }

    #[test]
    fn provider_text_guard_rejects_deep_json_before_serde_on_small_stack() {
        warm_provider_sanitizer();
        std::thread::Builder::new()
            .name("provider-text-json-admission-small-stack".to_string())
            .stack_size(512 * 1024)
            .spawn(|| {
                let depth = 20_000;
                let mut encoded = "[".repeat(depth);
                encoded.push_str(r#""Bearer abcdefghijklmnop""#);
                encoded.push_str(&"]".repeat(depth));
                let sanitized = sanitize_text_for_provider(&encoded);
                assert_eq!(sanitized, PROVIDER_JSON_SAFETY_OMISSION);
                assert!(!sanitized.contains("abcdefghijklmnop"));
            })
            .unwrap()
            .join()
            .expect("encoded JSON admission must fit a 512 KiB stack");
    }

    #[test]
    fn provider_text_guard_enforces_the_exact_encoded_byte_boundary() {
        let encoded = r#"{"token":"cursor-4"}"#;
        assert_eq!(
            sanitize_text_for_provider_with_byte_limit(encoded, encoded.len()),
            encoded
        );

        let over_limit = format!("{encoded} ");
        assert_eq!(
            sanitize_text_for_provider_with_byte_limit(&over_limit, encoded.len()),
            PROVIDER_JSON_SAFETY_OMISSION
        );
        assert_eq!(
            sanitize_text_for_provider_with_byte_limit("ordinary prose", 5),
            PROVIDER_TEXT_SAFETY_OMISSION
        );
        assert_eq!(
            sanitize_text_for_provider_with_byte_limit("{malformed", 64),
            "{malformed"
        );

        let expanding_redaction = r#"{"password":""}"#;
        assert_eq!(
            sanitize_text_for_provider_with_byte_limit(
                expanding_redaction,
                expanding_redaction.len(),
            ),
            PROVIDER_JSON_SAFETY_OMISSION,
            "sanitization must not grow a provider-bound document past its admitted byte ceiling",
        );
    }

    const VARIANT_SENTINEL: &str = "PLACEHOLDER-SENTINEL-VALUE-0123456789";

    fn sentinel_values(value: &str) -> KnownSecretValues {
        KnownSecretValues::from(HashMap::from([("k".to_string(), value.to_string())]))
    }

    #[test]
    fn known_secret_values_debug_never_prints_a_value() {
        let rendered = format!("{:?}", sentinel_values(VARIANT_SENTINEL));
        assert!(
            !rendered.contains(VARIANT_SENTINEL),
            "Debug leaked a value: {rendered}"
        );
        assert!(
            rendered.contains("k"),
            "the name is the diagnostic and should survive"
        );
        assert!(rendered.contains(&format!("<{} bytes>", VARIANT_SENTINEL.len())));
    }

    #[test]
    fn known_secret_values_debug_stays_redacted_inside_a_container() {
        let nested = vec![sentinel_values(VARIANT_SENTINEL)];
        let rendered = format!("{nested:?}");
        assert!(
            !rendered.contains(VARIANT_SENTINEL),
            "a container's Debug leaked a value"
        );
    }

    #[test]
    fn known_secret_values_zeroize_on_drop() {
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Arc;
        let observed = Arc::new(AtomicBool::new(false));
        {
            let mut values = sentinel_values(VARIANT_SENTINEL);
            values.install_drop_probe(Arc::clone(&observed));
        }
        assert!(
            observed.load(Ordering::SeqCst),
            "values were not zeroized on drop"
        );
    }

    #[test]
    fn known_secret_values_are_sealed() {
        use static_assertions::assert_not_impl_any;
        assert_not_impl_any!(KnownSecretValues: Clone, Serialize);
    }

    // ---- §15 item 3: cookie Debug is redacted --------------------------------

    fn placeholder_cookie() -> CookieWithMetadata {
        CookieWithMetadata {
            name: "session".to_string(),
            value: VARIANT_SENTINEL.to_string(),
            domain: "example.com".to_string(),
            path: "/".to_string(),
            secure: true,
            http_only: true,
            same_site: SameSite::Lax,
            expires: None,
        }
    }

    #[test]
    fn cookie_debug_never_prints_the_value() {
        let rendered = format!("{:?}", placeholder_cookie());
        assert!(
            !rendered.contains(VARIANT_SENTINEL),
            "CookieWithMetadata Debug leaked: {rendered}"
        );
        assert!(rendered.contains("session"));
        assert!(rendered.contains("example.com"));
        assert!(rendered.contains(&format!("<{} bytes>", VARIANT_SENTINEL.len())));
    }

    #[test]
    fn cookie_debug_stays_redacted_inside_a_container() {
        let rendered = format!("{:?}", vec![placeholder_cookie()]);
        assert!(!rendered.contains(VARIANT_SENTINEL));
    }
}
