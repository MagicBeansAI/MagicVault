//! One native prompt-and-fill operation. Values exist only in owned zeroizing
//! input/material buffers, never in the core store, registry, job or receipt.
use super::*;

pub(super) fn prompt_context(
    label: &str,
    browser: &str,
    request: &SecurePromptFill,
    target: &Target,
) -> String {
    let selections = request
        .fields
        .iter()
        .map(|f| format!("{:?} -> {:?}", f.field_name, f.css))
        .collect::<Vec<_>>()
        .join("; ");
    format!("One-time browser fill for client {label}. Browser: {browser} ({}). Page: {}. Selected frame: {}. Requested fields (untrusted names and selectors): {selections}. Values are held temporarily for this operation and are not saved in MagicVault. The website receives them; other browser tools may read them. No form submission or future-use permission is requested.", request.browser_handle, target.top_origin, target.origin)
}

// Native providers must finish cancellation/child cleanup before returning.
// Keep the human permit until this drain completes; never abandon an input UI.
async fn prompt_until<T>(
    work: impl std::future::Future<Output = Result<T, ErrorCode>>,
    deadline: Instant,
    cancel: CancellationToken,
) -> Result<T, ErrorCode> {
    if cancel.is_cancelled() {
        return Err(ErrorCode::Cancelled);
    }
    if Instant::now() >= deadline {
        return Err(ErrorCode::Expired);
    }
    tokio::pin!(work);
    let result = tokio::select! {
        result = &mut work => result,
        _ = cancel.cancelled() => {
            let _ = work.await;
            Err(ErrorCode::Cancelled)
        },
        _ = tokio::time::sleep(deadline.saturating_duration_since(Instant::now())) => {
            cancel.cancel();
            let _ = work.await;
            Err(ErrorCode::Expired)
        },
    };
    // Native cancellation may race the select and return a denial-shaped error.
    // Preserve our deadline classification; otherwise cancellation wins over
    // either a provider decision or an input produced during teardown.
    if matches!(result, Err(ErrorCode::Expired)) {
        return Err(ErrorCode::Expired);
    }
    if cancel.is_cancelled() {
        return Err(ErrorCode::Cancelled);
    }
    if Instant::now() >= deadline {
        return Err(ErrorCode::Expired);
    }
    result
}

impl Broker {
    pub(in crate::broker) async fn secure_prompt_fill(
        self: &Arc<Self>,
        auth: Auth,
        request: SecurePromptFill,
    ) -> Result<Response, ErrorCode> {
        let (auth2, req2) = (auth.clone(), request.clone());
        let (status, work) = self
            .transaction(move |b, s| {
                let label = b.peer(s, &auth2)?.label.clone();
                if let Some(job) = s.browsers.fills.get(&req2.operation_id) {
                    if job.owner != auth2.id || job.request != FillRequest::Prompt(req2.clone()) {
                        return Err(ErrorCode::Conflict);
                    }
                    return Ok((job.status.clone(), None));
                }
                if s.browsers.used_operations.contains(&req2.operation_id) {
                    return Err(ErrorCode::Conflict);
                }
                if s.browsers.used_operations.len() >= 4096
                    || s.browsers.fills.len() >= MAX_FILL_JOBS
                {
                    return Err(ErrorCode::Capacity);
                }
                let bound = s
                    .browsers
                    .targets
                    .get(&req2.target_handle)
                    .filter(|t| t.owner == auth2.id && t.browser == req2.browser_handle)
                    .cloned()
                    .ok_or(ErrorCode::StaleTarget)?;
                let browser = s
                    .browsers
                    .instances
                    .get(&req2.browser_handle)
                    .filter(|browser| browser.owner == auth2.id && browser.adapter.connected())
                    .ok_or(ErrorCode::StaleTarget)?;
                let context = prompt_context(&label, &browser.info.label, &req2, &bound.target);
                let adapter = Arc::clone(&browser.adapter);
                let permit = Arc::clone(&b.human_gate)
                    .try_acquire_owned()
                    .map_err(|_| ErrorCode::Busy)?;
                b.audit(s, "standalone_prompt_fill_requested", req2.operation_id)?;
                // The target/operation cannot be used by either fill path again.
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
                        request: FillRequest::Prompt(req2),
                        status: status.clone(),
                        cancel: cancel.clone(),
                        retain_until: Instant::now() + Duration::from_secs(600),
                        // No credential policy or remembered-use grant is acquired.
                        consent: None,
                    },
                );
                Ok((status, Some((context, bound, adapter, cancel, permit))))
            })
            .await?;
        if let Some((context, bound, adapter, cancel, permit)) = work {
            let broker = Arc::clone(self);
            self.background_jobs.spawn(async move {
                let count = request.fields.len();
                let material = broker
                    .collect_once(&request, &context, bound.expires, cancel.clone())
                    .await;
                let delivery = match material {
                    Ok(fields) => {
                        let (auth2, req2, cancel2) =
                            (auth.clone(), request.clone(), cancel.clone());
                        broker
                            .transaction(move |b, s| {
                                b.peer(s, &auth2)?;
                                if cancel2.is_cancelled() {
                                    return Err(ErrorCode::Cancelled);
                                }
                                if Instant::now() >= bound.expires {
                                    return Err(ErrorCode::Expired);
                                }
                                if !s.browsers.instances.get(&req2.browser_handle).is_some_and(
                                    |browser| {
                                        browser.owner == auth2.id && browser.adapter.connected()
                                    },
                                ) {
                                    return Err(ErrorCode::StaleTarget);
                                }
                                b.audit(s, "standalone_prompt_fill_authorized", req2.operation_id)?;
                                // Recheck after durable audit, immediately before releasing material.
                                if cancel2.is_cancelled() {
                                    return Err(ErrorCode::Cancelled);
                                }
                                if Instant::now() >= bound.expires {
                                    return Err(ErrorCode::Expired);
                                }
                                let job = s
                                    .browsers
                                    .fills
                                    .get_mut(&req2.operation_id)
                                    .ok_or(ErrorCode::Unavailable)?;
                                if job.owner != auth2.id || job.status.state != FillState::Pending {
                                    return Err(ErrorCode::Conflict);
                                }
                                job.status.state = FillState::Filling;
                                Ok(fields)
                            })
                            .await
                    }
                    Err(error) => Err(error),
                };
                let outcome = match delivery {
                    // Existing adapters revalidate top/frame document identities
                    // and preflight every locator before the first browser write.
                    Ok(fields) => adapter.fill(&bound.target, fields, cancel).await,
                    Err(error) => Outcome::failed(count, error),
                };
                broker
                    .complete_fill(auth, FillRequest::Prompt(request), outcome, permit)
                    .await;
            });
        }
        Ok(Response::Fill(status))
    }

    async fn collect_once(
        &self,
        request: &SecurePromptFill,
        context: &str,
        deadline: Instant,
        cancel: CancellationToken,
    ) -> Result<Vec<MaterialField>, ErrorCode> {
        let mut fields = Vec::with_capacity(request.fields.len());
        for (index, field) in request.fields.iter().enumerate() {
            let message = format!("{context}\nEnter field {} of {}: {:?} for selector {:?}. Continue collects the next field; a final Use once decision is required before any delivery. Enter the value only here, never in chat.", index + 1, request.fields.len(), field.field_name, field.css);
            let mut value = prompt_until(
                self.human.secret_once(&message, cancel.clone()),
                deadline,
                cancel.clone(),
            )
            .await?;
            if value.is_empty() || value.len() > 4096 || value.contains(['\r', '\n']) {
                return Err(ErrorCode::InvalidRequest);
            }
            fields.push(MaterialField {
                css: field.css.clone(),
                value: std::mem::take(&mut *value),
            });
        }
        let message = format!("{context}\nUse the entered values for this one fill? Cancel discards them without filling. Use once never saves them in MagicVault.");
        if !prompt_until(
            self.human.confirm_once(&message, cancel.clone()),
            deadline,
            cancel.clone(),
        )
        .await?
        {
            return Err(ErrorCode::Denied);
        }
        if cancel.is_cancelled() {
            return Err(ErrorCode::Cancelled);
        }
        if Instant::now() >= deadline {
            return Err(ErrorCode::Expired);
        }
        Ok(fields)
    }
}
