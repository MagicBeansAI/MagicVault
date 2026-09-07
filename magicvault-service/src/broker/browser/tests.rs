use super::*;
#[path = "native_tests.rs"]
mod native_tests;
use async_trait::async_trait;
use magicvault_core::{encryption::SecretEncryptionError, MasterKeyProvider};
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
};

const CANARY: &str = "SYNTHETIC-BROKER-BROWSER-CANARY";

#[tokio::test]
async fn discovery_filters_are_not_permissions_and_rediscovery_invalidates_handles() {
    let fixture = Fixture::new().await;
    let original = fixture.request().await;
    let Response::BrowserTargets(rows) = fixture
        .broker
        .browser_targets(
            fixture.auth.clone(),
            BrowserTargetsQuery {
                browser_handle: fixture.browser,
                top_origin: Some("https://missing.example".into()),
                tab_id: None,
            },
        )
        .await
        .unwrap()
    else {
        panic!("targets");
    };
    assert!(rows.is_empty());
    assert!(!fixture
        .broker
        .state
        .lock()
        .unwrap()
        .browsers
        .targets
        .contains_key(&original.target_handle));
    let result = fixture
        .broker
        .browser_targets(
            fixture.auth.clone(),
            BrowserTargetsQuery {
                browser_handle: fixture.browser,
                top_origin: Some("https://example.com/path".into()),
                tab_id: None,
            },
        )
        .await;
    assert!(matches!(result, Err(ErrorCode::InvalidRequest)));
    assert_eq!(fixture.adapter.calls.load(Ordering::SeqCst), 0);
}

// Fixed test-only key permits real store/registry reloads without OS keychain.
struct FixtureKey;
impl MasterKeyProvider for FixtureKey {
    fn get_or_create_key(&self) -> Result<[u8; 32], SecretEncryptionError> {
        Ok([47; 32])
    }
    fn delete_key(&self) -> Result<(), SecretEncryptionError> {
        Ok(())
    }
    fn provider_name(&self) -> &str {
        "synthetic-browser-fixture"
    }
}

#[test]
fn browser_consent_fits_native_limit_without_truncating_valid_requests() {
    let reference = format!("cred_{}", Uuid::new_v4());
    let request = SecureFill {
        operation_id: Uuid::new_v4(),
        browser_handle: Uuid::new_v4(),
        target_handle: Uuid::new_v4(),
        fields: (0..MAX_FIELDS)
            .map(|i| FillField {
                css: format!("{}{i}", "\\".repeat(511)),
                credential_ref: reference.clone(),
                credential_field: "f".repeat(64),
            })
            .collect(),
    };
    assert!(request.valid());
    let target = Target {
        tab: "tab".into(),
        frame: "frame".into(),
        document: "document".into(),
        top_document: "document".into(),
        origin: "o".repeat(256),
        top_origin: "t".repeat(256),
        is_main_frame: true,
    };
    let prompt = fill_prompt(&"l".repeat(80), &request, &target);
    assert!(prompt.contains(&format!("Browser handle: {}", request.browser_handle)));
    assert!(prompt.len() > 4096);
    assert!(prompt.len() <= crate::human::MAX_PROMPT_BYTES);
    let rule = BrowserRule {
        credential_ref: reference,
        origins: (0..MAX_ORIGINS)
            .map(|i| format!("{i:02}{}", "o".repeat(254)))
            .collect(),
        field_names: (0..MAX_FIELDS)
            .map(|i| format!("{i}{}", "f".repeat(63)))
            .collect(),
    };
    assert!(rule.valid());
    let prompt = rule_prompt(&"l".repeat(80), &"c".repeat(80), &rule);
    assert!(prompt.len() > 4096);
    assert!(prompt.len() <= crate::human::MAX_PROMPT_BYTES);
}

struct Human {
    deny_native: AtomicBool,
    native_prompts: AtomicUsize,
    deny_fill: AtomicBool,
    block_fill: AtomicBool,
    prompts: AtomicUsize,
}
#[async_trait]
impl HumanInteraction for Human {
    async fn confirm(&self, message: &str, cancel: CancellationToken) -> Result<bool, ErrorCode> {
        assert!(!message.contains(CANARY));
        if message.starts_with("Allow automatic browser") {
            self.native_prompts.fetch_add(1, Ordering::SeqCst);
            return Ok(!self.deny_native.load(Ordering::SeqCst));
        }
        if message.starts_with("Allow ONE") {
            self.prompts.fetch_add(1, Ordering::SeqCst);
            if self.block_fill.load(Ordering::SeqCst) {
                cancel.cancelled().await;
                return Err(ErrorCode::Cancelled);
            }
            return Ok(!self.deny_fill.load(Ordering::SeqCst));
        }
        Ok(true)
    }
    async fn secret(&self, _: &str, _: CancellationToken) -> Result<Zeroizing<String>, ErrorCode> {
        Ok(Zeroizing::new(CANARY.into()))
    }
}
struct Adapter {
    target: Target,
    calls: AtomicUsize,
    alive: AtomicBool,
    outcome: Mutex<Option<Outcome>>,
    fail_audit: Mutex<Option<std::path::PathBuf>>,
}
#[async_trait]
impl BrowserAdapter for Adapter {
    async fn targets(&self, _: CancellationToken) -> Result<Vec<Target>, ErrorCode> {
        Ok(vec![self.target.clone()])
    }
    async fn fill(
        &self,
        target: &Target,
        fields: Vec<MaterialField>,
        cancel: CancellationToken,
    ) -> Outcome {
        if cancel.is_cancelled() {
            return Outcome::failed(fields.len(), ErrorCode::Cancelled);
        }
        assert!(target == &self.target);
        assert!(fields.iter().all(|f| f.value == CANARY));
        self.calls.fetch_add(1, Ordering::SeqCst);
        if let Some(journal) = self.fail_audit.lock().unwrap().take() {
            // Fault injection, not atomic publication: preserve the synthetic
            // journal, then obstruct its original path with a directory so the
            // post-effect durable audit fails. This test-only rename is an
            // explicitly reviewed source-ratchet exception; no live store is used.
            fs::rename(&journal, journal.with_extension("fixture-backup")).unwrap();
            fs::create_dir(&journal).unwrap();
        }
        self.outcome.lock().unwrap().clone().unwrap_or(Outcome {
            fields: vec![FieldState::Filled; fields.len()],
            error: None,
        })
    }
    fn disconnect(&self) {
        self.alive.store(false, Ordering::SeqCst);
    }
    fn connected(&self) -> bool {
        self.alive.load(Ordering::SeqCst)
    }
}
struct Fixture {
    root: tempfile::TempDir,
    broker: Arc<Broker>,
    human: Arc<Human>,
    adapter: Arc<Adapter>,
    auth: Auth,
    token: String,
    reference: String,
    browser: Uuid,
}

async fn invoke(
    broker: &Arc<Broker>,
    token: Option<&str>,
    request: Request,
) -> Result<Response, ErrorCode> {
    match broker
        .execute(Envelope {
            version: VERSION,
            request_id: Uuid::new_v4(),
            epoch: Some(broker.epoch),
            token: token.map(str::to_owned),
            request,
        })
        .await
    {
        Reply::Ok(response) => Ok(response),
        Reply::Error(error) => Err(error),
    }
}
impl Fixture {
    async fn new() -> Self {
        let root = tempfile::Builder::new()
            .permissions(fs::Permissions::from_mode(0o700))
            .tempdir()
            .unwrap();
        let instance = storage::Instance {
            format_version: 1,
            id: Uuid::new_v4(),
        };
        fs::write(
            root.path().join("instance.json"),
            serde_json::to_vec(&instance).unwrap(),
        )
        .unwrap();
        fs::set_permissions(
            root.path().join("instance.json"),
            fs::Permissions::from_mode(0o600),
        )
        .unwrap();
        let lease = storage::open(root.path()).unwrap();
        let store = SecretStore::new_empty(Box::new(FixtureKey), root.path().join("vault"));
        let human = Arc::new(Human {
            deny_native: AtomicBool::new(false),
            native_prompts: AtomicUsize::new(0),
            deny_fill: AtomicBool::new(false),
            block_fill: AtomicBool::new(false),
            prompts: AtomicUsize::new(0),
        });
        let broker = Broker::with_components(lease, store, human.clone()).unwrap();
        let Response::Paired(pair) = invoke(
            &broker,
            None,
            Request::Pair(PairRequest {
                label: "fixture".into(),
            }),
        )
        .await
        .unwrap() else {
            panic!("pair");
        };
        let auth = Auth {
            id: pair.client_id,
            digest: token_hash(&pair.token),
        };
        let token = pair.token.clone();
        let Response::Enrolled(metadata) = invoke(
            &broker,
            Some(&token),
            Request::Enroll(EnrollRequest {
                label: "fixture".into(),
                field_names: vec!["username".into(), "password".into()],
            }),
        )
        .await
        .unwrap() else {
            panic!("enroll");
        };
        let adapter = Arc::new(Adapter {
            target: Target {
                tab: "tab".into(),
                frame: "frame".into(),
                document: "doc".into(),
                top_document: "doc".into(),
                origin: "https://example.com".into(),
                top_origin: "https://example.com".into(),
                is_main_frame: true,
            },
            calls: AtomicUsize::new(0),
            alive: AtomicBool::new(true),
            outcome: Mutex::new(None),
            fail_audit: Mutex::new(None),
        });
        let browser = Uuid::new_v4();
        broker
            .register_adapter(
                auth.clone(),
                Uuid::new_v4(),
                BrowserInfo {
                    browser_handle: browser,
                    label: "fixture".into(),
                    backend: BrowserBackend::Cdp,
                },
                adapter.clone(),
                None,
            )
            .await
            .unwrap();
        Self {
            root,
            broker,
            human,
            adapter,
            auth,
            token,
            reference: metadata.credential_ref,
            browser,
        }
    }
    async fn configure(&self, origins: Vec<String>) {
        self.broker
            .configure_browser_credential(
                self.auth.clone(),
                Uuid::new_v4(),
                BrowserRule {
                    credential_ref: self.reference.clone(),
                    origins,
                    field_names: vec!["username".into(), "password".into()],
                },
            )
            .await
            .unwrap();
    }
    async fn request(&self) -> SecureFill {
        let Response::BrowserTargets(targets) = self
            .broker
            .browser_targets(
                self.auth.clone(),
                BrowserQuery {
                    browser_handle: self.browser,
                }
                .into(),
            )
            .await
            .unwrap()
        else {
            panic!("targets");
        };
        SecureFill {
            operation_id: Uuid::new_v4(),
            browser_handle: self.browser,
            target_handle: targets[0].target_handle,
            fields: vec![FillField {
                css: "#password".into(),
                credential_ref: self.reference.clone(),
                credential_field: "password".into(),
            }],
        }
    }
    async fn settled(&self, id: Uuid) -> FillStatus {
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                let Response::Fill(status) = invoke(
                    &self.broker,
                    Some(&self.token),
                    Request::FillStatus(FillQuery { operation_id: id }),
                )
                .await
                .unwrap() else {
                    panic!("fill status");
                };
                if !matches!(status.state, FillState::Pending | FillState::Filling) {
                    return status;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap()
    }
}

#[tokio::test]
async fn metadata_permission_never_implicitly_authorizes_a_fill() {
    let f = Fixture::new().await;
    let request = f.request().await;
    assert!(matches!(
        invoke(&f.broker, Some(&f.token), Request::SecureFill(request)).await,
        Err(ErrorCode::Denied)
    ));
    assert_eq!(f.adapter.calls.load(Ordering::SeqCst), 0);
    assert_eq!(f.human.prompts.load(Ordering::SeqCst), 0);
    f.broker.quiesce().await;
}

#[tokio::test]
async fn approved_fill_is_end_to_end_once_and_audits_only_typed_status() {
    let f = Fixture::new().await;
    f.configure(vec!["https://example.com".into()]).await;
    let request = f.request().await;
    invoke(
        &f.broker,
        Some(&f.token),
        Request::SecureFill(request.clone()),
    )
    .await
    .unwrap();
    let status = f.settled(request.operation_id).await;
    assert_eq!(status.state, FillState::Filled);
    invoke(
        &f.broker,
        Some(&f.token),
        Request::SecureFill(request.clone()),
    )
    .await
    .unwrap();
    assert_eq!(f.adapter.calls.load(Ordering::SeqCst), 1);
    assert_eq!(f.human.prompts.load(Ordering::SeqCst), 1);
    let mut changed = request.clone();
    changed.fields[0].css = "#another".into();
    assert!(matches!(
        invoke(&f.broker, Some(&f.token), Request::SecureFill(changed)).await,
        Err(ErrorCode::Conflict)
    ));
    let mut duplicate = request;
    duplicate.operation_id = Uuid::new_v4();
    assert!(matches!(
        invoke(&f.broker, Some(&f.token), Request::SecureFill(duplicate)).await,
        Err(ErrorCode::StaleTarget)
    ));
    let audit = fs::read_to_string(
        f.root
            .path()
            .join("vault")
            .join(magicvault_core::store::SECRET_AUDIT_FILENAME),
    )
    .unwrap();
    assert!(!audit.contains(CANARY));
    assert!(!audit.contains("#password"));
    assert!(audit.contains("standalone_fill_completed"));
    assert!(audit.contains("\"state\":\"filled\""));
    f.broker.quiesce().await;
}

#[tokio::test]
async fn denied_and_cancelled_human_decisions_never_deliver() {
    for cancel in [false, true] {
        let f = Fixture::new().await;
        f.configure(vec!["https://example.com".into()]).await;
        f.human.deny_fill.store(!cancel, Ordering::SeqCst);
        f.human.block_fill.store(cancel, Ordering::SeqCst);
        let request = f.request().await;
        invoke(
            &f.broker,
            Some(&f.token),
            Request::SecureFill(request.clone()),
        )
        .await
        .unwrap();
        if cancel {
            invoke(
                &f.broker,
                Some(&f.token),
                Request::CancelFill(FillQuery {
                    operation_id: request.operation_id,
                }),
            )
            .await
            .unwrap();
        }
        let status = f.settled(request.operation_id).await;
        assert_eq!(
            status.state,
            if cancel {
                FillState::Cancelled
            } else {
                FillState::Denied
            }
        );
        assert_eq!(f.adapter.calls.load(Ordering::SeqCst), 0);
        f.broker.quiesce().await;
    }
}

#[tokio::test]
async fn wrong_origin_expired_target_and_other_client_fail_before_consent() {
    let f = Fixture::new().await;
    f.configure(vec!["https://other.example".into()]).await;
    let request = f.request().await;
    assert!(matches!(
        invoke(
            &f.broker,
            Some(&f.token),
            Request::SecureFill(request.clone())
        )
        .await,
        Err(ErrorCode::Denied)
    ));
    f.configure(vec!["https://example.com".into()]).await;
    f.broker
        .state
        .lock()
        .unwrap()
        .browsers
        .targets
        .get_mut(&request.target_handle)
        .unwrap()
        .expires = Instant::now();
    assert!(matches!(
        invoke(&f.broker, Some(&f.token), Request::SecureFill(request)).await,
        Err(ErrorCode::StaleTarget)
    ));
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
    let request = f.request().await;
    assert!(matches!(
        invoke(&f.broker, Some(&other.token), Request::SecureFill(request)).await,
        Err(ErrorCode::StaleTarget)
    ));
    assert_eq!(f.human.prompts.load(Ordering::SeqCst), 0);
    f.broker.quiesce().await;
}

#[tokio::test]
async fn partial_and_uncertain_outcomes_are_not_success_and_are_not_retried() {
    for uncertain in [false, true] {
        let f = Fixture::new().await;
        f.configure(vec!["https://example.com".into()]).await;
        let mut request = f.request().await;
        request.fields.push(FillField {
            css: "#username".into(),
            credential_ref: f.reference.clone(),
            credential_field: "username".into(),
        });
        *f.adapter.outcome.lock().unwrap() = Some(if uncertain {
            Outcome::uncertain(2)
        } else {
            Outcome {
                fields: vec![FieldState::Filled, FieldState::NotFilled],
                error: Some(ErrorCode::StaleTarget),
            }
        });
        invoke(
            &f.broker,
            Some(&f.token),
            Request::SecureFill(request.clone()),
        )
        .await
        .unwrap();
        let status = f.settled(request.operation_id).await;
        assert_eq!(
            status.state,
            if uncertain {
                FillState::Uncertain
            } else {
                FillState::Partial
            }
        );
        invoke(&f.broker, Some(&f.token), Request::SecureFill(request))
            .await
            .unwrap();
        assert_eq!(f.adapter.calls.load(Ordering::SeqCst), 1);
        f.broker.quiesce().await;
    }
}

#[tokio::test]
async fn retained_tombstone_prevents_reopening_an_expired_result() {
    let f = Fixture::new().await;
    f.configure(vec!["https://example.com".into()]).await;
    let request = f.request().await;
    invoke(
        &f.broker,
        Some(&f.token),
        Request::SecureFill(request.clone()),
    )
    .await
    .unwrap();
    f.settled(request.operation_id).await;
    f.broker
        .state
        .lock()
        .unwrap()
        .browsers
        .fills
        .get_mut(&request.operation_id)
        .unwrap()
        .retain_until = Instant::now();
    let mut replacement = f.request().await;
    replacement.operation_id = request.operation_id;
    assert!(matches!(
        invoke(&f.broker, Some(&f.token), Request::SecureFill(replacement)).await,
        Err(ErrorCode::Conflict)
    ));
    assert_eq!(f.adapter.calls.load(Ordering::SeqCst), 1);
    f.broker.quiesce().await;
}

#[tokio::test]
async fn disconnect_and_shutdown_cancel_pending_jobs_and_preserve_browser_ownership() {
    let f = Fixture::new().await;
    f.configure(vec!["https://example.com".into()]).await;
    f.human.block_fill.store(true, Ordering::SeqCst);
    let request = f.request().await;
    invoke(
        &f.broker,
        Some(&f.token),
        Request::SecureFill(request.clone()),
    )
    .await
    .unwrap();
    invoke(
        &f.broker,
        Some(&f.token),
        Request::DisconnectBrowser(BrowserQuery {
            browser_handle: f.browser,
        }),
    )
    .await
    .unwrap();
    let status = f.settled(request.operation_id).await;
    assert!(matches!(
        status.state,
        FillState::Cancelled | FillState::Denied
    ));
    assert!(!f.adapter.connected());
    assert_eq!(f.adapter.calls.load(Ordering::SeqCst), 0);
    f.broker.quiesce().await;
}

#[tokio::test]
async fn post_effect_audit_failure_is_queryable_uncertainty_and_blocks_new_work() {
    let f = Fixture::new().await;
    f.configure(vec!["https://example.com".into()]).await;
    *f.adapter.fail_audit.lock().unwrap() = Some(
        f.root
            .path()
            .join("vault")
            .join(magicvault_core::store::SECRET_AUDIT_FILENAME),
    );
    let request = f.request().await;
    invoke(
        &f.broker,
        Some(&f.token),
        Request::SecureFill(request.clone()),
    )
    .await
    .unwrap();
    let status = f.settled(request.operation_id).await;
    assert_eq!(status.state, FillState::Uncertain);
    assert_eq!(status.error, Some(ErrorCode::PersistenceUncertain));
    assert_eq!(status.fields, [FieldState::Filled]);
    assert!(matches!(
        invoke(&f.broker, Some(&f.token), Request::SecureFill(request)).await,
        Err(ErrorCode::PersistenceUncertain)
    ));
    assert_eq!(f.adapter.calls.load(Ordering::SeqCst), 1);
    f.broker.quiesce().await;
}
