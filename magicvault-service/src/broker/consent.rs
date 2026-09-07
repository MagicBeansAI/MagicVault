//! Human-created, exact-use grants. No RPC accepts an allow decision or scope.
use super::*;
use crate::human::UseDecision;

const MAX_GRANTS: usize = 16;
const MAX_GRANT_BYTES: usize = 12 * 1024;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ConsentGrant {
    pub owner: Uuid,
    pub info: ConsentGrantInfo,
}

#[derive(Clone)]
pub(super) struct ConsentTicket {
    pub scope: ConsentScope,
    label: String,
    existing: Option<Uuid>,
    epoch: Option<Uuid>,
}

pub(super) fn session_scope(scope: &ConsentScope) -> Option<Uuid> {
    match scope {
        ConsentScope::Browser {
            browser: ConsentBrowser::Session { browser_handle },
            ..
        } => Some(*browser_handle),
        _ => None,
    }
}

fn valid_scope(registry: &Registry, owner: Uuid, scope: &ConsentScope) -> bool {
    match scope {
        ConsentScope::Delivery { profile_id } => {
            !profile_id.is_nil()
                && registry
                    .delivery_profiles
                    .iter()
                    .any(|p| p.owner == owner && p.info.profile_id == *profile_id)
        }
        ConsentScope::Browser {
            browser,
            top_origin,
            frame_origin,
            is_main_frame,
            fields,
        } => {
            let identity = match browser {
                ConsentBrowser::Session { browser_handle } => !browser_handle.is_nil(),
                ConsentBrowser::Extension {
                    extension_id,
                    profile_id,
                } => registry.native_grants.iter().any(|g| {
                    g.client_id == owner
                        && g.allowed
                        && g.extension_id == *extension_id
                        && g.profile_id == *profile_id
                }),
            };
            identity
                && [top_origin, frame_origin].iter().all(|o| {
                    o.len() <= 256
                        && magicvault_effect::canonical_origin(o).as_deref() == Ok(o.as_str())
                })
                && (!is_main_frame || top_origin == frame_origin)
                && !fields.is_empty()
                && fields.len() <= MAX_FIELDS
                && fields.iter().all(|f| {
                    valid_css(&f.css)
                        && valid_reference(&f.credential_ref)
                        && valid_name(&f.credential_field)
                        && registry.browser_permissions.iter().any(|p| {
                            p.client_id == owner
                                && p.rule.credential_ref == f.credential_ref
                                && p.rule.field_names.contains(&f.credential_field)
                                && p.rule.origins.contains(top_origin)
                                && p.rule.origins.contains(frame_origin)
                        })
                })
                && fields.iter().map(|f| &f.css).collect::<BTreeSet<_>>().len() == fields.len()
        }
    }
}

pub(super) fn valid_grants(registry: &Registry) -> bool {
    registry.consents.len() <= MAX_GRANTS
        && registry.consents.iter().enumerate().all(|(index, g)| {
            !g.info.grant_id.is_nil()
                && valid_label(&g.info.label)
                && registry.peers.iter().any(|p| p.id == g.owner)
                && valid_scope(registry, g.owner, &g.info.scope)
                && serde_json::to_vec(&g.info).is_ok_and(|v| v.len() <= MAX_GRANT_BYTES)
                && registry.consents[..index].iter().all(|p| {
                    p.info.grant_id != g.info.grant_id
                        && (p.owner != g.owner || p.info.scope != g.info.scope)
                })
        })
}

pub(super) fn ticket(
    state: &State,
    owner: Uuid,
    label: String,
    scope: ConsentScope,
) -> ConsentTicket {
    ConsentTicket {
        existing: state
            .registry
            .consents
            .iter()
            .find(|g| g.owner == owner && g.info.scope == scope)
            .map(|g| g.info.grant_id),
        epoch: state.consent_epochs.get(&owner).copied(),
        label,
        scope,
    }
}

impl ConsentTicket {
    pub async fn decide(
        &self,
        human: &dyn HumanInteraction,
        message: &str,
        cancel: CancellationToken,
    ) -> Result<UseDecision, ErrorCode> {
        if cancel.is_cancelled() {
            return Err(ErrorCode::Cancelled);
        }
        if self.existing.is_some() {
            Ok(UseDecision::AllowOnce)
        } else {
            human.confirm_use(message, cancel).await
        }
    }
}

impl Broker {
    /// Called inside the final authorization transaction, before material leaves
    /// custody. Revoke/reset during a prompt cannot be undone by a late answer.
    pub(super) fn authorize_use(
        &self,
        state: &mut State,
        auth: &Auth,
        ticket: &ConsentTicket,
        decision: UseDecision,
        operation: Uuid,
    ) -> Result<(), ErrorCode> {
        self.peer(state, auth)?;
        if decision == UseDecision::Deny {
            return Err(ErrorCode::Denied);
        }
        if let Some(id) = ticket.existing {
            if !state.registry.consents.iter().any(|g| {
                g.owner == auth.id && g.info.grant_id == id && g.info.scope == ticket.scope
            }) {
                return Err(ErrorCode::Denied);
            }
            return self.audit(state, "standalone_remembered_consent_used", operation);
        }
        if state.consent_epochs.get(&auth.id).copied() != ticket.epoch {
            return Err(ErrorCode::Denied);
        }
        if decision == UseDecision::AlwaysAllow {
            if state.registry.consents.len() >= MAX_GRANTS {
                return Err(ErrorCode::Capacity);
            }
            if state
                .registry
                .consents
                .iter()
                .any(|g| g.owner == auth.id && g.info.scope == ticket.scope)
            {
                return Err(ErrorCode::Conflict);
            }
            if !valid_scope(&state.registry, auth.id, &ticket.scope) {
                return Err(ErrorCode::Denied);
            }
            let info = ConsentGrantInfo {
                grant_id: Uuid::new_v4(),
                label: ticket.label.clone(),
                scope: ticket.scope.clone(),
            };
            if serde_json::to_vec(&info)
                .map_err(|_| ErrorCode::Unavailable)?
                .len()
                > MAX_GRANT_BYTES
            {
                return Err(ErrorCode::Capacity);
            }
            self.audit(
                state,
                "standalone_remembered_consent_requested",
                info.grant_id,
            )?;
            let id = info.grant_id;
            state.registry.consents.push(ConsentGrant {
                owner: auth.id,
                info,
            });
            self.save(state)?;
            self.audit(state, "standalone_remembered_consent_saved", id)?;
        }
        Ok(())
    }

    pub(super) async fn list_consents(self: &Arc<Self>, auth: Auth) -> Result<Response, ErrorCode> {
        self.transaction(move |b, s| {
            b.peer(s, &auth)?;
            let mut grants: Vec<_> = s
                .registry
                .consents
                .iter()
                .filter(|g| g.owner == auth.id)
                .map(|g| g.info.clone())
                .collect();
            grants.sort_by_key(|g| g.grant_id);
            Ok(Response::Consents(grants))
        })
        .await
    }

    pub(super) async fn revoke_consent(
        self: &Arc<Self>,
        auth: Auth,
        id: Uuid,
        query: Option<ConsentQuery>,
    ) -> Result<Response, ErrorCode> {
        // Revoking authority needs no human approval and must work while a
        // decision is pending. Only this paired client's grants are visible.
        self.transaction(move |b, s| {
            b.peer(s, &auth)?;
            let scope = match query {
                Some(query) => Some(
                    s.registry
                        .consents
                        .iter()
                        .find(|g| g.owner == auth.id && g.info.grant_id == query.grant_id)
                        .ok_or(ErrorCode::NotFound)?
                        .info
                        .scope
                        .clone(),
                ),
                None => None,
            };
            b.audit(s, "standalone_remembered_consent_revoking", id)?;
            s.consent_epochs.insert(auth.id, Uuid::new_v4());
            s.registry.consents.retain(|g| {
                g.owner != auth.id || scope.as_ref().is_some_and(|scope| *scope != g.info.scope)
            });
            s.browsers.cancel_consents(auth.id, scope.as_ref());
            s.deliveries.cancel_consents(auth.id, scope.as_ref());
            b.save(s)?;
            b.audit(s, "standalone_remembered_consent_revoked", id)?;
            Ok(Response::ConsentRevoked)
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maximum_remembered_projection_fits_the_agent_reply_bound() {
        // Even maximally escaped valid CSS fits each 12 KiB grant, and all
        // admitted grants fit one value-free list reply without truncation.
        let field = FillField {
            css: "\\".repeat(512),
            credential_ref: format!("cred_{}", Uuid::new_v4()),
            credential_field: "f".repeat(64),
        };
        let info = ConsentGrantInfo {
            grant_id: Uuid::new_v4(),
            label: "l".repeat(80),
            scope: ConsentScope::Browser {
                browser: ConsentBrowser::Extension {
                    extension_id: "a".repeat(32),
                    profile_id: Uuid::new_v4(),
                },
                top_origin: "o".repeat(256),
                frame_origin: "f".repeat(256),
                is_main_frame: false,
                fields: vec![field; MAX_FIELDS],
            },
        };
        assert!(serde_json::to_vec(&info).unwrap().len() <= MAX_GRANT_BYTES);
        let reply = Reply::Ok(Response::Consents(vec![info; MAX_GRANTS]));
        assert!(serde_json::to_vec(&reply).unwrap().len() < MAX_REPLY_BYTES);
        assert!(MAX_GRANTS * (MAX_GRANT_BYTES + 128) < MAX_REPLY_BYTES);
    }
}
