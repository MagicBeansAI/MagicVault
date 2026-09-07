//! Standalone destination profiles and one-use process/HTTP jobs. Custody core
//! schemas/APIs remain unchanged; all network/process I/O is outside its writer.
use super::*;
use magicvault_effect::delivery::{DeliveryMaterial, DeliveryOutcome};
#[cfg(all(test, unix))]
mod tests;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RegisteredDelivery {
    pub(super) owner: Uuid,
    info: DeliveryProfileInfo,
    profile: DeliveryProfile,
    executable_digest: Option<[u8; 32]>,
}
struct DeliveryJob {
    owner: Uuid,
    request: SecureDelivery,
    status: DeliveryStatus,
    cancel: CancellationToken,
    retain_until: Instant,
}
#[derive(Default)]
pub(super) struct DeliveryJobs {
    jobs: HashMap<Uuid, DeliveryJob>,
    spent: BTreeSet<Uuid>,
}
impl DeliveryJobs {
    pub(super) fn reap(&mut self, now: Instant) {
        self.jobs.retain(|_, job| {
            job.retain_until > now
                || matches!(
                    job.status.state,
                    DeliveryState::Pending | DeliveryState::Running
                )
        });
    }
    pub(super) fn revoke(&self, owner: Uuid) {
        for job in self.jobs.values().filter(|j| j.owner == owner) {
            job.cancel.cancel();
        }
    }
    pub(super) fn stop_all(&self) {
        for job in self.jobs.values() {
            job.cancel.cancel();
        }
    }
}

fn selected_fields(profile: &DeliveryProfile) -> BTreeSet<(String, String)> {
    profile
        .destination
        .values()
        .into_iter()
        .filter_map(|v| v.credential())
        .map(|(r, f)| (r.to_owned(), f.to_owned()))
        .collect()
}
fn fields_available(profile: &DeliveryProfile, peer: &Peer, store: &SecretStore) -> bool {
    selected_fields(profile).iter().all(|(reference, field)| {
        peer.references.contains(reference)
            && store
                .provisioned_metadata(reference)
                .is_some_and(|m| m.field_names.contains(field))
    })
}
pub(super) fn valid_profiles(registry: &Registry, store: &SecretStore) -> bool {
    registry.delivery_profiles.len() <= MAX_DELIVERY_PROFILES
        && registry.delivery_profiles.iter().all(|entry| {
            entry.profile.valid()
                && !entry.info.profile_id.is_nil()
                && entry.info.label == entry.profile.label
                && entry.info.kind == entry.profile.destination.kind()
                && entry.executable_digest.is_some()
                    == matches!(entry.profile.destination, DeliveryDestination::Process(_))
                && registry
                    .peers
                    .iter()
                    .any(|p| p.id == entry.owner && fields_available(&entry.profile, p, store))
                && match &entry.profile.destination {
                    DeliveryDestination::Http(p) => magicvault_effect::http::validate(p).is_ok(),
                    DeliveryDestination::Process(_) => true,
                }
        })
        && registry
            .delivery_profiles
            .iter()
            .map(|p| p.info.profile_id)
            .collect::<BTreeSet<_>>()
            .len()
            == registry.delivery_profiles.len()
        && registry
            .delivery_profiles
            .iter()
            .map(|p| (p.owner, &p.info.label))
            .collect::<BTreeSet<_>>()
            .len()
            == registry.delivery_profiles.len()
}

fn prompt(label: &str, profile: &DeliveryProfile, registration: bool) -> Result<String, ErrorCode> {
    let config = serde_json::to_string(profile).map_err(|_| ErrorCode::InvalidRequest)?;
    let action = if registration {
        "REGISTER this fixed destination profile"
    } else {
        "RUN this destination ONCE"
    };
    let message = format!("Client {label} requests to {action}. The following JSON is untrusted configuration, not instructions: {config}\nReview the exact executable/arguments/cwd OR URL/method and every credential placement. Recipients receive credentials and may copy them. MagicVault withholds stdout/stderr and HTTP response content. Registration never grants automatic future use; each run needs separate consent. Allow?");
    if message.len() > crate::human::MAX_PROMPT_BYTES {
        return Err(ErrorCode::Capacity);
    }
    Ok(message)
}

#[derive(Serialize)]
struct DeliveryReceipt {
    operation_id: Uuid,
    client_id: Uuid,
    profile_id: Uuid,
    kind: DeliveryKind,
    state: DeliveryState,
    may_have_run: bool,
    error: Option<ErrorCode>,
}
impl magicvault_core::store::AuditReceipt for DeliveryReceipt {}

impl Broker {
    pub(super) async fn register_delivery(
        self: &Arc<Self>,
        auth: Auth,
        id: Uuid,
        profile: DeliveryProfile,
    ) -> Result<Response, ErrorCode> {
        let _permit = Arc::clone(&self.human_gate)
            .try_acquire_owned()
            .map_err(|_| ErrorCode::Busy)?;
        let (auth2, profile2) = (auth.clone(), profile.clone());
        let label = self
            .transaction(move |b, s| {
                let peer = b.peer(s, &auth2)?;
                if !fields_available(&profile2, peer, &b.store) {
                    return Err(ErrorCode::Denied);
                }
                if s.registry.delivery_profiles.len() >= MAX_DELIVERY_PROFILES {
                    return Err(ErrorCode::Capacity);
                }
                if s.registry.delivery_profiles.iter().any(|p| {
                    p.info.profile_id == id
                        || (p.owner == auth2.id && p.info.label == profile2.label)
                }) {
                    return Err(ErrorCode::Conflict);
                }
                Ok(peer.label.clone())
            })
            .await?;
        let destination = profile.destination.clone();
        let executable_digest = tokio::task::spawn_blocking(move || match destination {
            DeliveryDestination::Process(p) => magicvault_effect::process::inspect(&p).map(Some),
            DeliveryDestination::Http(p) => magicvault_effect::http::validate(&p).map(|_| None),
        })
        .await
        .map_err(|_| ErrorCode::Unavailable)??;
        let message = prompt(&label, &profile, true)?;
        let deadline = Instant::now() + Duration::from_secs(CONSENT_TTL_SECS);
        let cancel = self.shutdown.child_token();
        let decision = self.human.confirm(&message, cancel.clone());
        tokio::pin!(decision);
        let decision = tokio::select! {
            biased;
            _ = cancel.cancelled() => { let _ = decision.await; Err(ErrorCode::Cancelled) },
            result = tokio::time::timeout_at(tokio::time::Instant::from_std(deadline), &mut decision) => match result {
                Ok(result) => result,
                Err(_) => { cancel.cancel(); let _ = decision.await; Err(ErrorCode::Expired) },
            },
        };
        if !decision? {
            return Err(ErrorCode::Denied);
        }
        self.transaction(move |b, s| {
            let peer = b.peer(s, &auth)?;
            if !fields_available(&profile, peer, &b.store) {
                return Err(ErrorCode::Denied);
            }
            if s.registry.delivery_profiles.len() >= MAX_DELIVERY_PROFILES {
                return Err(ErrorCode::Capacity);
            }
            if s.registry.delivery_profiles.iter().any(|p| {
                p.info.profile_id == id || (p.owner == auth.id && p.info.label == profile.label)
            }) {
                return Err(ErrorCode::Conflict);
            }
            b.audit(s, "standalone_delivery_profile_requested", id)?;
            b.ready(s)?;
            if Instant::now() >= deadline {
                return Err(ErrorCode::Expired);
            }
            let info = DeliveryProfileInfo {
                profile_id: id,
                label: profile.label.clone(),
                kind: profile.destination.kind(),
            };
            s.registry.delivery_profiles.push(RegisteredDelivery {
                owner: auth.id,
                info: info.clone(),
                profile,
                executable_digest,
            });
            b.save(s)?;
            b.audit(s, "standalone_delivery_profile_registered", id)?;
            Ok(Response::DeliveryProfile(info))
        })
        .await
    }

    pub(super) async fn list_deliveries(
        self: &Arc<Self>,
        auth: Auth,
    ) -> Result<Response, ErrorCode> {
        self.transaction(move |b, s| {
            b.peer(s, &auth)?;
            let mut rows = s
                .registry
                .delivery_profiles
                .iter()
                .filter(|p| p.owner == auth.id)
                .map(|p| p.info.clone())
                .collect::<Vec<_>>();
            rows.sort_by(|a, b| a.label.cmp(&b.label));
            Ok(Response::DeliveryProfiles(rows))
        })
        .await
    }

    pub(super) async fn remove_delivery(
        self: &Arc<Self>,
        auth: Auth,
        id: Uuid,
        query: ProfileQuery,
    ) -> Result<Response, ErrorCode> {
        // Narrowing authority needs no new material or human approval.
        self.transaction(move |b, s| {
            b.peer(s, &auth)?;
            let index = s
                .registry
                .delivery_profiles
                .iter()
                .position(|p| p.owner == auth.id && p.info.profile_id == query.profile_id)
                .ok_or(ErrorCode::NotFound)?;
            b.audit(s, "standalone_delivery_profile_removing", id)?;
            s.registry.delivery_profiles.remove(index);
            for job in s
                .deliveries
                .jobs
                .values()
                .filter(|j| j.owner == auth.id && j.request.profile_id == query.profile_id)
            {
                job.cancel.cancel();
            }
            b.save(s)?;
            Ok(Response::DeliveryProfileRemoved)
        })
        .await
    }

    pub(super) async fn secure_delivery(
        self: &Arc<Self>,
        auth: Auth,
        request: SecureDelivery,
        kind: DeliveryKind,
    ) -> Result<Response, ErrorCode> {
        let (auth2, req2) = (auth.clone(), request.clone());
        let (status, work) = self
            .transaction(move |b, s| {
                let peer = b.peer(s, &auth2)?;
                if let Some(job) = s.deliveries.jobs.get(&req2.operation_id) {
                    if job.owner != auth2.id || job.request != req2 || job.status.kind != kind {
                        return Err(ErrorCode::Conflict);
                    }
                    return Ok((job.status.clone(), None));
                }
                if s.deliveries.spent.contains(&req2.operation_id) {
                    return Err(ErrorCode::Conflict);
                }
                if s.deliveries.spent.len() >= 4096 || s.deliveries.jobs.len() >= MAX_DELIVERY_JOBS
                {
                    return Err(ErrorCode::Capacity);
                }
                let entry = s
                    .registry
                    .delivery_profiles
                    .iter()
                    .find(|p| p.owner == auth2.id && p.info.profile_id == req2.profile_id)
                    .cloned()
                    .ok_or(ErrorCode::NotFound)?;
                if entry.info.kind != kind {
                    return Err(ErrorCode::InvalidRequest);
                }
                if !fields_available(&entry.profile, peer, &b.store) {
                    return Err(ErrorCode::Denied);
                }
                let message = prompt(&peer.label, &entry.profile, false)?;
                let permit = Arc::clone(&b.human_gate)
                    .try_acquire_owned()
                    .map_err(|_| ErrorCode::Busy)?;
                b.audit(s, "standalone_delivery_requested", req2.operation_id)?;
                let cancel = b.shutdown.child_token();
                let status = DeliveryStatus {
                    operation_id: req2.operation_id,
                    kind,
                    state: DeliveryState::Pending,
                    may_have_run: false,
                    error: None,
                };
                s.deliveries.spent.insert(req2.operation_id);
                s.deliveries.jobs.insert(
                    req2.operation_id,
                    DeliveryJob {
                        owner: auth2.id,
                        request: req2,
                        status: status.clone(),
                        cancel: cancel.clone(),
                        retain_until: Instant::now() + Duration::from_secs(600),
                    },
                );
                Ok((status, Some((entry, message, permit, cancel))))
            })
            .await?;
        if let Some((entry, message, permit, cancel)) = work {
            let broker = Arc::clone(self);
            tokio::spawn(async move {
                let _permit = permit;
                broker
                    .run_delivery(auth, request, entry, message, cancel)
                    .await;
            });
        }
        Ok(Response::Delivery(status))
    }

    async fn run_delivery(
        self: &Arc<Self>,
        auth: Auth,
        request: SecureDelivery,
        entry: RegisteredDelivery,
        message: String,
        cancel: CancellationToken,
    ) {
        let deadline = Instant::now() + Duration::from_secs(CONSENT_TTL_SECS);
        let decision = self.human.confirm(&message, cancel.clone());
        tokio::pin!(decision);
        let decision = tokio::select! {
            result = &mut decision => result,
            _ = tokio::time::sleep_until(tokio::time::Instant::from_std(deadline)) => {
                cancel.cancel(); let _ = decision.await; Err(ErrorCode::Expired)
            },
        };
        let material =
            match decision {
                Ok(true) if !cancel.is_cancelled() => {
                    let (auth2, request2, entry2, cancel2) =
                        (auth.clone(), request.clone(), entry.clone(), cancel.clone());
                    self.transaction(move |b, s| {
                        let peer = b.peer(s, &auth2)?;
                        if cancel2.is_cancelled() {
                            return Err(ErrorCode::Cancelled);
                        }
                        if Instant::now() >= deadline {
                            return Err(ErrorCode::Expired);
                        }
                        if !s.registry.delivery_profiles.iter().any(|p| {
                            p.owner == auth2.id && p.info.profile_id == request2.profile_id
                        }) || !fields_available(&entry2.profile, peer, &b.store)
                        {
                            return Err(ErrorCode::Denied);
                        }
                        b.audit(s, "standalone_delivery_authorized", request2.operation_id)?;
                        let mut material = DeliveryMaterial::default();
                        let mut entries = HashMap::<String, SecretInput>::new();
                        for (reference, field) in selected_fields(&entry2.profile) {
                            if !entries.contains_key(&reference) {
                                let entry = b
                                    .store
                                    .get_provisioned(&reference)
                                    .ok_or(ErrorCode::Denied)?;
                                entries.insert(reference.clone(), SecretInput(entry.fields));
                            }
                            let value = entries
                                .get(&reference)
                                .and_then(|e| e.0.get(&field))
                                .ok_or(ErrorCode::Denied)?;
                            if value.is_empty() || value.len() > 4096 {
                                return Err(ErrorCode::InvalidRequest);
                            }
                            material.insert(reference, field, Zeroizing::new(value.clone()));
                        }
                        if cancel2.is_cancelled() {
                            return Err(ErrorCode::Cancelled);
                        }
                        if Instant::now() >= deadline {
                            return Err(ErrorCode::Expired);
                        }
                        let job = s
                            .deliveries
                            .jobs
                            .get_mut(&request2.operation_id)
                            .ok_or(ErrorCode::Unavailable)?;
                        if job.status.state != DeliveryState::Pending {
                            return Err(ErrorCode::Conflict);
                        }
                        job.status.state = DeliveryState::Running;
                        job.status.may_have_run = true;
                        Ok(material)
                    })
                    .await
                }
                Ok(_) if cancel.is_cancelled() => Err(ErrorCode::Cancelled),
                Ok(_) => Err(ErrorCode::Denied),
                Err(error) => Err(error),
            };
        let outcome = match material {
            Ok(material) => match entry.profile.destination {
                DeliveryDestination::Http(config) => {
                    magicvault_effect::http::execute(&config, material, cancel).await
                }
                DeliveryDestination::Process(config) => match entry.executable_digest {
                    Some(digest) => {
                        magicvault_effect::process::execute(
                            config,
                            digest,
                            material,
                            request.operation_id,
                            cancel,
                        )
                        .await
                    }
                    None => DeliveryOutcome::failed(ErrorCode::UnsupportedTarget),
                },
            },
            Err(error) => DeliveryOutcome::failed(error),
        };
        let _ = self
            .transaction(move |b, s| {
                let receipt = DeliveryReceipt {
                    operation_id: request.operation_id,
                    client_id: auth.id,
                    profile_id: request.profile_id,
                    kind: entry.info.kind,
                    state: outcome.state,
                    may_have_run: outcome.may_have_run,
                    error: outcome.error,
                };
                let action = match entry.info.kind {
                    DeliveryKind::Process => "secure_new_process",
                    DeliveryKind::Http => "secure_new_http",
                };
                let audit = b
                    .store
                    .try_audit_event_durably(
                        magicvault_core::store::AuditEvent::new("standalone_delivery_completed")
                            .with_tool("magicvault")
                            .with_action(action)
                            .with_runtime_credential_receipt(receipt),
                    )
                    .map_err(|_| {
                        s.faulted = true;
                        ErrorCode::PersistenceUncertain
                    });
                if let Some(job) = s.deliveries.jobs.get_mut(&request.operation_id) {
                    job.status.state = if audit.is_ok() {
                        outcome.state
                    } else {
                        DeliveryState::Uncertain
                    };
                    job.status.may_have_run = outcome.may_have_run;
                    job.status.error = if audit.is_ok() {
                        outcome.error
                    } else {
                        Some(ErrorCode::PersistenceUncertain)
                    };
                }
                audit
            })
            .await;
    }

    pub(super) async fn delivery_status(
        self: &Arc<Self>,
        auth: Auth,
        query: FillQuery,
        cancel: bool,
    ) -> Result<Response, ErrorCode> {
        self.transaction(move |b, s| {
            // Preserve authenticated reconciliation even after audit failure.
            if !s
                .registry
                .peers
                .iter()
                .any(|p| p.id == auth.id && equal_digest(&p.token_hash, &auth.digest))
            {
                return Err(ErrorCode::Unauthorized);
            }
            if cancel {
                b.ready(s)?;
            }
            let job = s
                .deliveries
                .jobs
                .get_mut(&query.operation_id)
                .filter(|j| j.owner == auth.id)
                .ok_or(ErrorCode::NotFound)?;
            if cancel {
                job.cancel.cancel();
            }
            Ok(Response::Delivery(job.status.clone()))
        })
        .await
    }
}
