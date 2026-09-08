//! One core store and one serialized control-state writer. Native human waits
//! run outside every store/state lock; completion revalidates caller authority.
use crate::{
    human::HumanInteraction,
    storage::{self, InstanceLock},
};
use magicvault_core::{
    policy::SecretPolicy,
    store::{SecretAuditEvent, SecretPartitionStatus, SecretSourceKind, SecretStore},
    InjectionTarget,
};
use magicvault_primitives::durable_io::write_bytes_durably_with_mode_sync;
use magicvault_protocol::*;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeSet, HashMap},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;
use zeroize::{Zeroize, Zeroizing};
mod browser;
use browser::{BrowserPermission, BrowserState, NativeGrant};
mod delivery;
use delivery::{DeliveryJobs, RegisteredDelivery};
mod consent;
use consent::{ConsentGrant, ConsentTicket};

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Peer {
    id: Uuid,
    label: String,
    token_hash: [u8; 32],
    references: BTreeSet<String>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Registry {
    version: u32,
    peers: Vec<Peer>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    browser_permissions: Vec<BrowserPermission>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    native_grants: Vec<NativeGrant>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    delivery_profiles: Vec<RegisteredDelivery>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    consents: Vec<ConsentGrant>,
}

#[derive(Clone)]
struct Auth {
    id: Uuid,
    digest: [u8; 32],
}

struct Job {
    owner: Uuid,
    reference: String,
    decision: Decision,
    expires: Instant,
    retain_until: Instant,
}
impl Job {
    fn projection(&self, id: Uuid) -> ApprovalStatus {
        ApprovalStatus {
            approval_id: id,
            decision: self.decision,
            operation: "metadata_connection".into(),
        }
    }
}

struct State {
    registry: Registry,
    jobs: HashMap<Uuid, Job>,
    browsers: BrowserState,
    deliveries: DeliveryJobs,
    consent_epochs: HashMap<Uuid, Uuid>,
    faulted: bool,
}
impl State {
    fn existing_approval(
        &self,
        id: Uuid,
        owner: Uuid,
        reference: &str,
    ) -> Result<Option<ApprovalStatus>, ErrorCode> {
        let Some(job) = self.jobs.get(&id) else {
            return Ok(None);
        };
        if job.owner != owner || job.reference != reference {
            return Err(ErrorCode::Conflict);
        }
        Ok(Some(job.projection(id)))
    }

    fn reap(&mut self, now: Instant) {
        self.browsers.reap(now);
        self.deliveries.reap(now);
        self.registry.consents.retain(|g| {
            consent::session_scope(&g.info.scope)
                .is_none_or(|handle| self.browsers.session_live(handle))
        });
        self.jobs.retain(|_, job| job.retain_until > now);
        for job in self.jobs.values_mut() {
            if job.decision == Decision::Pending && job.expires <= now {
                job.decision = Decision::Expired;
            }
        }
    }
}

pub struct Broker {
    lease: Arc<InstanceLock>,
    store: SecretStore,
    state: Mutex<State>,
    human: Arc<dyn HumanInteraction>,
    human_gate: Arc<Semaphore>,
    background_jobs: tokio_util::task::TaskTracker,
    pub epoch: Uuid,
    pub shutdown: CancellationToken,
}

struct SecretInput(HashMap<String, String>);
impl Drop for SecretInput {
    fn drop(&mut self) {
        for value in self.0.values_mut() {
            value.zeroize();
        }
    }
}

fn token_hash(token: &str) -> [u8; 32] {
    Sha256::digest(token.as_bytes()).into()
}
fn equal_digest(left: &[u8; 32], right: &[u8; 32]) -> bool {
    left.iter()
        .zip(right.iter())
        .fold(0u8, |difference, (a, b)| difference | (a ^ b))
        == 0
}

fn ensure_enrollment_live(deadline: Instant) -> Result<(), ErrorCode> {
    if Instant::now() >= deadline {
        Err(ErrorCode::Expired)
    } else {
        Ok(())
    }
}

impl Broker {
    pub fn open(
        lease: Arc<InstanceLock>,
        human: Arc<dyn HumanInteraction>,
    ) -> Result<Arc<Self>, ErrorCode> {
        storage::validate_vault(lease.root())?;
        let key = storage::load_key(&lease.instance)?;
        let store = SecretStore::new(Box::new(key), lease.root().join("vault"))
            .map_err(|_| ErrorCode::Unavailable)?;
        Self::with_components(lease, store, human)
    }

    /// Trusted embedding seam. Executables always use the native key/human path.
    /// This is not an RPC or an auto-approval configuration flag.
    pub fn with_components(
        lease: Arc<InstanceLock>,
        store: SecretStore,
        human: Arc<dyn HumanInteraction>,
    ) -> Result<Arc<Self>, ErrorCode> {
        if store.base_dir() != lease.root().join("vault") {
            return Err(ErrorCode::InvalidRequest);
        }
        storage::validate_vault(lease.root())?;
        if store.partition_status(SecretSourceKind::Provisioned) != SecretPartitionStatus::Loaded
            || !store
                .feature_status(SecretSourceKind::Provisioned)
                .is_available()
        {
            return Err(ErrorCode::Unavailable);
        }
        let credentials = store.list_available();
        if credentials.len() > MAX_CREDENTIALS
            || credentials.iter().any(|entry| {
                !valid_reference(&entry.id)
                    || !valid_label(&entry.label)
                    || store
                        .provisioned_metadata(&entry.id)
                        .is_none_or(|metadata| {
                            metadata.field_names.is_empty()
                                || metadata.field_names.len() > MAX_FIELDS
                                || metadata.field_names.iter().any(|field| !valid_name(field))
                        })
            })
        {
            return Err(ErrorCode::Unavailable);
        }
        let path = lease.root().join("clients.json");
        let mut registry = match path.symlink_metadata() {
            Ok(_) => {
                let bytes = storage::read_private(&path, storage::MAX_STATE_BYTES)?;
                serde_json::from_slice::<Registry>(&bytes).map_err(|_| ErrorCode::Unavailable)?
            }
            Err(error)
                if error.kind() == std::io::ErrorKind::NotFound && credentials.is_empty() =>
            {
                Registry {
                    version: 1,
                    peers: Vec::new(),
                    browser_permissions: Vec::new(),
                    native_grants: Vec::new(),
                    delivery_profiles: Vec::new(),
                    consents: Vec::new(),
                }
            }
            Err(_) => return Err(ErrorCode::Unavailable),
        };
        if registry.version != 1
            || registry.peers.len() > MAX_CLIENTS
            || !browser::valid_permissions(&registry, &store)
            || !browser::valid_native_grants(&registry)
            || !delivery::valid_profiles(&registry, &store)
            || !consent::valid_grants(&registry)
            || registry.peers.iter().any(|p| {
                !valid_label(&p.label)
                    || p.references.len() > MAX_CREDENTIALS
                    || p.references
                        .iter()
                        .any(|r| !valid_reference(r) || !store.contains_provisioned(r))
            })
            || registry
                .peers
                .iter()
                .map(|p| p.id)
                .collect::<BTreeSet<_>>()
                .len()
                != registry.peers.len()
            || registry
                .peers
                .iter()
                .map(|p| &p.label)
                .collect::<BTreeSet<_>>()
                .len()
                != registry.peers.len()
        {
            return Err(ErrorCode::Unavailable);
        }
        // Connection-scoped CDP/custom-adapter approvals never survive restart.
        registry
            .consents
            .retain(|g| consent::session_scope(&g.info.scope).is_none());
        Ok(Arc::new(Self {
            lease,
            store,
            state: Mutex::new(State {
                registry,
                jobs: HashMap::new(),
                browsers: BrowserState::default(),
                deliveries: DeliveryJobs::default(),
                consent_epochs: HashMap::new(),
                faulted: false,
            }),
            human,
            human_gate: Arc::new(Semaphore::new(1)),
            background_jobs: tokio_util::task::TaskTracker::new(),
            epoch: Uuid::new_v4(),
            shutdown: CancellationToken::new(),
        }))
    }

    pub fn socket(&self) -> std::path::PathBuf {
        self.lease.socket()
    }

    pub async fn quiesce(self: &Arc<Self>) {
        self.shutdown.cancel();
        // The writer may be finishing filesystem I/O. Wait for its mutex on
        // the blocking lane, never on a Tokio worker thread.
        let _ = self
            .transaction(|_, state| {
                state.browsers.stop_all();
                state.deliveries.stop_all();
                Ok(())
            })
            .await;
        // Admission remains held through native-child cleanup and completion.
        // After acquiring it, no reserved job can still be waiting to spawn:
        // reservations own the permit and shutdown rejects new reservations.
        let _permit = self.human_gate.acquire().await;
        // Terminal publication releases admission before the async worker wakes.
        // Wait for those futures/destructors too, including their broker/lease
        // ownership, so immediate restart never races the old instance lock.
        self.background_jobs.close();
        self.background_jobs.wait().await;
    }

    async fn transaction<T: Send + 'static>(
        self: &Arc<Self>,
        operation: impl FnOnce(&Self, &mut State) -> Result<T, ErrorCode> + Send + 'static,
    ) -> Result<T, ErrorCode> {
        let broker = Arc::clone(self);
        tokio::task::spawn_blocking(move || {
            let mut state = broker.state.lock().map_err(|_| ErrorCode::Unavailable)?;
            state.reap(Instant::now());
            operation(&broker, &mut state)
        })
        .await
        .map_err(|_| ErrorCode::Unavailable)?
    }

    fn ready(&self, state: &State) -> Result<(), ErrorCode> {
        if state.faulted {
            return Err(ErrorCode::PersistenceUncertain);
        }
        if self.shutdown.is_cancelled() {
            return Err(ErrorCode::Unavailable);
        }
        Ok(())
    }

    fn peer<'a>(&self, state: &'a State, auth: &Auth) -> Result<&'a Peer, ErrorCode> {
        self.ready(state)?;
        state
            .registry
            .peers
            .iter()
            .find(|p| p.id == auth.id && equal_digest(&p.token_hash, &auth.digest))
            .ok_or(ErrorCode::Unauthorized)
    }

    fn save(&self, state: &mut State) -> Result<(), ErrorCode> {
        let bytes = serde_json::to_vec(&state.registry).map_err(|_| ErrorCode::Unavailable)?;
        if bytes.len() as u64 > storage::MAX_STATE_BYTES {
            state.faulted = true;
            return Err(ErrorCode::PersistenceUncertain);
        }
        if write_bytes_durably_with_mode_sync(
            &self.lease.root().join("clients.json"),
            &bytes,
            Some(0o600),
        )
        .is_err()
        {
            state.faulted = true;
            return Err(ErrorCode::PersistenceUncertain);
        }
        Ok(())
    }

    fn audit(&self, state: &mut State, event: &'static str, id: Uuid) -> Result<(), ErrorCode> {
        let event = SecretAuditEvent::new(event)
            .with_tool("magicvault")
            .with_detail(id.to_string());
        if self.store.try_audit_event_durably(event).is_err() {
            state.faulted = true;
            return Err(ErrorCode::PersistenceUncertain);
        }
        Ok(())
    }

    fn metadata(&self, reference: &str) -> Option<CredentialMetadata> {
        self.store
            .provisioned_metadata(reference)
            .filter(|metadata| {
                valid_reference(&metadata.id)
                    && valid_label(&metadata.label)
                    && metadata.field_names.len() <= MAX_FIELDS
                    && metadata.field_names.iter().all(|name| valid_name(name))
            })
            .map(|metadata| CredentialMetadata {
                credential_ref: metadata.id,
                label: metadata.label,
                field_names: metadata.field_names,
            })
    }

    pub async fn execute(self: &Arc<Self>, envelope: Envelope) -> Reply {
        match self.dispatch(envelope).await {
            Ok(response) => Reply::Ok(response),
            Err(error) => Reply::Error(error),
        }
    }

    async fn dispatch(self: &Arc<Self>, envelope: Envelope) -> Result<Response, ErrorCode> {
        if envelope.version != VERSION {
            return Err(ErrorCode::UnsupportedVersion);
        }
        envelope.request.validate()?;
        let request = envelope.request.clone();
        let id = envelope.request_id;
        if !matches!(request, Request::Status) && envelope.epoch != Some(self.epoch) {
            return Err(ErrorCode::StaleSession);
        }
        let digest = envelope
            .token
            .as_deref()
            .map(|token| {
                if token.len() != 64 || !token.bytes().all(|b| b.is_ascii_hexdigit()) {
                    return Err(ErrorCode::Unauthorized);
                }
                Ok(token_hash(token))
            })
            .transpose()?;
        let auth = self
            .transaction(move |broker, state| {
                let auth = digest.and_then(|digest| {
                    state
                        .registry
                        .peers
                        .iter()
                        .find(|p| equal_digest(&p.token_hash, &digest))
                        .map(|p| Auth { id: p.id, digest })
                });
                if digest.is_some() && auth.is_none() {
                    return Err(ErrorCode::Unauthorized);
                }
                if !matches!(
                    request,
                    Request::Status | Request::FillStatus(_) | Request::DeliveryStatus(_)
                ) {
                    broker.ready(state)?;
                }
                Ok(auth)
            })
            .await?;
        match envelope.request.clone() {
            Request::Status => {
                let client_id = auth.map(|a| a.id);
                self.transaction(move |broker, state| {
                    Ok(Response::Status(ServiceStatus {
                        epoch: broker.epoch,
                        client_id,
                        ready: !state.faulted && !broker.shutdown.is_cancelled(),
                        effects: vec![
                            "secure_fill".into(),
                            "secure_new_process".into(),
                            "secure_new_http".into(),
                        ],
                    }))
                })
                .await
            }
            Request::Pair(params) => {
                if auth.is_some() {
                    return Err(ErrorCode::InvalidRequest);
                }
                self.pair(id, params).await
            }
            request => {
                let auth = auth.ok_or(ErrorCode::Unauthorized)?;
                match request {
                    Request::Enroll(params) => self.enroll(auth, id, params).await,
                    Request::ListCredentials => {
                        self.transaction(move |broker, state| {
                            let peer = broker.peer(state, &auth)?;
                            let mut rows = peer
                                .references
                                .iter()
                                .filter_map(|r| broker.metadata(r))
                                .collect::<Vec<_>>();
                            rows.sort_by(|a, b| {
                                a.label
                                    .cmp(&b.label)
                                    .then(a.credential_ref.cmp(&b.credential_ref))
                            });
                            Ok(Response::Credentials(rows))
                        })
                        .await
                    }
                    Request::RequestAccess(params) => self.request_access(auth, id, params).await,
                    Request::ApprovalStatus(query) => {
                        self.transaction(move |broker, state| {
                            broker.peer(state, &auth)?;
                            let job = state
                                .jobs
                                .get(&query.approval_id)
                                .filter(|j| j.owner == auth.id)
                                .ok_or(ErrorCode::NotFound)?;
                            Ok(Response::Approval(job.projection(query.approval_id)))
                        })
                        .await
                    }
                    Request::RevokeClient(params) => self.revoke(auth, id, params).await,
                    Request::RegisterCdp(params) => self.register_cdp(auth, id, params).await,
                    Request::ListBrowsers => self.list_browsers(auth).await,
                    Request::BrowserTargets(query) => self.browser_targets(auth, query).await,
                    Request::DisconnectBrowser(query) => {
                        self.disconnect_browser(auth, id, query).await
                    }
                    Request::ConfigureBrowserCredential(rule) => {
                        self.configure_browser_credential(auth, id, rule).await
                    }
                    Request::SecureFill(request) => self.secure_fill(auth, request).await,
                    Request::FillStatus(query) => self.fill_status(auth, query, false).await,
                    Request::CancelFill(query) => self.fill_status(auth, query, true).await,
                    Request::RegisterDeliveryProfile(profile) => {
                        self.register_delivery(auth, id, profile).await
                    }
                    Request::ListDeliveryProfiles => self.list_deliveries(auth).await,
                    Request::RemoveDeliveryProfile(query) => {
                        self.remove_delivery(auth, id, query).await
                    }
                    Request::SecureNewProcess(request) => {
                        self.secure_delivery(auth, request, DeliveryKind::Process)
                            .await
                    }
                    Request::SecureNewHttp(request) => {
                        self.secure_delivery(auth, request, DeliveryKind::Http)
                            .await
                    }
                    Request::DeliveryStatus(query) => {
                        self.delivery_status(auth, query, false).await
                    }
                    Request::CancelDelivery(query) => self.delivery_status(auth, query, true).await,
                    Request::ListConsents => self.list_consents(auth).await,
                    Request::RevokeConsent(query) => {
                        self.revoke_consent(auth, id, Some(query)).await
                    }
                    Request::ClearConsents => self.revoke_consent(auth, id, None).await,
                    Request::Shutdown => {
                        let _permit = Arc::clone(&self.human_gate)
                            .try_acquire_owned()
                            .map_err(|_| ErrorCode::Busy)?;
                        let auth2 = auth.clone();
                        let label = self
                            .transaction(move |b, s| Ok(b.peer(s, &auth2)?.label.clone()))
                            .await?;
                        if !self.human.confirm(&format!("Client {label} requests stopping this standalone MagicVault instance. Pending requests will be cancelled. Allow?"), self.shutdown.clone()).await? { return Err(ErrorCode::Denied); }
                        self.transaction(move |b, s| {
                            b.peer(s, &auth)?;
                            b.audit(s, "standalone_stopping", id)?;
                            b.shutdown.cancel();
                            Ok(Response::Stopping)
                        })
                        .await
                    }
                    _ => Err(ErrorCode::InvalidRequest),
                }
            }
        }
    }

    async fn pair(self: &Arc<Self>, id: Uuid, params: PairRequest) -> Result<Response, ErrorCode> {
        let _permit = Arc::clone(&self.human_gate)
            .try_acquire_owned()
            .map_err(|_| ErrorCode::Busy)?;
        let label = params.label;
        let proposed = label.clone();
        self.transaction(move |broker, state| {
            broker.ready(state)?;
            if state.registry.peers.len() >= MAX_CLIENTS {
                return Err(ErrorCode::Capacity);
            }
            if state.registry.peers.iter().any(|p| p.label == proposed) {
                return Err(ErrorCode::Conflict);
            }
            Ok(())
        })
        .await?;
        if !self.human.confirm(&format!("Pair local client {label} with MagicVault? It can request enrollment and metadata access, but cannot approve itself or read credential values. Only allow the client you just started."), self.shutdown.clone()).await? { return Err(ErrorCode::Denied); }
        let mut random = Zeroizing::new([0u8; 32]);
        rand::rngs::OsRng.fill_bytes(&mut *random);
        let token = Zeroizing::new(hex::encode(*random));
        let digest = token_hash(&token);
        let client_id = Uuid::new_v4();
        self.transaction(move |broker, state| {
            broker.ready(state)?;
            if state.registry.peers.len() >= MAX_CLIENTS
                || state.registry.peers.iter().any(|p| p.label == label)
            {
                return Err(ErrorCode::Conflict);
            }
            broker.audit(state, "standalone_pair_requested", id)?;
            state.registry.peers.push(Peer {
                id: client_id,
                label,
                token_hash: digest,
                references: BTreeSet::new(),
            });
            broker.save(state)?;
            broker.audit(state, "standalone_paired", id)?;
            Ok(())
        })
        .await?;
        Ok(Response::Paired(Pairing {
            client_id,
            token: token.to_string(),
        }))
    }

    async fn enroll(
        self: &Arc<Self>,
        auth: Auth,
        id: Uuid,
        params: EnrollRequest,
    ) -> Result<Response, ErrorCode> {
        let _permit = Arc::clone(&self.human_gate)
            .try_acquire_owned()
            .map_err(|_| ErrorCode::Busy)?;
        let reference = format!("cred_{id}");
        let auth2 = auth.clone();
        let ref2 = reference.clone();
        let peer_label = self
            .transaction(move |b, s| {
                let peer = b.peer(s, &auth2)?;
                if b.store.contains_provisioned(&ref2) {
                    return Err(ErrorCode::Conflict);
                }
                if b.store.list_available().len() >= MAX_CREDENTIALS {
                    return Err(ErrorCode::Capacity);
                }
                Ok(peer.label.clone())
            })
            .await?;
        let deadline = Instant::now() + Duration::from_secs(CONSENT_TTL_SECS);
        if !self.human.confirm(&format!("Client {peer_label} requests enrolling {} with fields {}. Names/label are non-secret metadata. Values are collected next in hidden prompts. This does not permit browser, HTTP or process delivery. Allow?", params.label, params.field_names.join(", ")), self.shutdown.clone()).await? { return Err(ErrorCode::Denied); }
        let mut fields = SecretInput(HashMap::new());
        for field in &params.field_names {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(ErrorCode::Expired);
            }
            let cancel = self.shutdown.child_token();
            let message = format!("Enroll {} for {peer_label}: enter {field}. Do not put credentials in names, terminal commands or chat.", params.label);
            let input = self.human.secret(&message, cancel.clone());
            tokio::pin!(input);
            let mut value = tokio::select! {
                value = &mut input => value?,
                _ = tokio::time::sleep(remaining) => {
                    cancel.cancel();
                    // Keep the native future alive until its child is reaped.
                    let _ = input.await;
                    return Err(ErrorCode::Expired);
                },
            };
            if value.is_empty() || value.len() > 4096 || value.contains(['\r', '\n']) {
                return Err(ErrorCode::InvalidRequest);
            }
            fields.0.insert(field.clone(), std::mem::take(&mut *value));
        }
        ensure_enrollment_live(deadline)?;
        self.transaction(move |broker, state| {
            broker.peer(state, &auth)?;
            if broker.store.contains_provisioned(&reference) {
                return Err(ErrorCode::Conflict);
            }
            ensure_enrollment_live(deadline)?;
            broker.audit(state, "standalone_enrollment_requested", id)?;
            let injection = InjectionTarget::FormFields(
                params
                    .field_names
                    .iter()
                    .map(|name| (name.clone(), name.clone()))
                    .collect(),
            );
            // Enrollment activates metadata only. Future delivery requires its
            // own policy configuration/target-bound human authorization.
            let policy = SecretPolicy {
                allowed_tools: vec!["magicvault:metadata".into()],
                requires_approval: true,
                ..SecretPolicy::default()
            };
            // Revalidate at the write boundary, including time in the blocking
            // queue/state lock and in the durable audit append.
            ensure_enrollment_live(deadline)?;
            if broker
                .store
                .store_provisioned(
                    &reference,
                    params.label,
                    std::mem::take(&mut fields.0),
                    injection,
                    policy,
                )
                .is_err()
            {
                state.faulted = true;
                return Err(ErrorCode::PersistenceUncertain);
            }
            state
                .registry
                .peers
                .iter_mut()
                .find(|p| p.id == auth.id)
                .ok_or(ErrorCode::Unauthorized)?
                .references
                .insert(reference.clone());
            broker.save(state)?;
            broker.audit(state, "standalone_enrolled", id)?;
            Ok(Response::Enrolled(
                broker.metadata(&reference).ok_or(ErrorCode::Unavailable)?,
            ))
        })
        .await
    }

    async fn request_access(
        self: &Arc<Self>,
        auth: Auth,
        id: Uuid,
        params: AccessRequest,
    ) -> Result<Response, ErrorCode> {
        let auth2 = auth.clone();
        let reference = params.credential_ref;
        let ref2 = reference.clone();
        let (interaction, status) = self
            .transaction(move |broker, state| {
                let client_label = broker.peer(state, &auth2)?.label.clone();
                // Lookup, nonblocking admission and reservation are one transaction.
                // Concurrent replays cannot race a separate optimistic lookup and
                // replace a completed job or reopen a second human prompt.
                if let Some(existing) = state.existing_approval(id, auth2.id, &ref2)? {
                    return Ok((None, existing));
                }
                let permit = Arc::clone(&broker.human_gate)
                    .try_acquire_owned()
                    .map_err(|_| ErrorCode::Busy)?;
                let credential = broker.metadata(&ref2).ok_or(ErrorCode::Denied)?;
                if state.jobs.len() >= MAX_PENDING {
                    return Err(ErrorCode::Capacity);
                }
                let now = Instant::now();
                broker.audit(state, "standalone_metadata_access_requested", id)?;
                let job = Job {
                    owner: auth2.id,
                    reference: ref2,
                    decision: Decision::Pending,
                    expires: now + Duration::from_secs(CONSENT_TTL_SECS),
                    retain_until: now + Duration::from_secs(600),
                };
                let status = job.projection(id);
                state.jobs.insert(id, job);
                Ok((Some((client_label, credential.label, permit)), status))
            })
            .await?;
        let Some((client_label, credential_label, permit)) = interaction else {
            return Ok(Response::Approval(status));
        };
        let broker = Arc::clone(self);
        self.background_jobs.spawn(async move {
            let decision = broker.human.confirm(&format!("Allow client {client_label} to discover the label and field names of {credential_label} ({reference})? This is metadata only, not credential values or permission to perform a future effect."), broker.shutdown.clone()).await;
            let _ = broker
                .transaction(move |b, state| {
                    // Release admission before readers can observe the decided
                    // job, including refusal and persistence-error early returns.
                    let _permit = permit;
                    if b.ready(state).is_err() {
                        if let Some(job) = state.jobs.get_mut(&id) {
                            job.decision = Decision::Denied;
                        }
                        return Ok(());
                    }
                    let decision = match decision {
                        Ok(true) => Decision::Allowed,
                        Err(ErrorCode::Expired) => Decision::Expired,
                        _ => Decision::Denied,
                    };
                    let valid = b.peer(state, &auth).is_ok() && b.metadata(&reference).is_some();
                    let Some(job) = state.jobs.get(&id) else {
                        return Ok(());
                    };
                    if job.owner != auth.id || job.reference != reference {
                        return Err(ErrorCode::Conflict);
                    }
                    if job.decision != Decision::Pending {
                        return Ok(());
                    }
                    let decision = if !valid {
                        Decision::Denied
                    } else if job.expires <= Instant::now() {
                        Decision::Expired
                    } else {
                        decision
                    };
                    if decision == Decision::Allowed {
                        if let Some(peer) =
                            state.registry.peers.iter_mut().find(|p| p.id == auth.id)
                        {
                            peer.references.insert(reference);
                        }
                        if b.save(state).is_err() {
                            if let Some(job) = state.jobs.get_mut(&id) {
                                job.decision = Decision::Uncertain;
                            }
                            return Err(ErrorCode::PersistenceUncertain);
                        }
                    }
                    if b.audit(state, "standalone_metadata_access_decided", id)
                        .is_err()
                    {
                        if let Some(job) = state.jobs.get_mut(&id) {
                            job.decision = Decision::Uncertain;
                        }
                        return Err(ErrorCode::PersistenceUncertain);
                    }
                    if let Some(job) = state.jobs.get_mut(&id) {
                        job.decision = decision;
                    }
                    Ok(())
                })
                .await;
        });
        Ok(Response::Approval(status))
    }

    async fn revoke(
        self: &Arc<Self>,
        auth: Auth,
        id: Uuid,
        params: RevokeRequest,
    ) -> Result<Response, ErrorCode> {
        let _permit = Arc::clone(&self.human_gate)
            .try_acquire_owned()
            .map_err(|_| ErrorCode::Busy)?;
        let auth2 = auth.clone();
        let client_id = params.client_id;
        let (requester, target) = self
            .transaction(move |b, s| {
                let requester = b.peer(s, &auth2)?.label.clone();
                let target = s
                    .registry
                    .peers
                    .iter()
                    .find(|p| p.id == client_id)
                    .ok_or(ErrorCode::NotFound)?
                    .label
                    .clone();
                Ok((requester, target))
            })
            .await?;
        if !self.human.confirm(&format!("Client {requester} requests revoking {target} ({client_id}). Future access through that pairing will be refused. Stored credentials are not deleted. Allow?"), self.shutdown.clone()).await? { return Err(ErrorCode::Denied); }
        self.transaction(move |b, s| {
            b.peer(s, &auth)?;
            b.audit(s, "standalone_revocation_requested", id)?;
            s.registry.peers.retain(|p| p.id != client_id);
            s.registry.consents.retain(|g| g.owner != client_id);
            s.consent_epochs.remove(&client_id);
            s.registry
                .browser_permissions
                .retain(|p| p.client_id != client_id);
            s.browsers.revoke(client_id);
            s.registry
                .native_grants
                .retain(|p| p.client_id != client_id);
            s.registry
                .delivery_profiles
                .retain(|p| p.owner != client_id);
            s.deliveries.revoke(client_id);
            for job in s.jobs.values_mut().filter(|j| j.owner == client_id) {
                job.decision = Decision::Denied;
            }
            b.save(s)?;
            b.audit(s, "standalone_revoked", id)?;
            Ok(Response::Revoked)
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enrollment_write_boundary_rejects_an_expired_deadline() {
        assert_eq!(
            ensure_enrollment_live(Instant::now()),
            Err(ErrorCode::Expired)
        );
        assert_eq!(
            ensure_enrollment_live(Instant::now() + Duration::from_secs(CONSENT_TTL_SECS)),
            Ok(())
        );
    }

    #[test]
    fn configured_registry_capacity_fits_the_persistence_bound() {
        let references = (0..MAX_CREDENTIALS)
            .map(|i| format!("cred_{}", Uuid::from_u128(i as u128)))
            .collect::<BTreeSet<_>>();
        let registry = Registry {
            version: 1,
            browser_permissions: vec![],
            native_grants: vec![],
            delivery_profiles: vec![],
            consents: vec![],
            peers: (0..MAX_CLIENTS)
                .map(|i| Peer {
                    id: Uuid::from_u128(i as u128),
                    label: "x".repeat(80),
                    token_hash: [255; 32],
                    references: references.clone(),
                })
                .collect(),
        };
        // Conservative upper bounds include escaped origin strings (canonical
        // origins cannot contain raw quotes/backslashes), all field names and
        // each row's IDs, digest and serialization overhead. Keep new profile
        // capacity from making an otherwise valid maximum registry unwritable.
        let browser_bytes =
            64 * (MAX_ORIGINS * (256 + 3) + MAX_FIELDS * (64 + 3) + 512) + 128 * 512;
        let delivery_bytes = MAX_DELIVERY_PROFILES * (MAX_PROFILE_BYTES + 1024);
        assert!(
            (serde_json::to_vec(&registry).unwrap().len()
                + browser_bytes
                + delivery_bytes
                + 16 * (12 * 1024 + 128)) as u64
                <= storage::MAX_STATE_BYTES
        );
    }

    #[test]
    fn consent_expiry_and_retention_use_monotonic_deadlines() {
        let now = Instant::now();
        let id = Uuid::new_v4();
        let mut state = State {
            registry: Registry {
                version: 1,
                peers: vec![],
                browser_permissions: vec![],
                native_grants: vec![],
                delivery_profiles: vec![],
                consents: vec![],
            },
            jobs: HashMap::new(),
            browsers: BrowserState::default(),
            deliveries: DeliveryJobs::default(),
            consent_epochs: HashMap::new(),
            faulted: false,
        };
        state.jobs.insert(
            id,
            Job {
                owner: Uuid::new_v4(),
                reference: format!("cred_{}", Uuid::new_v4()),
                decision: Decision::Pending,
                expires: now,
                retain_until: now + Duration::from_secs(1),
            },
        );
        state.reap(now);
        assert_eq!(state.jobs[&id].decision, Decision::Expired);
        state.reap(now + Duration::from_secs(1));
        assert!(!state.jobs.contains_key(&id));
    }
}
