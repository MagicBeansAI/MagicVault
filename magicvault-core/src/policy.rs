use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::Mutex;

use chrono::{NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use zeroize::Zeroize;

use super::InjectionTarget;

const HTTP_PROVISIONED_POLICY_ROUTES: &[&str] = &[
    "http:get",
    "http:post",
    "http:put",
    "http:patch",
    "http:delete",
    "http:head",
    "http:options",
];

// Browser automation now enters as the `browser` skill/capability. The outer
// runtime authorizes and audits that pack invocation as `browser:execute`;
// command-level agent-browser argv is resolved inside the browser loop.
const BROWSER_PROVISIONED_POLICY_ROUTES: &[&str] = &["browser:execute"];

/// Policy attached to provisioned secrets.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct SecretPolicy {
    #[serde(default)]
    pub allowed_tools: Vec<String>,
    #[serde(default)]
    pub allowed_domains: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_uses_per_day: Option<u32>,
    #[serde(default)]
    pub requires_approval: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProvisionedPolicyRouteCatalog {
    pub http: Vec<String>,
    pub browser: Vec<String>,
}

impl ProvisionedPolicyRouteCatalog {
    pub fn for_target(&self, target: &InjectionTarget) -> Vec<String> {
        match target {
            InjectionTarget::Header { .. } | InjectionTarget::FormFields(_) => self.http.clone(),
            InjectionTarget::Cookies(_) => {
                let mut routes = self.http.clone();
                routes.extend(self.browser.clone());
                routes
            },
            InjectionTarget::Inline => Vec::new(),
        }
    }
}

/// Most hosts one request may name as a domain set.
pub const MAX_REQUESTED_DOMAINS: usize = 16;

/// The all-sites marker: a bare `*` in `SecretPolicy::allowed_domains` admits
/// every domain, and `domains: ["*"]` on the wire is `RequestedDomains::Any`.
pub const ANY_DOMAIN: &str = "*";

/// Why a requested domain set was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DomainScopeError {
    #[error("a requested domain set must name at least one host")]
    Empty,
    #[error("a requested domain set may name at most {MAX_REQUESTED_DOMAINS} hosts")]
    TooMany,
    #[error("requested domain '{0}' is not a DNS host name (no scheme, port, path or wildcard)")]
    InvalidHost(String),
    #[error("a request carries both a single domain and a domain set")]
    Conflicting,
}

/// Two or more distinct hosts, lowercased, sorted and deduplicated.
///
/// Only `RequestedDomains::hosts` builds one, so every value is canonical and
/// two sets naming the same hosts in any order or case compare equal.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DomainSet(Vec<String>);

impl DomainSet {
    pub fn hosts(&self) -> &[String] {
        &self.0
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// The target domains one runtime action may reach.
///
/// - `None`: no target domain. Denied for a domain-scoped secret.
/// - `One`: the legacy single domain. Grants, approvals and audits carry it
///   as given, so existing ones keep their identity. For a domain-scoped
///   secret the policy check lowercases it and denies it unless it is a DNS
///   host name.
/// - `Set`: every host the action may reach. Allowed only when each one
///   matches some `allowed_domains` pattern.
/// - `Any`: every host the action may name, with no filtering by the vault —
///   in particular no public/private distinction. Allowed only when the policy
///   is unrestricted (empty `allowed_domains`) or lists the bare `*`. Blocking
///   loopback, link-local and private addresses (SSRF) is the consumer's job.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub enum RequestedDomains {
    #[default]
    None,
    One(String),
    Set(DomainSet),
    Any,
}

impl RequestedDomains {
    /// The legacy `Option<&str>` domain, unchanged.
    pub fn from_legacy(domain: Option<&str>) -> Self {
        match domain {
            Some(domain) => Self::One(domain.to_owned()),
            None => Self::None,
        }
    }

    /// Canonicalize a host list: lowercase, validate, sort and dedupe.
    ///
    /// One distinct host becomes `One`, so a set of one is the single-domain
    /// request. `*` is refused here; ask for all sites with `Any`.
    pub fn hosts<I, S>(hosts: I) -> Result<Self, DomainScopeError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let mut canonical = BTreeSet::new();
        for host in hosts {
            let host = host.as_ref().to_ascii_lowercase();
            if !is_dns_host(&host) {
                return Err(DomainScopeError::InvalidHost(display_host(&host)));
            }
            canonical.insert(host);
            if canonical.len() > MAX_REQUESTED_DOMAINS {
                return Err(DomainScopeError::TooMany);
            }
        }
        let mut canonical = canonical.into_iter().collect::<Vec<_>>();
        match canonical.len() {
            0 => Err(DomainScopeError::Empty),
            1 => Ok(Self::One(canonical.remove(0))),
            _ => Ok(Self::Set(DomainSet(canonical))),
        }
    }

    pub fn is_none(&self) -> bool {
        matches!(self, Self::None)
    }

    /// Read the wire pair (`domain`, `domains`) that request, challenge,
    /// binding and audit types carry. `domains: ["*"]` is `Any`.
    pub fn from_wire(domain: Option<&str>, domains: &[String]) -> Result<Self, DomainScopeError> {
        if domains.is_empty() {
            return Ok(Self::from_legacy(domain));
        }
        if domain.is_some() {
            return Err(DomainScopeError::Conflicting);
        }
        if domains.len() == 1 && domains[0] == ANY_DOMAIN {
            return Ok(Self::Any);
        }
        Self::hosts(domains)
    }

    /// The wire pair for this value: `One` keeps the legacy `domain` field and
    /// leaves `domains` empty, so single-domain JSON is byte-identical.
    pub fn to_wire(&self) -> (Option<String>, Vec<String>) {
        match self {
            Self::None => (None, Vec::new()),
            Self::One(domain) => (Some(domain.clone()), Vec::new()),
            Self::Set(set) => (None, set.0.clone()),
            Self::Any => (None, vec![ANY_DOMAIN.to_owned()]),
        }
    }
}

/// Longest host text copied into an error or deny reason.
const MAX_DISPLAYED_HOST_BYTES: usize = 128;

/// Caller-supplied host text made safe for errors, deny reasons and logs:
/// truncated to `MAX_DISPLAYED_HOST_BYTES` on a character boundary, with
/// control characters (and quotes/backslashes) escaped.
fn display_host(host: &str) -> String {
    let mut end = host.len().min(MAX_DISPLAYED_HOST_BYTES);
    while !host.is_char_boundary(end) {
        end -= 1;
    }
    let mut shown = host[..end].escape_debug().to_string();
    if end < host.len() {
        shown.push_str("...");
    }
    shown
}

/// A lowercase DNS host name: LDH labels of 1-63 bytes, at most 253 bytes in
/// all, no leading/trailing hyphen, no scheme, port, path or wildcard.
fn is_dns_host(host: &str) -> bool {
    if host.is_empty() || host.len() > 253 {
        return false;
    }
    host.split('.').all(|label| {
        !label.is_empty()
            && label.len() <= 63
            && !label.starts_with('-')
            && !label.ends_with('-')
            && label
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    })
}

/// Canonical runtime access request used for secret policy checks.
///
/// `domain` is the legacy single target domain. `domains` carries a domain
/// set, or `["*"]` for all sites; it is omitted when empty, so a request
/// serialized before it existed still deserializes and a single-domain request
/// serializes exactly as before. At most one of the two is set.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AccessRequest {
    pub secret_id: String,
    pub tool: String,
    pub action: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub domain: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub domains: Vec<String>,
    pub requested_at: i64,
}

impl AccessRequest {
    pub fn new(
        secret_id: impl Into<String>,
        tool: impl Into<String>,
        action: impl Into<String>,
        domain: Option<String>,
    ) -> Self {
        Self {
            secret_id: secret_id.into(),
            tool: tool.into(),
            action: action.into(),
            domain,
            domains: Vec::new(),
            requested_at: Utc::now().timestamp(),
        }
    }

    /// A request for a domain set, all sites, one domain or none.
    pub fn scoped(
        secret_id: impl Into<String>,
        tool: impl Into<String>,
        action: impl Into<String>,
        domains: &RequestedDomains,
    ) -> Self {
        let (domain, domains) = domains.to_wire();
        Self {
            secret_id: secret_id.into(),
            tool: tool.into(),
            action: action.into(),
            domain,
            domains,
            requested_at: Utc::now().timestamp(),
        }
    }

    pub fn requested_domains(&self) -> Result<RequestedDomains, DomainScopeError> {
        RequestedDomains::from_wire(self.domain.as_deref(), &self.domains)
    }

    pub fn tool_action(&self) -> String {
        format!("{}:{}", self.tool, self.action)
    }
}

/// Single-use approval challenge emitted when a secret needs user confirmation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ApprovalChallenge {
    pub id: String,
    pub secret_id: String,
    pub tool: String,
    pub action: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub domain: Option<String>,
    /// A domain set, or `["*"]` for all sites; see `AccessRequest::domains`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub domains: Vec<String>,
    pub expires_at: i64,
}

impl ApprovalChallenge {
    pub fn requested_domains(&self) -> Result<RequestedDomains, DomainScopeError> {
        RequestedDomains::from_wire(self.domain.as_deref(), &self.domains)
    }
}

/// Policy decision returned by compiled secret checks.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum PolicyResult {
    Allowed,
    Denied { reason: String },
    NeedsApproval { challenge: ApprovalChallenge },
}

/// Payload returned after redeeming a single-use secret grant.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GrantBinding {
    pub tool: String,
    pub action: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub domain: Option<String>,
    /// A domain set, or `["*"]` for all sites; see `AccessRequest::domains`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub domains: Vec<String>,
    #[serde(skip)]
    delegated_authority: Option<DelegatedGrantAuthority>,
}

/// Additional exact authority carried only by generic delegated-credential grants.
/// Legacy secret grants retain their existing tool/action/domain semantics.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DelegatedGrantAuthority {
    principal: String,
    workspace: String,
    provider: String,
    agent_id: String,
}

impl DelegatedGrantAuthority {
    pub fn new(
        principal: impl Into<String>,
        workspace: impl Into<String>,
        provider: impl Into<String>,
        agent_id: impl Into<String>,
    ) -> Self {
        Self {
            principal: principal.into(),
            workspace: workspace.into(),
            provider: provider.into(),
            agent_id: agent_id.into(),
        }
    }
}

impl GrantBinding {
    pub fn from_request(request: &AccessRequest) -> Self {
        Self {
            tool: request.tool.clone(),
            action: request.action.clone(),
            domain: request.domain.clone(),
            domains: request.domains.clone(),
            delegated_authority: None,
        }
    }

    pub fn delegated(request: &AccessRequest, authority: DelegatedGrantAuthority) -> Self {
        Self {
            tool: request.tool.clone(),
            action: request.action.clone(),
            domain: request.domain.clone(),
            domains: request.domains.clone(),
            delegated_authority: Some(authority),
        }
    }

    pub fn requested_domains(&self) -> Result<RequestedDomains, DomainScopeError> {
        RequestedDomains::from_wire(self.domain.as_deref(), &self.domains)
    }

    /// Legacy single-domain match. A binding without a domain matches any
    /// domain; a binding for a domain set or all sites matches no legacy
    /// query, because it redeems only for exactly that set.
    pub fn matches(&self, tool: &str, action: &str, domain: Option<&str>) -> bool {
        if self.tool != tool || self.action != action || !self.domains.is_empty() {
            return false;
        }
        match self.domain.as_deref() {
            Some(expected_domain) => domain == Some(expected_domain),
            None => true,
        }
    }

    /// Exact match on tool, action and domain scope, like the bound batch
    /// path: unlike the legacy `matches`, a binding without a domain matches
    /// only `RequestedDomains::None`.
    pub fn matches_domains(&self, tool: &str, action: &str, domains: &RequestedDomains) -> bool {
        self.scope_matches(tool, action, domains)
    }

    fn scope_matches(&self, tool: &str, action: &str, domains: &RequestedDomains) -> bool {
        self.tool == tool
            && self.action == action
            && self
                .requested_domains()
                .is_ok_and(|bound| &bound == domains)
    }

    /// A `Secret`/`SecretScoped` expectation: exact scope, and never a
    /// delegated grant, which only its own authority may redeem.
    fn matches_secret_exact(&self, tool: &str, action: &str, domains: &RequestedDomains) -> bool {
        self.delegated_authority.is_none() && self.scope_matches(tool, action, domains)
    }

    pub fn matches_delegated(
        &self,
        tool: &str,
        action: &str,
        domain: Option<&str>,
        authority: &DelegatedGrantAuthority,
    ) -> bool {
        self.matches_delegated_domains(
            tool,
            action,
            &RequestedDomains::from_legacy(domain),
            authority,
        )
    }

    /// Exact delegated match on tool, action, domain scope and authority.
    pub fn matches_delegated_domains(
        &self,
        tool: &str,
        action: &str,
        domains: &RequestedDomains,
        authority: &DelegatedGrantAuthority,
    ) -> bool {
        self.scope_matches(tool, action, domains)
            && self.delegated_authority.as_ref() == Some(authority)
    }
}

/// Payload returned after redeeming a single-use secret grant.
pub struct RedemptionPayload {
    secret_id: String,
    fields: HashMap<String, String>,
    target: InjectionTarget,
    binding: GrantBinding,
    #[cfg(test)]
    drop_probe: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
}

impl RedemptionPayload {
    fn new(
        secret_id: String,
        fields: HashMap<String, String>,
        target: InjectionTarget,
        binding: GrantBinding,
    ) -> Self {
        Self {
            secret_id,
            fields,
            target,
            binding,
            #[cfg(test)]
            drop_probe: None,
        }
    }

    pub fn secret_id(&self) -> &str {
        &self.secret_id
    }

    pub fn fields(&self) -> &HashMap<String, String> {
        &self.fields
    }

    pub fn target(&self) -> &InjectionTarget {
        &self.target
    }

    pub fn binding(&self) -> &GrantBinding {
        &self.binding
    }

    pub fn with_canonical_value<T>(
        &self,
        consumer: impl for<'value> FnOnce(&'value str) -> T,
    ) -> Option<T> {
        if self.fields.len() != 1 {
            return None;
        }
        self.fields.get("value").map(|value| consumer(value))
    }

    #[cfg(test)]
    fn install_drop_probe(&mut self, probe: std::sync::Arc<std::sync::atomic::AtomicBool>) {
        self.drop_probe = Some(probe);
    }
}

impl Drop for RedemptionPayload {
    fn drop(&mut self) {
        for value in self.fields.values_mut() {
            value.zeroize();
        }
        #[cfg(test)]
        if let Some(probe) = &self.drop_probe {
            use std::sync::atomic::Ordering;
            probe.store(
                self.fields
                    .values()
                    .all(|value| value.as_bytes().iter().all(|byte| *byte == 0)),
                Ordering::SeqCst,
            );
        }
    }
}

/// Tracks per-secret daily usage for provisioned policies.
#[derive(Debug, Default)]
pub struct UsageTracker {
    entries: Mutex<HashMap<(String, NaiveDate), u32>>,
}

impl UsageTracker {
    pub fn is_within_limit(&self, secret_id: &str, at_ts: i64, limit: u32) -> bool {
        let date = date_key(at_ts);
        let guard = self.entries.lock().expect("usage tracker lock poisoned");
        guard
            .get(&(secret_id.to_string(), date))
            .copied()
            .unwrap_or_default()
            < limit
    }

    pub fn record_use(&self, secret_id: &str, at_ts: i64) {
        let _ = self.try_record_use(secret_id, at_ts, None);
    }

    /// Atomically enforce an optional daily limit and record one completed use.
    ///
    /// Keeping the comparison and increment under one lock prevents concurrent
    /// credential preparations from all consuming the same remaining policy slot.
    pub fn try_record_use(&self, secret_id: &str, at_ts: i64, limit: Option<u32>) -> bool {
        let date = date_key(at_ts);
        let mut guard = self.entries.lock().expect("usage tracker lock poisoned");
        let count = guard.entry((secret_id.to_string(), date)).or_default();
        if limit.is_some_and(|limit| *count >= limit) {
            return false;
        }
        *count = count.saturating_add(1);
        true
    }
}

#[derive(Debug, Clone)]
struct ApprovedRequest {
    fingerprint: RequestFingerprint,
    expires_at: i64,
}

/// Tracks pending approval challenges plus granted approvals waiting to be consumed.
#[derive(Debug)]
pub struct ApprovalTable {
    ttl_secs: i64,
    pending: Mutex<HashMap<String, ApprovalChallenge>>,
    approved: Mutex<Vec<ApprovedRequest>>,
}

impl ApprovalTable {
    pub fn new(ttl_secs: i64) -> Self {
        Self {
            ttl_secs,
            pending: Mutex::new(HashMap::new()),
            approved: Mutex::new(Vec::new()),
        }
    }

    pub fn issue(&self, request: &AccessRequest) -> ApprovalChallenge {
        self.reap_expired();
        let challenge = ApprovalChallenge {
            id: Uuid::new_v4().to_string(),
            secret_id: request.secret_id.clone(),
            tool: request.tool.clone(),
            action: request.action.clone(),
            domain: request.domain.clone(),
            domains: request.domains.clone(),
            expires_at: request.requested_at + self.ttl_secs,
        };
        self.pending
            .lock()
            .expect("approval pending lock poisoned")
            .insert(challenge.id.clone(), challenge.clone());
        challenge
    }

    pub fn grant(&self, challenge_id: &str) -> Option<ApprovalChallenge> {
        self.reap_expired();
        let challenge = self
            .pending
            .lock()
            .expect("approval pending lock poisoned")
            .remove(challenge_id);
        let Some(challenge) = challenge else {
            return None;
        };
        let Ok(domains) = challenge.requested_domains() else {
            return None;
        };

        self.approved
            .lock()
            .expect("approval granted lock poisoned")
            .push(ApprovedRequest {
                fingerprint: RequestFingerprint::new(
                    &challenge.secret_id,
                    &challenge.tool,
                    &challenge.action,
                    &domains,
                ),
                expires_at: challenge.expires_at,
            });
        Some(challenge)
    }

    /// Record an approval for exactly this request. A request whose domain
    /// set is malformed is not recorded.
    pub fn approve_request(&self, request: &AccessRequest) {
        self.reap_expired();
        let Ok(domains) = request.requested_domains() else {
            return;
        };
        self.approved
            .lock()
            .expect("approval granted lock poisoned")
            .push(ApprovedRequest {
                fingerprint: RequestFingerprint::new(
                    &request.secret_id,
                    &request.tool,
                    &request.action,
                    &domains,
                ),
                expires_at: request.requested_at + self.ttl_secs,
            });
    }

    pub fn consume(&self, request: &AccessRequest) -> bool {
        self.reap_expired();
        let Ok(domains) = request.requested_domains() else {
            return false;
        };
        let fingerprint =
            RequestFingerprint::new(&request.secret_id, &request.tool, &request.action, &domains);
        let mut guard = self
            .approved
            .lock()
            .expect("approval granted lock poisoned");
        if let Some(index) = guard
            .iter()
            .position(|entry| entry.fingerprint == fingerprint)
        {
            guard.remove(index);
            true
        } else {
            false
        }
    }

    pub fn list_pending(&self) -> Vec<ApprovalChallenge> {
        self.reap_expired();
        let mut challenges = self
            .pending
            .lock()
            .expect("approval pending lock poisoned")
            .values()
            .cloned()
            .collect::<Vec<_>>();
        challenges.sort_by(|left, right| {
            left.expires_at
                .cmp(&right.expires_at)
                .then(left.secret_id.cmp(&right.secret_id))
                .then(left.id.cmp(&right.id))
        });
        challenges
    }

    fn reap_expired(&self) {
        let now = Utc::now().timestamp();
        self.pending
            .lock()
            .expect("approval pending lock poisoned")
            .retain(|_, challenge| challenge.expires_at > now);
        self.approved
            .lock()
            .expect("approval granted lock poisoned")
            .retain(|entry| entry.expires_at > now);
    }
}

impl Default for ApprovalTable {
    fn default() -> Self {
        Self::new(300)
    }
}

pub fn provisioned_policy_route_catalog() -> ProvisionedPolicyRouteCatalog {
    ProvisionedPolicyRouteCatalog {
        http: HTTP_PROVISIONED_POLICY_ROUTES
            .iter()
            .map(|route| route.to_string())
            .collect(),
        browser: BROWSER_PROVISIONED_POLICY_ROUTES
            .iter()
            .map(|route| route.to_string())
            .collect(),
    }
}

pub fn provisioned_policy_routes_for_target(target: &InjectionTarget) -> &'static [&'static str] {
    match target {
        InjectionTarget::Header { .. } | InjectionTarget::FormFields(_) => {
            HTTP_PROVISIONED_POLICY_ROUTES
        },
        InjectionTarget::Cookies(_) => &[
            "http:get",
            "http:post",
            "http:put",
            "http:patch",
            "http:delete",
            "http:head",
            "http:options",
            "browser:execute",
        ],
        InjectionTarget::Inline => &[],
    }
}

pub fn find_unsupported_provisioned_policy_routes(
    target: &InjectionTarget,
    routes: &[String],
) -> Vec<String> {
    let supported = provisioned_policy_routes_for_target(target);
    routes
        .iter()
        .filter(|route| {
            !supported
                .iter()
                .any(|supported_route| supported_route == route)
        })
        .cloned()
        .collect()
}

struct GrantRecord {
    payload: RedemptionPayload,
    expires_at: i64,
}

/// Tracks short-lived, single-use grants for future adapter-based access.
#[derive(Default)]
pub struct GrantTable {
    grants: Mutex<HashMap<String, GrantRecord>>,
}

pub const MAX_ATOMIC_GRANT_REDEMPTIONS: usize = 64;

pub enum GrantBindingExpectation<'a> {
    Secret {
        tool: &'a str,
        action: &'a str,
        domain: Option<&'a str>,
    },
    Delegated {
        tool: &'a str,
        action: &'a str,
        domain: Option<&'a str>,
        authority: &'a DelegatedGrantAuthority,
    },
    /// `Secret` for a domain set or all sites: the grant must carry exactly
    /// `domains`.
    SecretScoped {
        tool: &'a str,
        action: &'a str,
        domains: &'a RequestedDomains,
    },
    /// `Delegated` for a domain set or all sites.
    DelegatedScoped {
        tool: &'a str,
        action: &'a str,
        domains: &'a RequestedDomains,
        authority: &'a DelegatedGrantAuthority,
    },
}

pub struct BoundGrantRedemption<'a> {
    pub token: &'a str,
    pub expected: GrantBindingExpectation<'a>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoundGrantRedemptionError {
    EmptyBatch,
    BatchTooLarge,
    DuplicateToken,
    Expired,
    MissingOrAlreadyRedeemed,
    BindingMismatch,
    Internal,
}

impl GrantTable {
    pub fn issue_grant(
        &self,
        secret_id: impl Into<String>,
        fields: HashMap<String, String>,
        target: InjectionTarget,
        binding: GrantBinding,
        ttl_secs: i64,
    ) -> String {
        self.reap_expired();
        let grant_id = Uuid::new_v4().to_string();
        let now = Utc::now().timestamp();
        self.grants
            .lock()
            .expect("grant table lock poisoned")
            .insert(
                grant_id.clone(),
                GrantRecord {
                    payload: RedemptionPayload::new(secret_id.into(), fields, target, binding),
                    expires_at: now + ttl_secs,
                },
            );
        grant_id
    }

    pub fn redeem_grant(&self, grant_id: &str) -> Option<RedemptionPayload> {
        self.reap_expired();
        self.grants
            .lock()
            .expect("grant table lock poisoned")
            .remove(grant_id)
            .map(|record| record.payload)
    }

    pub fn redeem_bound_batch(
        &self,
        requests: &[BoundGrantRedemption<'_>],
    ) -> Result<Vec<RedemptionPayload>, BoundGrantRedemptionError> {
        if requests.is_empty() {
            return Err(BoundGrantRedemptionError::EmptyBatch);
        }
        if requests.len() > MAX_ATOMIC_GRANT_REDEMPTIONS {
            return Err(BoundGrantRedemptionError::BatchTooLarge);
        }
        let mut unique_tokens = HashSet::with_capacity(requests.len());
        if requests
            .iter()
            .any(|request| !unique_tokens.insert(request.token))
        {
            return Err(BoundGrantRedemptionError::DuplicateToken);
        }

        let now = Utc::now().timestamp();
        let mut grants = self
            .grants
            .lock()
            .map_err(|_| BoundGrantRedemptionError::Internal)?;
        for request in requests {
            let Some(record) = grants.get(request.token) else {
                return Err(BoundGrantRedemptionError::MissingOrAlreadyRedeemed);
            };
            if record.expires_at <= now {
                grants.retain(|_, record| record.expires_at > now);
                return Err(BoundGrantRedemptionError::Expired);
            }
            let matches = match &request.expected {
                GrantBindingExpectation::Secret {
                    tool,
                    action,
                    domain,
                } => record.payload.binding().matches_secret_exact(
                    tool,
                    action,
                    &RequestedDomains::from_legacy(*domain),
                ),
                GrantBindingExpectation::Delegated {
                    tool,
                    action,
                    domain,
                    authority,
                } => record
                    .payload
                    .binding()
                    .matches_delegated(tool, action, *domain, authority),
                GrantBindingExpectation::SecretScoped {
                    tool,
                    action,
                    domains,
                } => record
                    .payload
                    .binding()
                    .matches_secret_exact(tool, action, domains),
                GrantBindingExpectation::DelegatedScoped {
                    tool,
                    action,
                    domains,
                    authority,
                } => record
                    .payload
                    .binding()
                    .matches_delegated_domains(tool, action, domains, authority),
            };
            if !matches {
                return Err(BoundGrantRedemptionError::BindingMismatch);
            }
        }

        let mut removed = Vec::with_capacity(requests.len());
        for request in requests {
            let Some(record) = grants.remove(request.token) else {
                for (token, record) in removed {
                    grants.insert(token, record);
                }
                return Err(BoundGrantRedemptionError::Internal);
            };
            removed.push((request.token.to_owned(), record));
        }
        Ok(removed
            .into_iter()
            .map(|(_, record)| record.payload)
            .collect())
    }

    pub fn discard_grants(&self, grant_ids: &[&str]) {
        if let Ok(mut grants) = self.grants.lock() {
            for grant_id in grant_ids.iter().take(MAX_ATOMIC_GRANT_REDEMPTIONS) {
                grants.remove(*grant_id);
            }
        }
    }

    pub fn reap_expired(&self) {
        let now = Utc::now().timestamp();
        self.grants
            .lock()
            .expect("grant table lock poisoned")
            .retain(|_, record| record.expires_at > now);
    }
}

/// Evaluate a provisioned secret policy against the canonical runtime action.
pub fn check_policy(
    policy: &SecretPolicy,
    request: &AccessRequest,
    usage: &UsageTracker,
    approvals: &ApprovalTable,
) -> PolicyResult {
    let route = request.tool_action();
    if !policy.allowed_tools.is_empty() && !policy.allowed_tools.iter().any(|item| item == &route) {
        return PolicyResult::Denied {
            reason: format!("tool/action '{}' is not allowed for this secret", route),
        };
    }

    let requested = match request.requested_domains() {
        Ok(requested) => requested,
        Err(error) => {
            return PolicyResult::Denied {
                reason: format!("invalid target domains: {error}"),
            };
        },
    };

    if !policy.allowed_domains.is_empty() {
        match &requested {
            RequestedDomains::None => {
                return PolicyResult::Denied {
                    reason: "secret is domain-scoped but runtime action had no target domain"
                        .into(),
                };
            },
            // The legacy single domain was never validated; a domain-scoped
            // secret now needs a real host so `*.suffix` cannot be satisfied
            // by text such as `evil.test/.example.com`.
            RequestedDomains::One(domain) => {
                let host = domain.to_ascii_lowercase();
                if !is_dns_host(&host) {
                    return PolicyResult::Denied {
                        reason: format!(
                            "target domain '{}' is not a DNS host name",
                            display_host(domain)
                        ),
                    };
                }
                if !policy
                    .allowed_domains
                    .iter()
                    .any(|pattern| domain_matches(pattern, &host))
                {
                    return PolicyResult::Denied {
                        reason: format!("domain '{}' is not allowed for this secret", host),
                    };
                }
            },
            // All-of: the action may reach any host in the set.
            RequestedDomains::Set(set) => {
                if let Some(host) = set.hosts().iter().find(|host| {
                    !policy
                        .allowed_domains
                        .iter()
                        .any(|pattern| domain_matches(pattern, host))
                }) {
                    return PolicyResult::Denied {
                        reason: format!("domain '{}' is not allowed for this secret", host),
                    };
                }
            },
            // `*.suffix` patterns do not admit all sites; only a bare `*` does.
            RequestedDomains::Any => {
                if !policy
                    .allowed_domains
                    .iter()
                    .any(|pattern| pattern == ANY_DOMAIN)
                {
                    return PolicyResult::Denied {
                        reason: "all target domains were requested but this secret is \
                                 domain-scoped without '*'"
                            .into(),
                    };
                }
            },
        }
    }

    if let Some(limit) = policy.max_uses_per_day {
        if !usage.is_within_limit(&request.secret_id, request.requested_at, limit) {
            return PolicyResult::Denied {
                reason: format!("daily usage limit ({limit}) exceeded"),
            };
        }
    }

    if policy.requires_approval && !approvals.consume(request) {
        return PolicyResult::NeedsApproval {
            challenge: approvals.issue(request),
        };
    }

    PolicyResult::Allowed
}

/// Approval identity. `Legacy` is the pre-domain-set string, byte-identical
/// for no-domain and single-domain requests; a set or all-sites request is a
/// separate variant, so no legacy domain string can collide with it.
#[derive(Debug, Clone, PartialEq, Eq)]
enum RequestFingerprint {
    Legacy(String),
    Scoped {
        secret_id: String,
        tool: String,
        action: String,
        domains: RequestedDomains,
    },
}

impl RequestFingerprint {
    fn new(secret_id: &str, tool: &str, action: &str, domains: &RequestedDomains) -> Self {
        match domains {
            RequestedDomains::None => {
                Self::Legacy(request_fingerprint(secret_id, tool, action, None))
            },
            RequestedDomains::One(domain) => {
                Self::Legacy(request_fingerprint(secret_id, tool, action, Some(domain)))
            },
            RequestedDomains::Set(_) | RequestedDomains::Any => Self::Scoped {
                secret_id: secret_id.to_owned(),
                tool: tool.to_owned(),
                action: action.to_owned(),
                domains: domains.clone(),
            },
        }
    }
}

fn request_fingerprint(secret_id: &str, tool: &str, action: &str, domain: Option<&str>) -> String {
    format!(
        "{}|{}|{}|{}",
        secret_id,
        tool,
        action,
        domain.unwrap_or_default()
    )
}

fn date_key(ts: i64) -> NaiveDate {
    chrono::DateTime::<Utc>::from_timestamp(ts, 0)
        .unwrap_or_else(Utc::now)
        .date_naive()
}

/// Match a policy pattern against a lowercase host, ignoring ASCII case in
/// the pattern.
fn domain_matches(pattern: &str, domain: &str) -> bool {
    if pattern == ANY_DOMAIN {
        return !domain.is_empty();
    }
    let pattern = pattern.to_ascii_lowercase();
    if pattern == domain {
        return true;
    }
    if let Some(suffix) = pattern.strip_prefix("*.") {
        return domain == suffix || domain.ends_with(&format!(".{suffix}"));
    }
    false
}

#[cfg(test)]
mod tests {
    use std::{
        fmt,
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc, Barrier,
        },
        thread,
    };

    use static_assertions::assert_not_impl_any;

    use super::*;

    fn request(action: &str, domain: Option<&str>) -> AccessRequest {
        AccessRequest {
            secret_id: "secret-1".to_string(),
            tool: "http".to_string(),
            action: action.to_string(),
            domain: domain.map(str::to_string),
            domains: Vec::new(),
            requested_at: Utc::now().timestamp(),
        }
    }

    fn grant_binding(tool: &str, action: &str, domain: Option<&str>) -> GrantBinding {
        GrantBinding::from_request(&AccessRequest {
            secret_id: "secret-1".to_string(),
            tool: tool.to_string(),
            action: action.to_string(),
            domain: domain.map(str::to_string),
            domains: Vec::new(),
            requested_at: Utc::now().timestamp(),
        })
    }

    #[test]
    fn matching_action_is_allowed() {
        let policy = SecretPolicy {
            allowed_tools: vec!["http:post".to_string()],
            ..Default::default()
        };
        let usage = UsageTracker::default();
        let approvals = ApprovalTable::default();

        let result = check_policy(&policy, &request("post", None), &usage, &approvals);

        assert_eq!(result, PolicyResult::Allowed);
    }

    #[test]
    fn mismatching_action_is_denied() {
        let policy = SecretPolicy {
            allowed_tools: vec!["http:post".to_string()],
            ..Default::default()
        };
        let usage = UsageTracker::default();
        let approvals = ApprovalTable::default();

        let result = check_policy(&policy, &request("get", None), &usage, &approvals);

        assert!(matches!(result, PolicyResult::Denied { .. }));
    }

    #[test]
    fn approval_required_returns_challenge() {
        let policy = SecretPolicy {
            requires_approval: true,
            ..Default::default()
        };
        let usage = UsageTracker::default();
        let approvals = ApprovalTable::new(60);

        let result = check_policy(
            &policy,
            &request("post", Some("api.example.com")),
            &usage,
            &approvals,
        );

        assert!(matches!(result, PolicyResult::NeedsApproval { .. }));
    }

    #[test]
    fn approved_challenge_is_consumed() {
        let policy = SecretPolicy {
            requires_approval: true,
            ..Default::default()
        };
        let usage = UsageTracker::default();
        let approvals = ApprovalTable::new(60);
        let req = request("post", Some("api.example.com"));

        let PolicyResult::NeedsApproval { challenge } =
            check_policy(&policy, &req, &usage, &approvals)
        else {
            panic!("expected approval challenge");
        };

        assert!(approvals.grant(&challenge.id).is_some());
        assert_eq!(
            check_policy(&policy, &req, &usage, &approvals),
            PolicyResult::Allowed
        );
        assert!(matches!(
            check_policy(&policy, &req, &usage, &approvals),
            PolicyResult::NeedsApproval { .. }
        ));
    }

    #[test]
    fn expired_challenge_requires_reapproval() {
        let approvals = ApprovalTable::new(-1);
        let req = request("post", Some("api.example.com"));
        let challenge = approvals.issue(&req);

        assert!(approvals.grant(&challenge.id).is_none());
    }

    #[test]
    fn daily_usage_budget_is_enforced() {
        let usage = UsageTracker::default();
        let req = request("post", None);
        usage.record_use(&req.secret_id, req.requested_at);

        assert!(!usage.is_within_limit(&req.secret_id, req.requested_at, 1));
    }

    #[test]
    fn daily_usage_budget_has_one_atomic_winner_under_contention() {
        const CONTENDERS: usize = 32;

        let usage = Arc::new(UsageTracker::default());
        let barrier = Arc::new(Barrier::new(CONTENDERS + 1));
        let requested_at = Utc::now().timestamp();
        let mut joins = Vec::new();
        for _ in 0..CONTENDERS {
            let usage = Arc::clone(&usage);
            let barrier = Arc::clone(&barrier);
            joins.push(thread::spawn(move || {
                barrier.wait();
                usage.try_record_use("secret-1", requested_at, Some(1))
            }));
        }
        barrier.wait();

        assert_eq!(
            joins
                .into_iter()
                .map(|join| join.join().expect("usage contender"))
                .filter(|won| *won)
                .count(),
            1
        );
        assert!(!usage.is_within_limit("secret-1", requested_at, 1));
    }

    #[test]
    fn grant_issue_and_redeem_round_trip() {
        let grants = GrantTable::default();
        let mut fields = HashMap::new();
        fields.insert("value".to_string(), "secret".to_string());

        let grant_id = grants.issue_grant(
            "secret-1",
            fields.clone(),
            InjectionTarget::Header {
                name: "Authorization".to_string(),
                prefix: Some("Bearer ".to_string()),
            },
            grant_binding("http", "post", Some("api.example.com")),
            60,
        );

        let payload = grants.redeem_grant(&grant_id).unwrap();
        assert_eq!(payload.secret_id, "secret-1");
        assert_eq!(payload.fields, fields);
        assert!(payload
            .binding
            .matches("http", "post", Some("api.example.com")));
    }

    #[test]
    fn double_redeem_fails() {
        let grants = GrantTable::default();
        let grant_id = grants.issue_grant(
            "secret-1",
            HashMap::new(),
            InjectionTarget::Header {
                name: "Authorization".to_string(),
                prefix: None,
            },
            grant_binding("http", "get", None),
            60,
        );

        assert!(grants.redeem_grant(&grant_id).is_some());
        assert!(grants.redeem_grant(&grant_id).is_none());
    }

    #[test]
    fn expired_grant_fails() {
        let grants = GrantTable::default();
        let grant_id = grants.issue_grant(
            "secret-1",
            HashMap::new(),
            InjectionTarget::Header {
                name: "Authorization".to_string(),
                prefix: None,
            },
            grant_binding("http", "get", None),
            -1,
        );

        assert!(grants.redeem_grant(&grant_id).is_none());
    }

    #[test]
    fn bound_batch_validates_every_grant_before_consuming_any() {
        let grants = GrantTable::default();
        let first = grants.issue_grant(
            "secret-1",
            HashMap::from([("value".to_string(), "first-secret".to_string())]),
            InjectionTarget::Inline,
            grant_binding("tool", "read", None),
            60,
        );
        let second = grants.issue_grant(
            "secret-2",
            HashMap::from([("value".to_string(), "second-secret".to_string())]),
            InjectionTarget::Inline,
            grant_binding("tool", "write", None),
            60,
        );
        let requests = [
            BoundGrantRedemption {
                token: &first,
                expected: GrantBindingExpectation::Secret {
                    tool: "tool",
                    action: "read",
                    domain: None,
                },
            },
            BoundGrantRedemption {
                token: &second,
                expected: GrantBindingExpectation::Secret {
                    tool: "tool",
                    action: "read",
                    domain: None,
                },
            },
        ];

        assert_eq!(
            grants.redeem_bound_batch(&requests).err(),
            Some(BoundGrantRedemptionError::BindingMismatch)
        );
        assert!(grants.redeem_grant(&first).is_some());
        assert!(grants.redeem_grant(&second).is_some());
    }

    #[test]
    fn delegated_batch_is_exact_to_scope_provider_agent_route_and_single_use() {
        let grants = GrantTable::default();
        let expected_authority =
            DelegatedGrantAuthority::new("owner", "default", "provider", "agent-a");
        let wrong_authority =
            DelegatedGrantAuthority::new("owner", "default", "provider", "agent-b");
        let request = AccessRequest::new("secret-1", "tool", "run", None);
        let token = grants.issue_grant(
            "secret-1",
            HashMap::from([("value".to_string(), "delegated-secret".to_string())]),
            InjectionTarget::Inline,
            GrantBinding::delegated(&request, expected_authority.clone()),
            60,
        );

        let wrong = [BoundGrantRedemption {
            token: &token,
            expected: GrantBindingExpectation::Delegated {
                tool: "tool",
                action: "run",
                domain: None,
                authority: &wrong_authority,
            },
        }];
        assert_eq!(
            grants.redeem_bound_batch(&wrong).err(),
            Some(BoundGrantRedemptionError::BindingMismatch)
        );

        let correct = [BoundGrantRedemption {
            token: &token,
            expected: GrantBindingExpectation::Delegated {
                tool: "tool",
                action: "run",
                domain: None,
                authority: &expected_authority,
            },
        }];
        assert_eq!(
            grants
                .redeem_bound_batch(&correct)
                .expect("exact delegated redemption")
                .len(),
            1
        );
        assert_eq!(
            grants.redeem_bound_batch(&correct).err(),
            Some(BoundGrantRedemptionError::MissingOrAlreadyRedeemed)
        );
    }

    #[test]
    fn delegated_batch_requires_exact_domain_even_when_grant_has_no_domain() {
        let grants = GrantTable::default();
        let authority = DelegatedGrantAuthority::new("owner", "default", "provider", "agent-a");
        let request = AccessRequest::new("secret-1", "tool", "run", None);
        let token = grants.issue_grant(
            "secret-1",
            HashMap::from([("value".to_string(), "delegated-secret".to_string())]),
            InjectionTarget::Inline,
            GrantBinding::delegated(&request, authority.clone()),
            60,
        );
        let wrong_domain = [BoundGrantRedemption {
            token: &token,
            expected: GrantBindingExpectation::Delegated {
                tool: "tool",
                action: "run",
                domain: Some("api.example.com"),
                authority: &authority,
            },
        }];

        assert_eq!(
            grants.redeem_bound_batch(&wrong_domain).err(),
            Some(BoundGrantRedemptionError::BindingMismatch)
        );
        assert!(grants.redeem_grant(&token).is_some());
    }

    #[test]
    fn phase2f_secret_batch_requires_exact_domain_without_changing_legacy_matching() {
        let grants = GrantTable::default();
        let token = grants.issue_grant(
            "secret-1",
            HashMap::from([("value".to_string(), "secret".to_string())]),
            InjectionTarget::Inline,
            grant_binding("tool", "run", None),
            60,
        );
        let exact_batch = [BoundGrantRedemption {
            token: &token,
            expected: GrantBindingExpectation::Secret {
                tool: "tool",
                action: "run",
                domain: Some("api.example.com"),
            },
        }];

        assert_eq!(
            grants.redeem_bound_batch(&exact_batch).err(),
            Some(BoundGrantRedemptionError::BindingMismatch)
        );
        let payload = grants.redeem_grant(&token).expect("grant was not consumed");
        assert!(
            payload
                .binding()
                .matches("tool", "run", Some("api.example.com")),
            "legacy direct consumers retain no-domain wildcard matching"
        );
    }

    #[test]
    fn delegated_authority_cannot_be_forged_through_grant_binding_deserialization() {
        let binding: GrantBinding = serde_json::from_value(serde_json::json!({
            "tool": "tool",
            "action": "run",
            "delegated_authority": {
                "principal": "other",
                "workspace": "other",
                "provider": "other",
                "agent_id": "other"
            }
        }))
        .expect("legacy grant binding remains deserializable");

        assert!(binding.delegated_authority.is_none());
    }

    #[test]
    fn bound_batch_rejects_duplicates_and_oversized_batches_without_consumption() {
        let grants = GrantTable::default();
        let token = grants.issue_grant(
            "secret-1",
            HashMap::from([("value".to_string(), "secret".to_string())]),
            InjectionTarget::Inline,
            grant_binding("tool", "run", None),
            60,
        );
        let duplicates = [
            BoundGrantRedemption {
                token: &token,
                expected: GrantBindingExpectation::Secret {
                    tool: "tool",
                    action: "run",
                    domain: None,
                },
            },
            BoundGrantRedemption {
                token: &token,
                expected: GrantBindingExpectation::Secret {
                    tool: "tool",
                    action: "run",
                    domain: None,
                },
            },
        ];
        assert_eq!(
            grants.redeem_bound_batch(&duplicates).err(),
            Some(BoundGrantRedemptionError::DuplicateToken)
        );

        let oversized = (0..=MAX_ATOMIC_GRANT_REDEMPTIONS)
            .map(|_| BoundGrantRedemption {
                token: "unused",
                expected: GrantBindingExpectation::Secret {
                    tool: "tool",
                    action: "run",
                    domain: None,
                },
            })
            .collect::<Vec<_>>();
        assert_eq!(
            grants.redeem_bound_batch(&oversized).err(),
            Some(BoundGrantRedemptionError::BatchTooLarge)
        );
        assert!(grants.redeem_grant(&token).is_some());
    }

    #[test]
    fn bound_batch_classifies_expiry_without_consuming_other_grants() {
        let grants = GrantTable::default();
        let live = grants.issue_grant(
            "live",
            HashMap::from([("value".to_string(), "live-secret".to_string())]),
            InjectionTarget::Inline,
            grant_binding("tool", "run", None),
            60,
        );
        let expired = grants.issue_grant(
            "expired",
            HashMap::from([("value".to_string(), "expired-secret".to_string())]),
            InjectionTarget::Inline,
            grant_binding("tool", "run", None),
            -1,
        );
        let requests = [
            BoundGrantRedemption {
                token: &expired,
                expected: GrantBindingExpectation::Secret {
                    tool: "tool",
                    action: "run",
                    domain: None,
                },
            },
            BoundGrantRedemption {
                token: &live,
                expected: GrantBindingExpectation::Secret {
                    tool: "tool",
                    action: "run",
                    domain: None,
                },
            },
        ];
        assert_eq!(
            grants.redeem_bound_batch(&requests).err(),
            Some(BoundGrantRedemptionError::Expired)
        );
        assert!(grants.redeem_grant(&live).is_some());
    }

    #[test]
    fn redemption_payload_is_sealed_and_zeroizes_values_on_drop() {
        assert_not_impl_any!(RedemptionPayload: Clone, fmt::Debug, Serialize);
        let observed = Arc::new(AtomicBool::new(false));
        {
            let mut payload = RedemptionPayload::new(
                "secret-1".to_string(),
                HashMap::from([("value".to_string(), "zeroize-me".to_string())]),
                InjectionTarget::Inline,
                grant_binding("tool", "run", None),
            );
            payload.install_drop_probe(Arc::clone(&observed));
        }
        assert!(observed.load(Ordering::SeqCst));
    }

    fn scoped_policy(allowed: &[&str]) -> SecretPolicy {
        SecretPolicy {
            allowed_domains: allowed.iter().map(|item| item.to_string()).collect(),
            ..Default::default()
        }
    }

    fn scoped(domains: &RequestedDomains) -> AccessRequest {
        AccessRequest::scoped("secret-1", "http", "get", domains)
    }

    fn check(policy: &SecretPolicy, request: &AccessRequest) -> PolicyResult {
        check_policy(
            policy,
            request,
            &UsageTracker::default(),
            &ApprovalTable::default(),
        )
    }

    fn reddit() -> RequestedDomains {
        RequestedDomains::hosts(["www.reddit.com", "OAUTH.reddit.com", "www.reddit.com"])
            .expect("valid host set")
    }

    #[test]
    fn host_sets_are_canonical_bounded_and_validated() {
        let RequestedDomains::Set(set) = reddit() else {
            panic!("two distinct hosts form a set");
        };
        assert_eq!(set.hosts(), ["oauth.reddit.com", "www.reddit.com"]);
        assert_eq!(
            reddit(),
            RequestedDomains::hosts(["www.reddit.com", "oauth.reddit.com"]).unwrap()
        );
        assert_eq!(
            RequestedDomains::hosts(["API.Example.com", "api.example.com"]).unwrap(),
            RequestedDomains::One("api.example.com".to_string())
        );
        assert_eq!(
            RequestedDomains::hosts(Vec::<String>::new()),
            Err(DomainScopeError::Empty)
        );
        let too_many = (0..=MAX_REQUESTED_DOMAINS).map(|index| format!("h{index}.example.com"));
        assert_eq!(
            RequestedDomains::hosts(too_many),
            Err(DomainScopeError::TooMany)
        );
        let at_limit = (0..MAX_REQUESTED_DOMAINS).map(|index| format!("h{index}.example.com"));
        assert!(RequestedDomains::hosts(at_limit).is_ok());
        for bad in [
            "*",
            "*.example.com",
            "https://example.com",
            "example.com:443",
            "example.com/path",
            "exa mple.com",
            "",
            "example..com",
            "-example.com",
            "example.com.",
            "user@example.com",
        ] {
            assert!(
                matches!(
                    RequestedDomains::hosts(["ok.example.com", bad]),
                    Err(DomainScopeError::InvalidHost(_))
                ),
                "{bad:?} must be refused"
            );
        }
    }

    #[test]
    fn domain_set_is_allowed_only_when_every_host_matches() {
        let request = scoped(&reddit());
        assert_eq!(
            check(&scoped_policy(&["*.reddit.com"]), &request),
            PolicyResult::Allowed
        );
        assert_eq!(
            check(
                &scoped_policy(&["oauth.reddit.com", "www.reddit.com"]),
                &request
            ),
            PolicyResult::Allowed
        );
        assert_eq!(
            check(&scoped_policy(&["*.REDDIT.com"]), &request),
            PolicyResult::Allowed,
            "set members compare against lowercased patterns"
        );
        let PolicyResult::Denied { reason } =
            check(&scoped_policy(&["oauth.reddit.com"]), &request)
        else {
            panic!("a host outside the policy must deny the whole set");
        };
        assert!(reason.contains("www.reddit.com"), "{reason}");
        assert_eq!(check(&scoped_policy(&[]), &request), PolicyResult::Allowed);
        assert_eq!(check(&scoped_policy(&["*"]), &request), PolicyResult::Allowed);
    }

    #[test]
    fn any_domain_needs_an_unrestricted_policy_or_a_bare_star() {
        let any = scoped(&RequestedDomains::Any);
        assert_eq!(check(&scoped_policy(&[]), &any), PolicyResult::Allowed);
        assert_eq!(
            check(&scoped_policy(&["api.example.com", "*"]), &any),
            PolicyResult::Allowed
        );
        assert!(matches!(
            check(&scoped_policy(&["*.example.com"]), &any),
            PolicyResult::Denied { .. }
        ));
        assert!(matches!(
            check(&scoped_policy(&["api.example.com"]), &any),
            PolicyResult::Denied { .. }
        ));
    }

    #[test]
    fn bare_star_policy_admits_every_single_domain_but_not_a_missing_one() {
        let policy = scoped_policy(&["*"]);
        assert_eq!(
            check(&policy, &request("get", Some("anything.example.org"))),
            PolicyResult::Allowed
        );
        assert!(matches!(
            check(&policy, &request("get", None)),
            PolicyResult::Denied { .. }
        ));
        assert!(matches!(
            check(&policy, &request("get", Some(""))),
            PolicyResult::Denied { .. }
        ));
    }

    #[test]
    fn single_domain_policy_checks_ignore_case_and_need_a_host() {
        let policy = scoped_policy(&["*.example.com", "exact.test"]);
        for (domain, allowed) in [
            (Some("api.example.com"), true),
            (Some("example.com"), true),
            (Some("exact.test"), true),
            (Some("EXACT.test"), true),
            (Some("API.Example.COM"), true),
            (Some("evil.test/.example.com"), false),
            (Some("https://api.example.com"), false),
            (Some("api.example.com:443"), false),
            (Some("evil-example.com"), false),
            (None, false),
        ] {
            assert_eq!(
                check(&policy, &request("get", domain)) == PolicyResult::Allowed,
                allowed,
                "{domain:?}"
            );
        }
    }

    #[test]
    fn malformed_wire_domains_are_denied_even_without_a_domain_policy() {
        let mut both = request("get", Some("api.example.com"));
        both.domains = vec!["a.example.com".to_string(), "b.example.com".to_string()];
        let mut bad_host = request("get", None);
        bad_host.domains = vec!["https://a.example.com".to_string()];
        let mut star_in_set = request("get", None);
        star_in_set.domains = vec!["*".to_string(), "a.example.com".to_string()];
        for request in [both, bad_host, star_in_set] {
            assert!(matches!(
                check(&SecretPolicy::default(), &request),
                PolicyResult::Denied { .. }
            ));
        }
    }

    #[test]
    fn old_serialized_requests_deserialize_and_single_domain_json_is_unchanged() {
        let old: AccessRequest = serde_json::from_value(serde_json::json!({
            "secret_id": "secret-1",
            "tool": "http",
            "action": "get",
            "domain": "api.example.com",
            "requested_at": 1
        }))
        .expect("pre-domain-set request");
        assert!(old.domains.is_empty());
        assert_eq!(
            old.requested_domains().unwrap(),
            RequestedDomains::One("api.example.com".to_string())
        );
        assert_eq!(
            serde_json::to_value(&old).unwrap(),
            serde_json::json!({
                "secret_id": "secret-1",
                "tool": "http",
                "action": "get",
                "domain": "api.example.com",
                "requested_at": 1
            })
        );
        let no_domain: AccessRequest = serde_json::from_value(serde_json::json!({
            "secret_id": "secret-1", "tool": "http", "action": "get", "requested_at": 1
        }))
        .unwrap();
        assert_eq!(no_domain.requested_domains().unwrap(), RequestedDomains::None);

        let mut set = scoped(&reddit());
        set.requested_at = 1;
        let json = serde_json::to_value(&set).unwrap();
        assert_eq!(
            json["domains"],
            serde_json::json!(["oauth.reddit.com", "www.reddit.com"])
        );
        assert!(json.get("domain").is_none());
        let round_trip: AccessRequest = serde_json::from_value(json).unwrap();
        assert_eq!(round_trip.requested_domains().unwrap(), reddit());

        let any = scoped(&RequestedDomains::Any);
        assert_eq!(serde_json::to_value(&any).unwrap()["domains"], serde_json::json!(["*"]));
        assert_eq!(any.requested_domains().unwrap(), RequestedDomains::Any);
    }

    #[test]
    fn legacy_fingerprints_are_byte_identical() {
        assert_eq!(
            RequestFingerprint::new(
                "secret-1",
                "http",
                "get",
                &RequestedDomains::One("api.example.com".to_string())
            ),
            RequestFingerprint::Legacy("secret-1|http|get|api.example.com".to_string())
        );
        assert_eq!(
            RequestFingerprint::new("secret-1", "http", "get", &RequestedDomains::None),
            RequestFingerprint::Legacy("secret-1|http|get|".to_string())
        );
        assert_eq!(
            request_fingerprint("secret-1", "http", "get", Some("api.example.com")),
            "secret-1|http|get|api.example.com"
        );
        assert!(matches!(
            RequestFingerprint::new("secret-1", "http", "get", &reddit()),
            RequestFingerprint::Scoped { .. }
        ));
    }

    #[test]
    fn approval_for_a_set_is_consumed_only_by_that_exact_set() {
        let approvals = ApprovalTable::new(60);
        let other = RequestedDomains::hosts(["oauth.reddit.com", "old.reddit.com"]).unwrap();
        approvals.approve_request(&scoped(&reddit()));

        assert!(!approvals.consume(&scoped(&other)));
        assert!(!approvals.consume(&scoped(&RequestedDomains::Any)));
        assert!(!approvals.consume(&request("get", Some("oauth.reddit.com"))));
        assert!(!approvals.consume(&request("get", None)));
        let reordered = RequestedDomains::hosts(["WWW.reddit.com", "oauth.reddit.com"]).unwrap();
        assert!(approvals.consume(&scoped(&reordered)));
        assert!(!approvals.consume(&scoped(&reddit())), "approval is single-use");

        approvals.approve_request(&request("get", Some("api.example.com")));
        assert!(approvals.consume(&request("get", Some("api.example.com"))));
    }

    #[test]
    fn challenge_for_any_round_trips_through_grant() {
        let policy = SecretPolicy {
            requires_approval: true,
            ..Default::default()
        };
        let usage = UsageTracker::default();
        let approvals = ApprovalTable::new(60);
        let any = scoped(&RequestedDomains::Any);
        let PolicyResult::NeedsApproval { challenge } =
            check_policy(&policy, &any, &usage, &approvals)
        else {
            panic!("approval challenge");
        };
        assert_eq!(challenge.domains, vec!["*".to_string()]);
        assert_eq!(challenge.requested_domains().unwrap(), RequestedDomains::Any);
        assert!(approvals.grant(&challenge.id).is_some());
        assert!(matches!(
            check_policy(&policy, &scoped(&reddit()), &usage, &approvals),
            PolicyResult::NeedsApproval { .. }
        ));
        assert_eq!(
            check_policy(&policy, &any, &usage, &approvals),
            PolicyResult::Allowed
        );
    }

    #[test]
    fn set_binding_matches_only_the_same_set() {
        let binding = GrantBinding::from_request(&scoped(&reddit()));
        let other = RequestedDomains::hosts(["oauth.reddit.com", "old.reddit.com"]).unwrap();

        assert!(binding.matches_domains("http", "get", &reddit()));
        assert!(!binding.matches_domains("http", "get", &other));
        assert!(!binding.matches_domains("http", "get", &RequestedDomains::Any));
        assert!(!binding.matches_domains("http", "post", &reddit()));
        assert!(!binding.matches("http", "get", Some("www.reddit.com")));
        assert!(!binding.matches("http", "get", None));

        let any = GrantBinding::from_request(&scoped(&RequestedDomains::Any));
        assert!(any.matches_domains("http", "get", &RequestedDomains::Any));
        assert!(!any.matches_domains("http", "get", &reddit()));
        assert!(!any.matches("http", "get", Some("www.reddit.com")));

        let legacy = grant_binding("http", "get", None);
        assert!(!legacy.matches_domains("http", "get", &reddit()));
        assert!(legacy.matches_domains("http", "get", &RequestedDomains::None));
        assert!(legacy.matches("http", "get", Some("www.reddit.com")));
        assert_eq!(
            serde_json::to_value(grant_binding("http", "get", Some("api.example.com"))).unwrap(),
            serde_json::json!({"tool": "http", "action": "get", "domain": "api.example.com"})
        );
    }

    #[test]
    fn secret_batch_with_sets_requires_the_exact_set() {
        let grants = GrantTable::default();
        let token = grants.issue_grant(
            "secret-1",
            HashMap::from([("value".to_string(), "secret".to_string())]),
            InjectionTarget::Inline,
            GrantBinding::from_request(&scoped(&reddit())),
            60,
        );
        let other = RequestedDomains::hosts(["oauth.reddit.com", "old.reddit.com"]).unwrap();
        for wrong in [
            GrantBindingExpectation::SecretScoped {
                tool: "http",
                action: "get",
                domains: &other,
            },
            GrantBindingExpectation::SecretScoped {
                tool: "http",
                action: "get",
                domains: &RequestedDomains::Any,
            },
            GrantBindingExpectation::Secret {
                tool: "http",
                action: "get",
                domain: None,
            },
            GrantBindingExpectation::Secret {
                tool: "http",
                action: "get",
                domain: Some("www.reddit.com"),
            },
        ] {
            assert_eq!(
                grants
                    .redeem_bound_batch(&[BoundGrantRedemption {
                        token: &token,
                        expected: wrong,
                    }])
                    .err(),
                Some(BoundGrantRedemptionError::BindingMismatch)
            );
        }
        let exact = reddit();
        assert_eq!(
            grants
                .redeem_bound_batch(&[BoundGrantRedemption {
                    token: &token,
                    expected: GrantBindingExpectation::SecretScoped {
                        tool: "http",
                        action: "get",
                        domains: &exact,
                    },
                }])
                .expect("exact set redeems")
                .len(),
            1
        );

        let legacy = grants.issue_grant(
            "secret-1",
            HashMap::new(),
            InjectionTarget::Inline,
            grant_binding("http", "get", Some("api.example.com")),
            60,
        );
        let one = RequestedDomains::One("api.example.com".to_string());
        assert!(grants
            .redeem_bound_batch(&[BoundGrantRedemption {
                token: &legacy,
                expected: GrantBindingExpectation::SecretScoped {
                    tool: "http",
                    action: "get",
                    domains: &one,
                },
            }])
            .is_ok());
    }

    #[test]
    fn delegated_batch_with_sets_requires_the_exact_set_and_authority() {
        let grants = GrantTable::default();
        let authority = DelegatedGrantAuthority::new("owner", "default", "provider", "agent-a");
        let token = grants.issue_grant(
            "secret-1",
            HashMap::from([("value".to_string(), "delegated-secret".to_string())]),
            InjectionTarget::Inline,
            GrantBinding::delegated(&scoped(&RequestedDomains::Any), authority.clone()),
            60,
        );
        let set = reddit();
        for wrong in [
            GrantBindingExpectation::DelegatedScoped {
                tool: "http",
                action: "get",
                domains: &set,
                authority: &authority,
            },
            GrantBindingExpectation::Delegated {
                tool: "http",
                action: "get",
                domain: None,
                authority: &authority,
            },
        ] {
            assert_eq!(
                grants
                    .redeem_bound_batch(&[BoundGrantRedemption {
                        token: &token,
                        expected: wrong,
                    }])
                    .err(),
                Some(BoundGrantRedemptionError::BindingMismatch)
            );
        }
        assert!(grants
            .redeem_bound_batch(&[BoundGrantRedemption {
                token: &token,
                expected: GrantBindingExpectation::DelegatedScoped {
                    tool: "http",
                    action: "get",
                    domains: &RequestedDomains::Any,
                    authority: &authority,
                },
            }])
            .is_ok());
    }

    #[test]
    fn pattern_case_is_ignored_for_single_domains_and_sets_alike() {
        let policy = scoped_policy(&["API.example.com", "*.Reddit.COM"]);
        assert_eq!(
            check(&policy, &request("get", Some("api.example.com"))),
            PolicyResult::Allowed
        );
        assert_eq!(check(&policy, &scoped(&reddit())), PolicyResult::Allowed);
        let mixed = RequestedDomains::hosts(["api.example.com", "www.reddit.com"]).unwrap();
        assert_eq!(check(&policy, &scoped(&mixed)), PolicyResult::Allowed);
    }

    #[test]
    fn a_non_host_single_domain_cannot_satisfy_a_suffix_pattern() {
        let policy = scoped_policy(&["*.example.com"]);
        let PolicyResult::Denied { reason } =
            check(&policy, &request("get", Some("evil.test/.example.com")))
        else {
            panic!("a path-bearing domain must be denied");
        };
        assert!(reason.contains("not a DNS host name"), "{reason}");
        assert_eq!(
            check(&SecretPolicy::default(), &request("get", Some("evil.test/.example.com"))),
            PolicyResult::Allowed,
            "an unrestricted policy still ignores the domain"
        );
    }

    #[test]
    fn host_text_in_errors_and_reasons_is_bounded_and_escaped() {
        let long = format!("{}\n\u{1b}[31m", "a".repeat(400));
        let Err(DomainScopeError::InvalidHost(shown)) =
            RequestedDomains::hosts(["ok.example.com", long.as_str()])
        else {
            panic!("invalid host");
        };
        assert!(shown.len() <= MAX_DISPLAYED_HOST_BYTES + 3, "{}", shown.len());
        assert!(shown.ends_with("..."));
        assert!(!shown.chars().any(char::is_control));

        let PolicyResult::Denied { reason } = check(
            &scoped_policy(&["*.example.com"]),
            &request("get", Some("bad\r\nhost\u{7}")),
        ) else {
            panic!("denied");
        };
        assert!(!reason.chars().any(char::is_control), "{reason:?}");
        assert!(reason.contains("bad\\r\\nhost\\u{7}"), "{reason}");
        assert_eq!(display_host("é".repeat(100).as_str()).len(), 128 + 3);
    }

    #[test]
    fn wire_domain_plus_star_conflicts_and_a_star_domain_stays_one() {
        assert_eq!(
            RequestedDomains::from_wire(Some("api.example.com"), &["*".to_string()]),
            Err(DomainScopeError::Conflicting)
        );
        let mut request = request("get", Some("*"));
        assert_eq!(
            request.requested_domains().unwrap(),
            RequestedDomains::One("*".to_string())
        );
        assert!(matches!(
            check(&scoped_policy(&["*"]), &request),
            PolicyResult::Denied { .. }
        ));
        request.domains = vec!["*".to_string()];
        assert!(matches!(
            check(&SecretPolicy::default(), &request),
            PolicyResult::Denied { .. }
        ));
    }

    #[test]
    fn unicode_hosts_are_refused_and_punycode_is_accepted() {
        for unicode in ["bücher.example", "例え.jp", "BÜCHER.example"] {
            assert!(
                matches!(
                    RequestedDomains::hosts([unicode]),
                    Err(DomainScopeError::InvalidHost(_))
                ),
                "{unicode}"
            );
        }
        assert_eq!(
            RequestedDomains::hosts(["XN--BCHER-KVA.example"]).unwrap(),
            RequestedDomains::One("xn--bcher-kva.example".to_string())
        );
        assert_eq!(
            check(
                &scoped_policy(&["*.example"]),
                &request("get", Some("xn--bcher-kva.example"))
            ),
            PolicyResult::Allowed
        );
        assert!(matches!(
            check(&scoped_policy(&["*.example"]), &request("get", Some("bücher.example"))),
            PolicyResult::Denied { .. }
        ));
    }

    #[test]
    fn a_delegated_grant_never_redeems_through_a_secret_expectation() {
        let grants = GrantTable::default();
        let authority = DelegatedGrantAuthority::new("owner", "default", "provider", "agent-a");
        let legacy = grants.issue_grant(
            "secret-1",
            HashMap::from([("value".to_string(), "delegated-secret".to_string())]),
            InjectionTarget::Inline,
            GrantBinding::delegated(&request("get", None), authority.clone()),
            60,
        );
        let any = grants.issue_grant(
            "secret-1",
            HashMap::from([("value".to_string(), "delegated-secret".to_string())]),
            InjectionTarget::Inline,
            GrantBinding::delegated(&scoped(&RequestedDomains::Any), authority.clone()),
            60,
        );
        for (token, expected) in [
            (
                &legacy,
                GrantBindingExpectation::Secret {
                    tool: "http",
                    action: "get",
                    domain: None,
                },
            ),
            (
                &any,
                GrantBindingExpectation::SecretScoped {
                    tool: "http",
                    action: "get",
                    domains: &RequestedDomains::Any,
                },
            ),
        ] {
            assert_eq!(
                grants
                    .redeem_bound_batch(&[BoundGrantRedemption { token, expected }])
                    .err(),
                Some(BoundGrantRedemptionError::BindingMismatch)
            );
        }
        assert!(grants
            .redeem_bound_batch(&[BoundGrantRedemption {
                token: &legacy,
                expected: GrantBindingExpectation::Delegated {
                    tool: "http",
                    action: "get",
                    domain: None,
                    authority: &authority,
                },
            }])
            .is_ok());
    }

    #[test]
    fn header_targets_only_accept_http_policy_routes() {
        let unsupported = find_unsupported_provisioned_policy_routes(
            &InjectionTarget::Header {
                name: "Authorization".to_string(),
                prefix: None,
            },
            &[
                "http:post".to_string(),
                "browser:execute".to_string(),
                "http:request".to_string(),
            ],
        );

        assert_eq!(
            unsupported,
            vec!["browser:execute".to_string(), "http:request".to_string()]
        );
    }

    #[test]
    fn cookie_targets_reject_internal_restore_state_route() {
        let unsupported = find_unsupported_provisioned_policy_routes(
            &InjectionTarget::Cookies(vec![]),
            &["browser:restore_state".to_string()],
        );

        assert_eq!(unsupported, vec!["browser:restore_state".to_string()]);
    }
}
