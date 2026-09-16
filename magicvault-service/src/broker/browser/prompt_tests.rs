use super::*;

impl Fixture {
    async fn prompt_request(&self) -> SecurePromptFill {
        let r = self.request().await;
        SecurePromptFill {
            operation_id: r.operation_id,
            browser_handle: r.browser_handle,
            target_handle: r.target_handle,
            fields: vec![
                PromptFillField {
                    css: "#username".into(),
                    field_name: "username".into(),
                },
                PromptFillField {
                    css: "#password".into(),
                    field_name: "password".into(),
                },
            ],
        }
    }
    async fn start_prompt(&self, request: SecurePromptFill) -> Result<Response, ErrorCode> {
        invoke(
            &self.broker,
            Some(&self.token),
            Request::SecurePromptFill(request),
        )
        .await
    }
    async fn input_waiting(&self) {
        tokio::time::timeout(Duration::from_secs(2), self.human.once.started.notified())
            .await
            .unwrap();
    }
    fn assert_no_saved_inputs(&self) {
        assert!(self.broker.store.list_available().is_empty());
        let s = self.broker.state.lock().unwrap();
        assert!(s.registry.browser_permissions.is_empty());
        assert!(s.registry.consents.is_empty());
        assert!(s.registry.peers.iter().all(|p| p.references.is_empty()));
        fn inspect(path: &std::path::Path) {
            for entry in fs::read_dir(path).unwrap() {
                let entry = entry.unwrap();
                if entry.file_type().unwrap().is_dir() {
                    inspect(&entry.path());
                } else {
                    let data = fs::read(entry.path()).unwrap();
                    assert!(!data.windows(CANARY.len()).any(|w| w == CANARY.as_bytes()));
                }
            }
        }
        inspect(self.root.path());
    }
}

#[tokio::test]
async fn empty_vault_prompt_fill_is_once_receipt_only_and_never_enrolled() {
    let f = Fixture::with_enrollment(false).await;
    // Even if the saved-use provider would remember permission, JIT never asks it.
    f.human.remember.store(true, Ordering::SeqCst);
    let request = f.prompt_request().await;
    let response = f.start_prompt(request.clone()).await.unwrap();
    assert!(!serde_json::to_string(&response).unwrap().contains(CANARY));
    assert_eq!(
        f.settled(request.operation_id).await.state,
        FillState::Filled
    );
    assert_eq!(f.adapter.calls.load(Ordering::SeqCst), 1);
    assert_eq!(f.human.once.inputs.load(Ordering::SeqCst), 2);
    assert_eq!(f.human.once.confirmations.load(Ordering::SeqCst), 1);
    assert_eq!(f.human.prompts.load(Ordering::SeqCst), 0);
    f.start_prompt(request.clone()).await.unwrap();
    assert_eq!(f.human.once.inputs.load(Ordering::SeqCst), 2);
    let mut changed = request.clone();
    changed.fields[0].css = "#other".into();
    assert!(matches!(
        f.start_prompt(changed).await,
        Err(ErrorCode::Conflict)
    ));
    let mut replay = request.clone();
    replay.operation_id = Uuid::new_v4();
    assert!(matches!(
        f.start_prompt(replay).await,
        Err(ErrorCode::StaleTarget)
    ));
    // Both APIs share one operation namespace; an ID cannot switch modes.
    let saved = SecureFill {
        operation_id: request.operation_id,
        browser_handle: request.browser_handle,
        target_handle: request.target_handle,
        fields: vec![FillField {
            css: "#password".into(),
            credential_ref: format!("cred_{}", Uuid::new_v4()),
            credential_field: "password".into(),
        }],
    };
    assert!(matches!(
        invoke(&f.broker, Some(&f.token), Request::SecureFill(saved)).await,
        Err(ErrorCode::Conflict)
    ));
    // Result eviction cannot resurrect a spent operation.
    f.broker
        .state
        .lock()
        .unwrap()
        .browsers
        .fills
        .get_mut(&request.operation_id)
        .unwrap()
        .retain_until = Instant::now();
    assert!(matches!(
        f.start_prompt(request).await,
        Err(ErrorCode::Conflict)
    ));
    f.assert_no_saved_inputs();
    let audit = fs::read_to_string(
        f.root
            .path()
            .join("vault")
            .join(magicvault_core::store::SECRET_AUDIT_FILENAME),
    )
    .unwrap();
    assert!(audit.contains("secure_prompt_fill"));
    assert!(!audit.contains("#password"));
    assert!(f.broker.human_gate.try_acquire().is_ok());
    f.broker.quiesce().await;
}

#[tokio::test]
async fn denying_final_use_or_invalid_input_never_delivers_or_saves() {
    for value in [None, Some(""), Some("line\nbreak"), Some("line\rbreak")] {
        let f = Fixture::with_enrollment(false).await;
        *f.human.once.value.lock().unwrap() = value.map(str::to_owned);
        f.human.once.deny.store(true, Ordering::SeqCst);
        let r = f.prompt_request().await;
        f.start_prompt(r.clone()).await.unwrap();
        let status = f.settled(r.operation_id).await;
        assert_eq!(
            status.error,
            Some(if value.is_none() {
                ErrorCode::Denied
            } else {
                ErrorCode::InvalidRequest
            })
        );
        assert_eq!(f.adapter.calls.load(Ordering::SeqCst), 0);
        f.assert_no_saved_inputs();
        f.broker.quiesce().await;
    }
}

#[tokio::test]
async fn cancellation_during_second_input_or_final_confirmation_discards_all_inputs() {
    for confirm in [false, true] {
        let f = Fixture::with_enrollment(false).await;
        f.human
            .once
            .block_at
            .store(if confirm { 0 } else { 2 }, Ordering::SeqCst);
        f.human.once.block_confirm.store(confirm, Ordering::SeqCst);
        let r = f.prompt_request().await;
        f.start_prompt(r.clone()).await.unwrap();
        f.input_waiting().await;
        let next = f.prompt_request().await;
        assert!(matches!(
            f.start_prompt(next.clone()).await,
            Err(ErrorCode::Busy)
        ));
        invoke(
            &f.broker,
            Some(&f.token),
            Request::CancelFill(FillQuery {
                operation_id: r.operation_id,
            }),
        )
        .await
        .unwrap();
        assert_eq!(f.settled(r.operation_id).await.state, FillState::Cancelled);
        assert_eq!(f.adapter.calls.load(Ordering::SeqCst), 0);
        f.assert_no_saved_inputs();
        assert!(f.broker.human_gate.try_acquire().is_ok());
        // Busy did not spend the second target/ID. A new human action is required.
        f.human.once.block_at.store(0, Ordering::SeqCst);
        f.human.once.block_confirm.store(false, Ordering::SeqCst);
        f.start_prompt(next.clone()).await.unwrap();
        assert_eq!(f.settled(next.operation_id).await.state, FillState::Filled);
        f.broker.quiesce().await;
    }
}

#[tokio::test]
async fn expiry_during_input_cancels_native_wait_without_delivery() {
    let f = Fixture::with_enrollment(false).await;
    let r = f.prompt_request().await;
    f.broker
        .state
        .lock()
        .unwrap()
        .browsers
        .targets
        .get_mut(&r.target_handle)
        .unwrap()
        .expires = Instant::now() + Duration::from_millis(100);
    f.human.once.block_at.store(2, Ordering::SeqCst);
    f.start_prompt(r.clone()).await.unwrap();
    assert_eq!(f.settled(r.operation_id).await.state, FillState::Expired);
    assert_eq!(f.adapter.calls.load(Ordering::SeqCst), 0);
    f.assert_no_saved_inputs();
    f.broker.quiesce().await;
}

#[tokio::test]
async fn disconnect_clear_consents_and_shutdown_cancel_one_time_input() {
    for action in 0..3 {
        let f = Fixture::with_enrollment(false).await;
        f.human.once.block_at.store(2, Ordering::SeqCst);
        let r = f.prompt_request().await;
        f.start_prompt(r.clone()).await.unwrap();
        f.input_waiting().await;
        // Use trusted state changes to exercise revocation while native UI owns admission.
        match action {
            0 => {
                f.broker.state.lock().unwrap().browsers.revoke(f.auth.id);
            }
            1 => {
                f.broker
                    .state
                    .lock()
                    .unwrap()
                    .browsers
                    .cancel_consents(f.auth.id, None);
            }
            _ => f.broker.shutdown.cancel(),
        }
        assert_eq!(f.settled(r.operation_id).await.state, FillState::Cancelled);
        assert_eq!(f.adapter.calls.load(Ordering::SeqCst), 0);
        f.assert_no_saved_inputs();
        f.broker.quiesce().await;
    }
}

#[tokio::test]
async fn other_clients_and_expired_or_wrong_browser_targets_cannot_prompt() {
    let f = Fixture::with_enrollment(false).await;
    let Response::Paired(other) = invoke(
        &f.broker,
        None,
        Request::Pair(PairRequest {
            label: "other".into(),
        }),
    )
    .await
    .unwrap() else {
        panic!("pair");
    };
    let r = f.prompt_request().await;
    assert!(matches!(
        invoke(
            &f.broker,
            Some(&other.token),
            Request::SecurePromptFill(r.clone())
        )
        .await,
        Err(ErrorCode::StaleTarget)
    ));
    let mut wrong = r.clone();
    wrong.browser_handle = Uuid::new_v4();
    assert!(matches!(
        f.start_prompt(wrong).await,
        Err(ErrorCode::StaleTarget)
    ));
    f.broker
        .state
        .lock()
        .unwrap()
        .browsers
        .targets
        .get_mut(&r.target_handle)
        .unwrap()
        .expires = Instant::now();
    assert!(matches!(
        f.start_prompt(r).await,
        Err(ErrorCode::StaleTarget)
    ));
    assert_eq!(f.human.once.inputs.load(Ordering::SeqCst), 0);
    f.broker.quiesce().await;
}

#[tokio::test]
async fn partial_uncertain_and_failed_audit_never_report_success_or_replay() {
    for mode in 0..3 {
        let f = Fixture::with_enrollment(false).await;
        match mode {
            0 => {
                *f.adapter.outcome.lock().unwrap() = Some(Outcome {
                    fields: vec![FieldState::Filled, FieldState::NotFilled],
                    error: Some(ErrorCode::StaleTarget),
                })
            }
            1 => *f.adapter.outcome.lock().unwrap() = Some(Outcome::uncertain(2)),
            _ => {
                *f.adapter.fail_audit.lock().unwrap() = Some(
                    f.root
                        .path()
                        .join("vault")
                        .join(magicvault_core::store::SECRET_AUDIT_FILENAME),
                )
            }
        }
        let r = f.prompt_request().await;
        f.start_prompt(r.clone()).await.unwrap();
        let status = f.settled(r.operation_id).await;
        assert_eq!(
            status.state,
            if mode == 0 {
                FillState::Partial
            } else {
                FillState::Uncertain
            }
        );
        let retry = f.start_prompt(r).await;
        if mode == 2 {
            assert!(matches!(retry, Err(ErrorCode::PersistenceUncertain)));
        } else {
            retry.unwrap();
        }
        assert_eq!(f.adapter.calls.load(Ordering::SeqCst), 1);
        f.assert_no_saved_inputs();
        assert!(f.broker.human_gate.try_acquire().is_ok());
        f.broker.quiesce().await;
    }
}

#[test]
fn maximal_one_time_prompt_stays_within_native_display_bound() {
    let request = SecurePromptFill {
        operation_id: Uuid::new_v4(),
        browser_handle: Uuid::new_v4(),
        target_handle: Uuid::new_v4(),
        fields: (0..MAX_FIELDS)
            .map(|i| PromptFillField {
                css: format!("{}{i}", "\\".repeat(511)),
                field_name: format!("{}{i}", "n".repeat(63)),
            })
            .collect(),
    };
    assert!(request.valid());
    let target = Target {
        tab: "t".into(),
        frame: "f".into(),
        document: "d".into(),
        top_document: "d".into(),
        origin: "x".repeat(256),
        top_origin: "y".repeat(256),
        is_main_frame: true,
    };
    let context =
        super::super::prompt::prompt_context(&"l".repeat(80), &"b".repeat(80), &request, &target);
    assert!(context.len() + 1500 < crate::human::MAX_PROMPT_BYTES);
}
