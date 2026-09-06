use std::collections::{HashMap, HashSet};
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

/// Canonical runtime access request used for secret policy checks.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AccessRequest {
    pub secret_id: String,
    pub tool: String,
    pub action: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub domain: Option<String>,
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
            requested_at: Utc::now().timestamp(),
        }
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
    pub expires_at: i64,
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
            delegated_authority: None,
        }
    }

    pub fn delegated(request: &AccessRequest, authority: DelegatedGrantAuthority) -> Self {
        Self {
            tool: request.tool.clone(),
            action: request.action.clone(),
            domain: request.domain.clone(),
            delegated_authority: Some(authority),
        }
    }

    pub fn matches(&self, tool: &str, action: &str, domain: Option<&str>) -> bool {
        if self.tool != tool || self.action != action {
            return false;
        }
        match self.domain.as_deref() {
            Some(expected_domain) => domain == Some(expected_domain),
            None => true,
        }
    }

    fn matches_exact(&self, tool: &str, action: &str, domain: Option<&str>) -> bool {
        self.tool == tool && self.action == action && self.domain.as_deref() == domain
    }

    pub fn matches_delegated(
        &self,
        tool: &str,
        action: &str,
        domain: Option<&str>,
        authority: &DelegatedGrantAuthority,
    ) -> bool {
        self.matches_exact(tool, action, domain)
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
    fingerprint: String,
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

        self.approved
            .lock()
            .expect("approval granted lock poisoned")
            .push(ApprovedRequest {
                fingerprint: request_fingerprint(
                    &challenge.secret_id,
                    &challenge.tool,
                    &challenge.action,
                    challenge.domain.as_deref(),
                ),
                expires_at: challenge.expires_at,
            });
        Some(challenge)
    }

    pub fn approve_request(&self, request: &AccessRequest) {
        self.reap_expired();
        self.approved
            .lock()
            .expect("approval granted lock poisoned")
            .push(ApprovedRequest {
                fingerprint: request_fingerprint(
                    &request.secret_id,
                    &request.tool,
                    &request.action,
                    request.domain.as_deref(),
                ),
                expires_at: request.requested_at + self.ttl_secs,
            });
    }

    pub fn consume(&self, request: &AccessRequest) -> bool {
        self.reap_expired();
        let fingerprint = request_fingerprint(
            &request.secret_id,
            &request.tool,
            &request.action,
            request.domain.as_deref(),
        );
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
                } => record
                    .payload
                    .binding()
                    .matches_exact(tool, action, *domain),
                GrantBindingExpectation::Delegated {
                    tool,
                    action,
                    domain,
                    authority,
                } => record
                    .payload
                    .binding()
                    .matches_delegated(tool, action, *domain, authority),
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

    if !policy.allowed_domains.is_empty() {
        let Some(domain) = request.domain.as_deref() else {
            return PolicyResult::Denied {
                reason: "secret is domain-scoped but runtime action had no target domain".into(),
            };
        };

        if !policy
            .allowed_domains
            .iter()
            .any(|pattern| domain_matches(pattern, domain))
        {
            return PolicyResult::Denied {
                reason: format!("domain '{}' is not allowed for this secret", domain),
            };
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

fn domain_matches(pattern: &str, domain: &str) -> bool {
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
            requested_at: Utc::now().timestamp(),
        }
    }

    fn grant_binding(tool: &str, action: &str, domain: Option<&str>) -> GrantBinding {
        GrantBinding::from_request(&AccessRequest {
            secret_id: "secret-1".to_string(),
            tool: tool.to_string(),
            action: action.to_string(),
            domain: domain.map(str::to_string),
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
