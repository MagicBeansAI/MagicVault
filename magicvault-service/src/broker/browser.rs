//! Standalone browser permissions and one-use effect jobs. Shared core policy
//! and embedded consumers are untouched. No browser I/O under the state lock.
use super::*;
use magicvault_effect::{
    canonical_origin, cdp::CdpBrowser, matches_target_filter, valid_target_filter, BrowserAdapter,
    MaterialField, Outcome, Target,
};
#[cfg(all(test, unix))]
mod tests;

const MAX_BROWSER_PERMISSIONS: usize = 64;
const MAX_NATIVE_GRANTS: usize = 128;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct NativeGrant {
    pub client_id: Uuid,
    extension_id: String,
    profile_id: Uuid,
    token_hash: [u8; 32],
    allowed: bool,
}

pub(super) fn valid_native_grants(registry: &Registry) -> bool {
    registry.native_grants.len() <= MAX_NATIVE_GRANTS
        && registry.native_grants.iter().all(|g| {
            !g.profile_id.is_nil()
                && magicvault_effect::bridge::valid_extension_id(&g.extension_id)
                && registry.peers.iter().any(|p| p.id == g.client_id)
        })
        && registry
            .native_grants
            .iter()
            .map(|g| (g.client_id, &g.extension_id, g.profile_id))
            .collect::<BTreeSet<_>>()
            .len()
            == registry.native_grants.len()
}

#[derive(Serialize)]
struct FillReceipt {
    operation_id: Uuid,
    client_id: Uuid,
    browser_handle: Uuid,
    target_handle: Uuid,
    state: FillState,
    fields: Vec<FieldState>,
    error: Option<ErrorCode>,
}
impl magicvault_core::store::AuditReceipt for FillReceipt {}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct BrowserPermission {
    pub client_id: Uuid,
    pub rule: BrowserRule,
}

#[derive(Clone, PartialEq, Eq)]
struct NativeIdentity {
    profile_id: Uuid,
    extension_id: String,
}

struct RegisteredBrowser {
    owner: Uuid,
    native_profile: Option<NativeIdentity>,
    info: BrowserInfo,
    adapter: Arc<dyn BrowserAdapter>,
}
#[derive(Clone)]
struct BoundTarget {
    owner: Uuid,
    browser: Uuid,
    target: Target,
    expires: Instant,
}
struct FillJob {
    owner: Uuid,
    request: SecureFill,
    status: FillStatus,
    cancel: CancellationToken,
    retain_until: Instant,
}

#[derive(Default)]
pub(super) struct BrowserState {
    instances: HashMap<Uuid, RegisteredBrowser>,
    targets: HashMap<Uuid, BoundTarget>,
    fills: HashMap<Uuid, FillJob>,
    // Retained for the daemon epoch even after detailed results expire. Never
    // reopen an old operation ID with a new document/request after eviction.
    used_operations: BTreeSet<Uuid>,
}
impl BrowserState {
    pub(super) fn reap(&mut self, now: Instant) {
        self.instances.retain(|_, b| b.adapter.connected());
        for job in self
            .fills
            .values()
            .filter(|j| !self.instances.contains_key(&j.request.browser_handle))
        {
            job.cancel.cancel();
        }
        self.targets
            .retain(|_, t| t.expires > now && self.instances.contains_key(&t.browser));
        self.fills.retain(|_, job| {
            job.retain_until > now
                || matches!(job.status.state, FillState::Pending | FillState::Filling)
        });
    }
    pub(super) fn revoke(&mut self, owner: Uuid) {
        self.instances.retain(|_, b| {
            if b.owner == owner {
                b.adapter.disconnect();
                false
            } else {
                true
            }
        });
        self.targets.retain(|_, t| t.owner != owner);
        for job in self.fills.values_mut().filter(|j| j.owner == owner) {
            job.cancel.cancel();
        }
    }
    pub(super) fn stop_all(&mut self) {
        for browser in self.instances.values() {
            browser.adapter.disconnect();
        }
        for job in self.fills.values() {
            job.cancel.cancel();
        }
        self.instances.clear();
        self.targets.clear();
    }
}

pub(super) fn valid_permissions(registry: &Registry, store: &SecretStore) -> bool {
    registry.browser_permissions.len() <= MAX_BROWSER_PERMISSIONS
        && registry.browser_permissions.iter().all(|p| {
            p.rule.valid()
                && !p.rule.origins.is_empty()
                && p.rule
                    .origins
                    .iter()
                    .all(|o| canonical_origin(o).as_deref() == Ok(o.as_str()))
                && registry.peers.iter().any(|peer| {
                    peer.id == p.client_id && peer.references.contains(&p.rule.credential_ref)
                })
                && store
                    .provisioned_metadata(&p.rule.credential_ref)
                    .is_some_and(|m| p.rule.field_names.iter().all(|f| m.field_names.contains(f)))
        })
        && registry
            .browser_permissions
            .iter()
            .map(|p| (p.client_id, &p.rule.credential_ref))
            .collect::<BTreeSet<_>>()
            .len()
            == registry.browser_permissions.len()
}

fn allowed(state: &State, auth: &Auth, target: &Target, field: &FillField) -> bool {
    state.registry.browser_permissions.iter().any(|p| {
        p.client_id == auth.id
            && p.rule.credential_ref == field.credential_ref
            && p.rule.field_names.contains(&field.credential_field)
            && p.rule.origins.contains(&target.origin)
            && p.rule.origins.contains(&target.top_origin)
    })
}

fn rule_prompt(label: &str, credential: &str, rule: &BrowserRule) -> String {
    format!("Configure browser use for client {label}: credential {credential} ({}), fields {}. Exact permitted origins for BOTH page and selected frame: {}. An empty origin list removes permission. Each fill still requires its own human decision. Websites receive the filled values; other browser tools can observe them. Allow?",rule.credential_ref,rule.field_names.join(", "),rule.origins.join(", "))
}

fn fill_prompt(label: &str, request: &SecureFill, target: &Target) -> String {
    // Printable ASCII plus quoting prevents control/bidi characters from
    // becoming native prompt instructions. Selectors remain untrusted metadata.
    let selections = request
        .fields
        .iter()
        .map(|f| format!("{}:{} -> {:?}", f.credential_ref, f.credential_field, f.css))
        .collect::<Vec<_>>()
        .join("; ");
    format!("Allow ONE credential fill for client {label}? Browser handle: {}. Page: {}. Selected frame: {}. Requested fields (untrusted selectors): {selections}. No form submission is requested. The website receives these values and other browser tools may read them.",request.browser_handle,target.top_origin,target.origin)
}

impl Broker {
    #[cfg(unix)]
    pub(crate) async fn accept_native(
        self: &Arc<Self>,
        mut stream: tokio::net::UnixStream,
    ) -> Result<(), ErrorCode> {
        use crate::native::{NativeGreeting, NativeHello, NATIVE_VERSION};
        use magicvault_effect::bridge::{self, NativeBridge};
        if !stream
            .peer_cred()
            .is_ok_and(|p| p.uid() == unsafe { libc::geteuid() })
        {
            return Err(ErrorCode::Unauthorized);
        }
        let cancel = self.shutdown.child_token();
        let admission = async {
            let bytes = tokio::time::timeout(Duration::from_secs(5), bridge::read_frame(&mut stream))
                .await.map_err(|_| ErrorCode::Expired)??;
            let hello: NativeHello = serde_json::from_slice(&bytes).map_err(|_| ErrorCode::UnsupportedVersion)?;
            drop(bytes);
            if hello.version != NATIVE_VERSION { return Err(ErrorCode::UnsupportedVersion); }
            // Native host EOF cancels a pending consent; a vanished profile cannot
            // leave a surprise approval dialog or acquire a remembered grant.
            tokio::select! {
                result = self.authorize_native(&hello, cancel.clone()) => result,
                _ = bridge::read_frame(&mut stream) => { cancel.cancel(); Err(ErrorCode::Cancelled) }
            }
        }.await;
        let (auth, identity) = match admission {
            Ok(admission) => admission,
            Err(code) => {
                cancel.cancel();
                let bytes = serde_json::to_vec(&NativeGreeting::Error {
                    version: NATIVE_VERSION,
                    code,
                })
                .map_err(|_| ErrorCode::Unavailable)?;
                let _ = tokio::time::timeout(
                    Duration::from_secs(5),
                    bridge::write_frame(&mut stream, &bytes),
                )
                .await;
                return Err(code);
            }
        };
        let adapter = NativeBridge::authenticated(stream);
        let info = BrowserInfo {
            browser_handle: Uuid::new_v4(),
            label: format!("Chromium profile {}", identity.profile_id),
            backend: BrowserBackend::Extension,
        };
        if let Err(code) = self
            .register_adapter(
                auth,
                Uuid::new_v4(),
                info.clone(),
                adapter.clone(),
                Some(identity),
            )
            .await
        {
            let bytes = serde_json::to_vec(&NativeGreeting::Error {
                version: NATIVE_VERSION,
                code,
            })
            .map_err(|_| ErrorCode::Unavailable)?;
            let _ = adapter.initialize(&bytes).await;
            adapter.disconnect();
            return Err(code);
        }
        let greeting = serde_json::to_vec(&NativeGreeting::Ready {
            version: NATIVE_VERSION,
            browser_handle: info.browser_handle,
        })
        .map_err(|_| ErrorCode::Unavailable)?;
        adapter.initialize(&greeting).await
    }

    async fn authorize_native(
        self: &Arc<Self>,
        hello: &crate::native::NativeHello,
        cancel: CancellationToken,
    ) -> Result<(Auth, NativeIdentity), ErrorCode> {
        use crate::native;
        let browser = hello
            .browser
            .as_ref()
            .filter(|p| p.valid())
            .ok_or(ErrorCode::UnsupportedVersion)?;
        if hello.instance_id != self.lease.instance.id
            || !magicvault_effect::bridge::valid_extension_id(&hello.extension_id)
            || hello.token.len() != 64
            || !hello.token.bytes().all(|b| b.is_ascii_hexdigit())
        {
            return Err(ErrorCode::Unauthorized);
        }
        let root = self.lease.root().to_owned();
        let extension_id = hello.extension_id.clone();
        let profile_id = browser.profile_id;
        let profile_digest = token_hash(&browser.capability);
        let digest = token_hash(&hello.token);
        let manual = browser.manual;
        let (auth, label, remembered) = self
            .transaction(move |b, s| {
                b.ready(s)?;
                let config = native::load_config(
                    &native::config_path(&root),
                    &format!("chrome-extension://{extension_id}/"),
                )?;
                let expected = native::hello(&config)?;
                if !equal_digest(&token_hash(&expected.token), &digest) {
                    return Err(ErrorCode::Unauthorized);
                }
                let peer = s
                    .registry
                    .peers
                    .iter()
                    .find(|p| equal_digest(&p.token_hash, &digest))
                    .ok_or(ErrorCode::Unauthorized)?;
                if s.browsers.instances.values().any(|p| {
                    p.owner == peer.id
                        && p.native_profile.as_ref().is_some_and(|n| {
                            n.profile_id == profile_id && n.extension_id == extension_id
                        })
                }) {
                    return Err(ErrorCode::Conflict); // Never evict a live profile, even a copied one.
                }
                if s.browsers.instances.len() >= MAX_BROWSERS {
                    return Err(ErrorCode::Capacity);
                }
                let grant = s.registry.native_grants.iter().find(|g| {
                    g.client_id == peer.id
                        && g.extension_id == extension_id
                        && g.profile_id == profile_id
                });
                if grant.is_some_and(|g| !equal_digest(&g.token_hash, &profile_digest)) {
                    return Err(ErrorCode::Unauthorized);
                }
                if grant.is_some_and(|g| !g.allowed) && !manual {
                    return Err(ErrorCode::Denied);
                }
                Ok((
                    Auth {
                        id: peer.id,
                        digest,
                    },
                    peer.label.clone(),
                    grant.is_some_and(|g| g.allowed),
                ))
            })
            .await?;
        if remembered {
            return Ok((
                auth,
                NativeIdentity {
                    profile_id,
                    extension_id: hello.extension_id.clone(),
                },
            ));
        }
        // Reconnects with an existing approval never contend for the human gate.
        let _permit = Arc::clone(&self.human_gate)
            .try_acquire_owned()
            .map_err(|_| ErrorCode::Busy)?;
        let auth2 = auth.clone();
        let extension_id = hello.extension_id.clone();
        self.transaction(move |b, s| {
            b.peer(s, &auth2)?;
            if !s.registry.native_grants.iter().any(|g| {
                g.client_id == auth2.id
                    && g.extension_id == extension_id
                    && g.profile_id == profile_id
            }) {
                if s.registry.native_grants.len() >= MAX_NATIVE_GRANTS {
                    return Err(ErrorCode::Capacity);
                }
                // Durable refusal before prompting: crash/denial must not start a
                // new unattended prompt loop. An explicit Retry can ask again.
                b.audit(s, "standalone_browser_pairing_requested", profile_id)?;
                s.registry.native_grants.push(NativeGrant {
                    client_id: auth2.id,
                    extension_id,
                    profile_id,
                    token_hash: profile_digest,
                    allowed: false,
                });
                b.save(s)?;
            }
            Ok(())
        })
        .await?;
        if !self.human.confirm(&format!("Allow automatic browser connections for client {label}, extension {}, profile {profile_id}? Check the profile ID on its setup page. This remembers target-discovery access on permitted sites, not credential use. Every fill still needs separate permission and consent. Disconnecting this browser through MagicVault revokes reconnection.", hello.extension_id), cancel.clone()).await? {
            return Err(ErrorCode::Denied);
        }
        let auth2 = auth.clone();
        let extension_id = hello.extension_id.clone();
        self.transaction(move |b, s| {
            b.peer(s, &auth2)?;
            if cancel.is_cancelled() {
                return Err(ErrorCode::Cancelled);
            }
            let grant = s
                .registry
                .native_grants
                .iter_mut()
                .find(|g| {
                    g.client_id == auth2.id
                        && g.extension_id == extension_id
                        && g.profile_id == profile_id
                        && equal_digest(&g.token_hash, &profile_digest)
                })
                .ok_or(ErrorCode::Unauthorized)?;
            grant.allowed = true;
            b.save(s)?;
            b.audit(s, "standalone_browser_paired", profile_id)?;
            Ok(())
        })
        .await?;
        Ok((
            auth,
            NativeIdentity {
                profile_id,
                extension_id: hello.extension_id.clone(),
            },
        ))
    }

    pub(super) async fn configure_browser_credential(
        self: &Arc<Self>,
        auth: Auth,
        id: Uuid,
        mut rule: BrowserRule,
    ) -> Result<Response, ErrorCode> {
        for origin in &mut rule.origins {
            *origin = canonical_origin(origin)?;
        }
        if !rule.valid() {
            return Err(ErrorCode::InvalidRequest);
        }
        let _permit = Arc::clone(&self.human_gate)
            .try_acquire_owned()
            .map_err(|_| ErrorCode::Busy)?;
        let (auth2, rule2) = (auth.clone(), rule.clone());
        let (label, credential) = self
            .transaction(move |b, s| {
                let peer = b.peer(s, &auth2)?;
                if !peer.references.contains(&rule2.credential_ref) {
                    return Err(ErrorCode::Denied);
                }
                let metadata = b.metadata(&rule2.credential_ref).ok_or(ErrorCode::Denied)?;
                if rule2
                    .field_names
                    .iter()
                    .any(|f| !metadata.field_names.contains(f))
                {
                    return Err(ErrorCode::Denied);
                }
                Ok((peer.label.clone(), metadata.label))
            })
            .await?;
        let message = rule_prompt(&label, &credential, &rule);
        if !self.human.confirm(&message, self.shutdown.clone()).await? {
            return Err(ErrorCode::Denied);
        }
        self.transaction(move |b, s| {
            let peer = b.peer(s, &auth)?;
            if !peer.references.contains(&rule.credential_ref) {
                return Err(ErrorCode::Denied);
            }
            if !s
                .registry
                .browser_permissions
                .iter()
                .any(|p| p.client_id == auth.id && p.rule.credential_ref == rule.credential_ref)
                && !rule.origins.is_empty()
                && s.registry.browser_permissions.len() >= MAX_BROWSER_PERMISSIONS
            {
                return Err(ErrorCode::Capacity);
            }
            b.audit(s, "standalone_browser_policy_requested", id)?;
            s.registry.browser_permissions.retain(|p| {
                !(p.client_id == auth.id && p.rule.credential_ref == rule.credential_ref)
            });
            if !rule.origins.is_empty() {
                s.registry.browser_permissions.push(BrowserPermission {
                    client_id: auth.id,
                    rule,
                });
            }
            // A policy update cancels outstanding effects of this client. A
            // write already delivered to a recipient cannot be rolled back.
            for job in s.browsers.fills.values().filter(|j| j.owner == auth.id) {
                job.cancel.cancel();
            }
            b.save(s)?;
            b.audit(s, "standalone_browser_policy_configured", id)?;
            Ok(Response::BrowserCredentialConfigured)
        })
        .await
    }

    pub(super) async fn register_cdp(
        self: &Arc<Self>,
        auth: Auth,
        id: Uuid,
        params: RegisterCdp,
    ) -> Result<Response, ErrorCode> {
        magicvault_effect::cdp::validate_endpoint(&params.endpoint)?;
        let _permit = Arc::clone(&self.human_gate)
            .try_acquire_owned()
            .map_err(|_| ErrorCode::Busy)?;
        let auth2 = auth.clone();
        let label = self
            .transaction(move |b, s| {
                let label = b.peer(s, &auth2)?.label.clone();
                if s.browsers.instances.len() >= MAX_BROWSERS {
                    return Err(ErrorCode::Capacity);
                }
                Ok(label)
            })
            .await?;
        if !self.human.confirm(&format!("Client {label} requests attaching MagicVault to browser {} at {}. Only connect a trusted, dedicated local automation profile. This grants target discovery, not credential use. Allow?",params.label,params.endpoint),self.shutdown.clone()).await? { return Err(ErrorCode::Denied); }
        let adapter = CdpBrowser::connect(&params.endpoint).await?;
        let info = BrowserInfo {
            browser_handle: Uuid::new_v4(),
            label: params.label,
            backend: BrowserBackend::Cdp,
        };
        self.register_adapter(auth, id, info, adapter, None).await
    }

    async fn register_adapter(
        self: &Arc<Self>,
        auth: Auth,
        id: Uuid,
        info: BrowserInfo,
        adapter: Arc<dyn BrowserAdapter>,
        native_profile: Option<NativeIdentity>,
    ) -> Result<Response, ErrorCode> {
        self.transaction(move |b, s| {
            b.peer(s, &auth)?;
            if let Some(profile) = &native_profile {
                if !s.registry.native_grants.iter().any(|g| {
                    g.client_id == auth.id
                        && g.profile_id == profile.profile_id
                        && g.extension_id == profile.extension_id
                        && g.allowed
                }) {
                    return Err(ErrorCode::Denied);
                }
                if s.browsers
                    .instances
                    .values()
                    .any(|p| p.owner == auth.id && p.native_profile.as_ref() == Some(profile))
                {
                    return Err(ErrorCode::Conflict);
                }
            }
            if s.browsers.instances.len() >= MAX_BROWSERS {
                return Err(ErrorCode::Capacity);
            }
            b.audit(s, "standalone_browser_registered", id)?;
            s.browsers.instances.insert(
                info.browser_handle,
                RegisteredBrowser {
                    owner: auth.id,
                    native_profile,
                    info: info.clone(),
                    adapter,
                },
            );
            Ok(Response::Browser(info))
        })
        .await
    }

    pub(super) async fn list_browsers(self: &Arc<Self>, auth: Auth) -> Result<Response, ErrorCode> {
        self.transaction(move |b, s| {
            b.peer(s, &auth)?;
            let mut rows = s
                .browsers
                .instances
                .values()
                .filter(|b| b.owner == auth.id)
                .map(|b| b.info.clone())
                .collect::<Vec<_>>();
            rows.sort_by_key(|b| b.browser_handle);
            Ok(Response::Browsers(rows))
        })
        .await
    }

    pub(super) async fn browser_targets(
        self: &Arc<Self>,
        auth: Auth,
        query: BrowserTargetsQuery,
    ) -> Result<Response, ErrorCode> {
        let auth2 = auth.clone();
        let handle = query.browser_handle;
        let filter = TargetFilter {
            top_origin: query.top_origin,
            tab_id: query.tab_id,
        };
        if !valid_target_filter(&filter) {
            return Err(ErrorCode::InvalidRequest);
        }
        let adapter = self
            .transaction(move |b, s| {
                b.peer(s, &auth2)?;
                let browser = s
                    .browsers
                    .instances
                    .get(&handle)
                    .filter(|b| b.owner == auth2.id)
                    .ok_or(ErrorCode::NotFound)?;
                Ok(Arc::clone(&browser.adapter))
            })
            .await?;
        let targets = adapter
            .targets_filtered(&filter, self.shutdown.child_token())
            .await?;
        if targets.len() > MAX_TARGETS
            || targets
                .iter()
                .any(|t| !t.valid() || !matches_target_filter(t, &filter))
        {
            adapter.disconnect();
            return Err(ErrorCode::TransportUncertain);
        }
        self.transaction(move |b, s| {
            b.peer(s, &auth)?;
            if !s.browsers.instances.contains_key(&handle) {
                return Err(ErrorCode::StaleSession);
            }
            // Rediscovery invalidates unused old handles, never a pending job's
            // captured binding. Each discovered handle can be consumed once.
            s.browsers.targets.retain(|_, t| t.browser != handle);
            let remaining = MAX_TARGETS.saturating_sub(s.browsers.targets.len());
            if targets.len() > remaining {
                return Err(ErrorCode::Capacity);
            }
            let mut rows = Vec::new();
            for target in targets {
                let target_handle = Uuid::new_v4();
                rows.push(BrowserTarget {
                    target_handle,
                    browser_handle: handle,
                    tab_id: target.tab.clone(),
                    frame_id: target.frame.clone(),
                    origin: target.origin.clone(),
                    top_origin: target.top_origin.clone(),
                    is_main_frame: target.is_main_frame,
                    expires_in_seconds: TARGET_TTL_SECS,
                });
                s.browsers.targets.insert(
                    target_handle,
                    BoundTarget {
                        owner: auth.id,
                        browser: handle,
                        target,
                        expires: Instant::now() + Duration::from_secs(TARGET_TTL_SECS),
                    },
                );
            }
            Ok(Response::BrowserTargets(rows))
        })
        .await
    }

    pub(super) async fn disconnect_browser(
        self: &Arc<Self>,
        auth: Auth,
        id: Uuid,
        query: BrowserQuery,
    ) -> Result<Response, ErrorCode> {
        self.transaction(move |b, s| {
            b.peer(s, &auth)?;
            let browser = s
                .browsers
                .instances
                .get(&query.browser_handle)
                .filter(|b| b.owner == auth.id)
                .ok_or(ErrorCode::NotFound)?;
            browser.adapter.disconnect();
            let native_profile = browser.native_profile.clone();
            if let Some(profile) = native_profile {
                for grant in s.registry.native_grants.iter_mut().filter(|g| {
                    g.client_id == auth.id
                        && g.profile_id == profile.profile_id
                        && g.extension_id == profile.extension_id
                }) {
                    grant.allowed = false;
                }
                b.save(s)?;
            }
            s.browsers.instances.remove(&query.browser_handle);
            s.browsers
                .targets
                .retain(|_, t| t.browser != query.browser_handle);
            for job in s
                .browsers
                .fills
                .values()
                .filter(|j| j.request.browser_handle == query.browser_handle)
            {
                job.cancel.cancel();
            }
            b.audit(s, "standalone_browser_disconnected", id)?;
            Ok(Response::BrowserDisconnected)
        })
        .await
    }

    pub(super) async fn secure_fill(
        self: &Arc<Self>,
        auth: Auth,
        request: SecureFill,
    ) -> Result<Response, ErrorCode> {
        let (auth2, req2) = (auth.clone(), request.clone());
        let reserved = self
            .transaction(move |b, s| {
                let peer = b.peer(s, &auth2)?;
                let label = peer.label.clone();
                if let Some(job) = s.browsers.fills.get(&req2.operation_id) {
                    if job.owner != auth2.id || job.request != req2 {
                        return Err(ErrorCode::Conflict);
                    }
                    return Ok((job.status.clone(), None));
                }
                if s.browsers.used_operations.contains(&req2.operation_id) {
                    return Err(ErrorCode::Conflict);
                }
                if s.browsers.used_operations.len() >= 4096 {
                    return Err(ErrorCode::Capacity);
                }
                if s.browsers.fills.len() >= MAX_FILL_JOBS {
                    return Err(ErrorCode::Capacity);
                }
                let bound = s
                    .browsers
                    .targets
                    .get(&req2.target_handle)
                    .filter(|t| t.owner == auth2.id && t.browser == req2.browser_handle)
                    .cloned()
                    .ok_or(ErrorCode::StaleTarget)?;
                if req2.fields.iter().any(|f| {
                    !peer.references.contains(&f.credential_ref)
                        || !allowed(s, &auth2, &bound.target, f)
                        || b.metadata(&f.credential_ref)
                            .is_none_or(|m| !m.field_names.contains(&f.credential_field))
                }) {
                    return Err(ErrorCode::Denied);
                }
                let adapter = Arc::clone(
                    &s.browsers
                        .instances
                        .get(&req2.browser_handle)
                        .ok_or(ErrorCode::StaleTarget)?
                        .adapter,
                );
                let permit = Arc::clone(&b.human_gate)
                    .try_acquire_owned()
                    .map_err(|_| ErrorCode::Busy)?;
                b.audit(s, "standalone_fill_requested", req2.operation_id)?;
                // Consume the handle before launching a human job. Replaying after
                // status retention expires cannot execute the old binding again.
                s.browsers.targets.remove(&req2.target_handle);
                s.browsers.used_operations.insert(req2.operation_id);
                let cancel = b.shutdown.child_token();
                let status = FillStatus {
                    operation_id: req2.operation_id,
                    state: FillState::Pending,
                    fields: vec![FieldState::NotFilled; req2.fields.len()],
                    error: None,
                };
                s.browsers.fills.insert(
                    req2.operation_id,
                    FillJob {
                        owner: auth2.id,
                        request: req2,
                        status: status.clone(),
                        cancel: cancel.clone(),
                        retain_until: Instant::now() + Duration::from_secs(600),
                    },
                );
                Ok((status, Some((label, bound, adapter, permit, cancel))))
            })
            .await?;
        let (status, work) = reserved;
        if let Some((label, bound, adapter, permit, cancel)) = work {
            let broker = Arc::clone(self);
            tokio::spawn(async move {
                let _permit = permit; // Held through native cleanup and final audit.
                broker
                    .run_fill(auth, request, label, bound, adapter, cancel)
                    .await;
            });
        }
        Ok(Response::Fill(status))
    }

    async fn run_fill(
        self: &Arc<Self>,
        auth: Auth,
        request: SecureFill,
        label: String,
        bound: BoundTarget,
        adapter: Arc<dyn BrowserAdapter>,
        cancel: CancellationToken,
    ) {
        let id = request.operation_id;
        let count = request.fields.len();
        let message = fill_prompt(&label, &request, &bound.target);
        let decision = self.human.confirm(&message, cancel.clone());
        tokio::pin!(decision);
        let confirmed = tokio::select! {
            result = &mut decision => result,
            _ = tokio::time::sleep(bound.expires.saturating_duration_since(Instant::now())) => {
                cancel.cancel(); let _ = decision.await; Err(ErrorCode::Expired)
            },
        };
        let delivery = match confirmed {
            Ok(true) if !cancel.is_cancelled() => {
                let (auth2, req2, target2, cancel2) = (
                    auth.clone(),
                    request.clone(),
                    bound.target.clone(),
                    cancel.clone(),
                );
                self.transaction(move |b, s| {
                    let peer = b.peer(s, &auth2)?;
                    if cancel2.is_cancelled() {
                        return Err(ErrorCode::Cancelled);
                    }
                    if Instant::now() >= bound.expires {
                        return Err(ErrorCode::Expired);
                    }
                    if req2.fields.iter().any(|f| {
                        !peer.references.contains(&f.credential_ref)
                            || !allowed(s, &auth2, &target2, f)
                    }) {
                        return Err(ErrorCode::Denied);
                    }
                    if !s.browsers.instances.contains_key(&req2.browser_handle) {
                        return Err(ErrorCode::StaleTarget);
                    }
                    b.audit(s, "standalone_fill_authorized", id)?;
                    if cancel2.is_cancelled() {
                        return Err(ErrorCode::Cancelled);
                    }
                    if Instant::now() >= bound.expires {
                        return Err(ErrorCode::Expired);
                    }
                    let mut fields = Vec::with_capacity(req2.fields.len());
                    let mut entries = HashMap::<String, SecretInput>::new();
                    for field in &req2.fields {
                        // Core returns an owned trusted entry. Erase every
                        // cloned field, including unselected fields, on exit.
                        if !entries.contains_key(&field.credential_ref) {
                            let entry = b
                                .store
                                .get_provisioned(&field.credential_ref)
                                .ok_or(ErrorCode::Denied)?;
                            entries.insert(field.credential_ref.clone(), SecretInput(entry.fields));
                        }
                        let value = entries
                            .get(&field.credential_ref)
                            .and_then(|entry| entry.0.get(&field.credential_field))
                            .cloned()
                            .ok_or(ErrorCode::Denied)?;
                        let material = MaterialField {
                            css: field.css.clone(),
                            value,
                        };
                        if material.value.is_empty() || material.value.len() > 4096 {
                            return Err(ErrorCode::InvalidRequest);
                        }
                        fields.push(material);
                    }
                    // Custody resolution can take time. Check once more before
                    // material leaves the writer; all temporary fields erase on
                    // an early return, including unselected cloned entry fields.
                    if cancel2.is_cancelled() {
                        return Err(ErrorCode::Cancelled);
                    }
                    if Instant::now() >= bound.expires {
                        return Err(ErrorCode::Expired);
                    }
                    let job = s
                        .browsers
                        .fills
                        .get_mut(&id)
                        .ok_or(ErrorCode::Unavailable)?;
                    if job.status.state != FillState::Pending {
                        return Err(ErrorCode::Conflict);
                    }
                    // Linearization point: authority checked and material
                    // released for this one effect. Revocation cancels further
                    // delivery but cannot recall an already-applied DOM write.
                    job.status.state = FillState::Filling;
                    Ok(fields)
                })
                .await
            }
            Ok(_) if cancel.is_cancelled() => Err(ErrorCode::Cancelled),
            Ok(_) => Err(ErrorCode::Denied),
            Err(ErrorCode::Expired) => Err(ErrorCode::Expired),
            Err(_) if cancel.is_cancelled() => Err(ErrorCode::Cancelled),
            Err(error) => Err(error),
        };
        let outcome = match delivery {
            Ok(fields) => adapter.fill(&bound.target, fields, cancel.clone()).await,
            Err(error) => Outcome::failed(count, error),
        };
        let _ = self
            .transaction(move |b, s| {
                let outcome = if outcome.valid(count) {
                    outcome
                } else {
                    Outcome::uncertain(count)
                };
                let any_written = outcome.fields.iter().any(|v| *v == FieldState::Filled);
                let uncertain = outcome.fields.contains(&FieldState::Uncertain);
                let status = if uncertain {
                    FillState::Uncertain
                } else if outcome.fields.iter().all(|v| *v == FieldState::Filled)
                    && outcome.error.is_none()
                {
                    FillState::Filled
                } else if any_written {
                    FillState::Partial
                } else {
                    match outcome.error {
                        Some(
                            ErrorCode::Denied
                            | ErrorCode::Unauthorized
                            | ErrorCode::PermissionDenied,
                        ) => FillState::Denied,
                        Some(ErrorCode::Expired) => FillState::Expired,
                        Some(ErrorCode::Cancelled) => FillState::Cancelled,
                        _ => FillState::Failed,
                    }
                };
                let receipt = FillReceipt {
                    operation_id: id,
                    client_id: auth.id,
                    browser_handle: request.browser_handle,
                    target_handle: request.target_handle,
                    state: status,
                    fields: outcome.fields.clone(),
                    error: outcome.error,
                };
                let audit = b
                    .store
                    .try_audit_event_durably(
                        magicvault_core::store::AuditEvent::new("standalone_fill_completed")
                            .with_tool("magicvault")
                            .with_action("secure_fill")
                            .with_runtime_credential_receipt(receipt),
                    )
                    .map_err(|_| {
                        s.faulted = true;
                        ErrorCode::PersistenceUncertain
                    });
                if let Some(job) = s.browsers.fills.get_mut(&id) {
                    job.status = FillStatus {
                        operation_id: id,
                        state: if audit.is_ok() {
                            status
                        } else {
                            FillState::Uncertain
                        },
                        fields: outcome.fields,
                        error: if audit.is_ok() {
                            outcome.error
                        } else {
                            Some(ErrorCode::PersistenceUncertain)
                        },
                    };
                }
                audit
            })
            .await;
    }

    pub(super) async fn fill_status(
        self: &Arc<Self>,
        auth: Auth,
        query: FillQuery,
        cancel: bool,
    ) -> Result<Response, ErrorCode> {
        self.transaction(move |b, s| {
            // Read-only reconciliation is allowed after audit uncertainty, but
            // still authenticates the caller. No new effect can be authorized.
            let valid = s
                .registry
                .peers
                .iter()
                .any(|p| p.id == auth.id && equal_digest(&p.token_hash, &auth.digest));
            if !valid {
                return Err(ErrorCode::Unauthorized);
            }
            if cancel {
                b.ready(s)?;
            }
            let job = s
                .browsers
                .fills
                .get_mut(&query.operation_id)
                .filter(|j| j.owner == auth.id)
                .ok_or(ErrorCode::NotFound)?;
            if cancel {
                job.cancel.cancel();
            }
            Ok(Response::Fill(job.status.clone()))
        })
        .await
    }
}
