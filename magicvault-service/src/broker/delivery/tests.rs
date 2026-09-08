use super::*;
use async_trait::async_trait;
use magicvault_core::{
    encryption::{MasterKeyProvider, SecretEncryptionError},
    InMemoryKeyProvider,
};
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    sync::atomic::{AtomicU8, AtomicUsize, Ordering},
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const CANARY: &str = "SYNTHETIC-DELIVERY-SERVICE-CANARY";
#[path = "consent_tests.rs"]
mod consent_tests;
#[path = "completion_tests.rs"]
mod completion_tests;

#[tokio::test(flavor = "multi_thread")]
async fn client_revocation_removes_persisted_destination_authority() {
    let f = Fixture::new().await;
    let _profile = f.register(f.process()).await;
    let Response::Status(owner) = response(&f.broker, Some(&f.token), Request::Status).await else {
        panic!("status")
    };
    let Response::Paired(admin) = response(
        &f.broker,
        None,
        Request::Pair(PairRequest {
            label: "Revocation fixture".into(),
        }),
    )
    .await
    else {
        panic!("pair")
    };
    assert!(matches!(
        response(
            &f.broker,
            Some(&admin.token),
            Request::RevokeClient(RevokeRequest {
                client_id: owner.client_id.unwrap(),
            })
        )
        .await,
        Response::Revoked
    ));
    assert!(matches!(
        request_raw(&f.broker, Some(&f.token), Request::ListDeliveryProfiles).await,
        Reply::Error(ErrorCode::Unauthorized)
    ));
    f.broker
        .transaction(|_, s| {
            assert!(s.registry.delivery_profiles.is_empty());
            Ok(())
        })
        .await
        .unwrap();
    assert!(!f.root.path().join("runs").exists());
    f.broker.quiesce().await;
}
struct Human {
    denied: tokio::sync::Notify,
    mode: AtomicU8,
    uses: AtomicUsize,
}
#[async_trait]
impl HumanInteraction for Human {
    async fn confirm_use(
        &self,
        message: &str,
        cancel: CancellationToken,
    ) -> Result<crate::human::UseDecision, ErrorCode> {
        use crate::human::UseDecision;
        self.uses.fetch_add(1, Ordering::SeqCst);
        let mode = self.mode.load(Ordering::SeqCst);
        if mode == 4 {
            cancel.cancelled().await;
            return Ok(UseDecision::AlwaysAllow); // Malicious late provider answer.
        }
        if matches!(mode, 5 | 6) {
            cancel.cancelled().await;
            // Native dialog teardown reports Denied. A provider's explicit
            // deadline result must instead retain Expired when cancellation races.
            return Err(if mode == 5 {
                ErrorCode::Denied
            } else {
                ErrorCode::Expired
            });
        }
        if mode == 7 {
            self.denied.notify_one();
            return Err(ErrorCode::Denied); // Native human Deny, without cancellation.
        }
        self.confirm(message, cancel).await.map(|yes| {
            if !yes {
                UseDecision::Deny
            } else if mode == 3 {
                UseDecision::AlwaysAllow
            } else {
                UseDecision::AllowOnce
            }
        })
    }
    async fn confirm(&self, message: &str, cancel: CancellationToken) -> Result<bool, ErrorCode> {
        assert!(!message.contains(CANARY));
        if message.contains("requests to RUN this destination ONCE.") {
            match self.mode.load(Ordering::SeqCst) {
                1 => return Ok(false),
                2 => {
                    cancel.cancelled().await;
                    return Err(ErrorCode::Cancelled);
                }
                _ => {}
            }
        }
        Ok(true)
    }
    async fn secret(&self, _: &str, _: CancellationToken) -> Result<Zeroizing<String>, ErrorCode> {
        Ok(Zeroizing::new(CANARY.into()))
    }
}
struct Key(Arc<InMemoryKeyProvider>);
impl MasterKeyProvider for Key {
    fn get_or_create_key(&self) -> Result<[u8; 32], SecretEncryptionError> {
        self.0.get_or_create_key()
    }
    fn delete_key(&self) -> Result<(), SecretEncryptionError> {
        self.0.delete_key()
    }
    fn provider_name(&self) -> &str {
        "synthetic-delivery"
    }
}
struct Fixture {
    root: tempfile::TempDir,
    broker: Arc<Broker>,
    human: Arc<Human>,
    token: String,
    reference: String,
    key: Arc<InMemoryKeyProvider>,
}
async fn response(broker: &Arc<Broker>, token: Option<&str>, request: Request) -> Response {
    match request_raw(broker, token, request).await {
        Reply::Ok(r) => r,
        Reply::Error(e) => panic!("closed fixture error: {e:?}"),
    }
}
async fn request_raw(broker: &Arc<Broker>, token: Option<&str>, request: Request) -> Reply {
    broker
        .execute(Envelope {
            version: VERSION,
            request_id: Uuid::new_v4(),
            epoch: Some(broker.epoch),
            token: token.map(str::to_owned),
            request,
        })
        .await
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
        let key = Arc::new(InMemoryKeyProvider::new());
        let human = Arc::new(Human {
            denied: tokio::sync::Notify::new(),
            mode: AtomicU8::new(0),
            uses: AtomicUsize::new(0),
        });
        let store =
            SecretStore::new(Box::new(Key(key.clone())), root.path().join("vault")).unwrap();
        let broker =
            Broker::with_components(storage::open(root.path()).unwrap(), store, human.clone())
                .unwrap();
        let Response::Paired(pair) = response(
            &broker,
            None,
            Request::Pair(PairRequest {
                label: "Delivery fixture".into(),
            }),
        )
        .await
        else {
            panic!("pair")
        };
        let token = pair.token.clone();
        let Response::Enrolled(metadata) = response(
            &broker,
            Some(&token),
            Request::Enroll(EnrollRequest {
                label: "Synthetic token".into(),
                field_names: vec!["token".into()],
            }),
        )
        .await
        else {
            panic!("enroll")
        };
        Self {
            root,
            broker,
            human,
            token,
            reference: metadata.credential_ref,
            key,
        }
    }
    fn value(&self) -> InputValue {
        InputValue::Credential {
            credential_ref: self.reference.clone(),
            credential_field: "token".into(),
            prefix: String::new(),
            suffix: String::new(),
        }
    }
    fn process(&self) -> DeliveryProfile {
        let executable = self.root.path().join("recipient");
        // Script content contains no credential. Success proves nonempty input;
        // exact delivery is separately checked by the effect transport tests.
        fs::write(&executable,"#!/bin/sh\n[ -n \"$MV_TOKEN\" ] || exit 1\nprintf 'run\\n' >> runs\nprintf '%s' \"$MV_TOKEN\"\n").unwrap();
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
        DeliveryProfile {
            label: "Process fixture".into(),
            destination: DeliveryDestination::Process(ProcessDestination {
                executable: executable.to_str().unwrap().into(),
                arguments: vec![],
                working_directory: self.root.path().to_str().unwrap().into(),
                environment: vec![NamedValue {
                    name: "MV_TOKEN".into(),
                    value: self.value(),
                }],
                stdin: None,
                timeout_secs: 2,
            }),
        }
    }
    async fn register(&self, profile: DeliveryProfile) -> Uuid {
        let Response::DeliveryProfile(info) = response(
            &self.broker,
            Some(&self.token),
            Request::RegisterDeliveryProfile(profile),
        )
        .await
        else {
            panic!("profile")
        };
        info.profile_id
    }
    async fn run(&self, profile_id: Uuid, kind: DeliveryKind) -> SecureDelivery {
        let request = SecureDelivery {
            profile_id,
            operation_id: Uuid::new_v4(),
        };
        let request_wire = match kind {
            DeliveryKind::Process => Request::SecureNewProcess(request.clone()),
            DeliveryKind::Http => Request::SecureNewHttp(request.clone()),
        };
        assert!(matches!(
            response(&self.broker, Some(&self.token), request_wire).await,
            Response::Delivery(_)
        ));
        request
    }
    async fn status(&self, id: Uuid) -> DeliveryStatus {
        let Response::Delivery(status) = response(
            &self.broker,
            Some(&self.token),
            Request::DeliveryStatus(FillQuery { operation_id: id }),
        )
        .await
        else {
            panic!("status")
        };
        status
    }
    async fn settled(&self, id: Uuid) -> DeliveryStatus {
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let status = self.status(id).await;
                if !matches!(
                    status.state,
                    DeliveryState::Pending | DeliveryState::Running
                ) {
                    return status;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap()
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn registered_process_executes_once_and_audits_only_closed_receipts() {
    let f = Fixture::new().await;
    let profile = f.register(f.process()).await;
    let request = f.run(profile, DeliveryKind::Process).await;
    let status = f.settled(request.operation_id).await;
    assert_eq!(status.state, DeliveryState::Completed);
    assert!(status.may_have_run);
    let Response::Delivery(replay) = response(
        &f.broker,
        Some(&f.token),
        Request::SecureNewProcess(request.clone()),
    )
    .await
    else {
        panic!("replay")
    };
    assert_eq!(status, replay);
    assert_eq!(
        fs::read_to_string(f.root.path().join("runs")).unwrap(),
        "run\n"
    );
    assert!(matches!(
        request_raw(&f.broker, Some(&f.token), Request::SecureNewHttp(request)).await,
        Reply::Error(ErrorCode::Conflict)
    ));
    let audit = fs::read_to_string(
        f.root
            .path()
            .join("vault")
            .join(magicvault_core::store::SECRET_AUDIT_FILENAME),
    )
    .unwrap();
    assert!(audit.contains("standalone_delivery_completed"));
    assert!(!audit.contains(CANARY));
    f.broker.quiesce().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn denial_cancel_and_profile_removal_never_dispatch_pending_work() {
    let f = Fixture::new().await;
    let profile = f.register(f.process()).await;
    f.human.mode.store(1, Ordering::SeqCst);
    let request = f.run(profile, DeliveryKind::Process).await;
    let status = f.settled(request.operation_id).await;
    assert_eq!(status.state, DeliveryState::Denied);
    assert!(!status.may_have_run);
    f.human.mode.store(2, Ordering::SeqCst);
    let request = f.run(profile, DeliveryKind::Process).await;
    response(
        &f.broker,
        Some(&f.token),
        Request::CancelDelivery(FillQuery {
            operation_id: request.operation_id,
        }),
    )
    .await;
    assert_eq!(
        f.settled(request.operation_id).await.state,
        DeliveryState::Cancelled
    );
    let request = f.run(profile, DeliveryKind::Process).await;
    response(
        &f.broker,
        Some(&f.token),
        Request::RemoveDeliveryProfile(ProfileQuery {
            profile_id: profile,
        }),
    )
    .await;
    assert!(!f.settled(request.operation_id).await.may_have_run);
    assert!(!f.root.path().join("runs").exists());
    f.broker.quiesce().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn another_client_cannot_invoke_inspect_or_remove_a_profile() {
    let f = Fixture::new().await;
    let profile = f.register(f.process()).await;
    let Response::Paired(other) = response(
        &f.broker,
        None,
        Request::Pair(PairRequest {
            label: "Other client".into(),
        }),
    )
    .await
    else {
        panic!("pair")
    };
    let request = f.run(profile, DeliveryKind::Process).await;
    f.settled(request.operation_id).await;
    for request in [
        Request::SecureNewProcess(SecureDelivery {
            operation_id: Uuid::new_v4(),
            profile_id: profile,
        }),
        Request::DeliveryStatus(FillQuery {
            operation_id: request.operation_id,
        }),
        Request::RemoveDeliveryProfile(ProfileQuery {
            profile_id: profile,
        }),
    ] {
        assert!(matches!(
            request_raw(&f.broker, Some(&other.token), request).await,
            Reply::Error(ErrorCode::NotFound)
        ));
    }
    let Response::DeliveryProfiles(rows) =
        response(&f.broker, Some(&other.token), Request::ListDeliveryProfiles).await
    else {
        panic!("profiles")
    };
    assert!(rows.is_empty());
    let mut foreign = f.process();
    foreign.label = "Foreign attempt".into();
    assert!(matches!(
        request_raw(
            &f.broker,
            Some(&other.token),
            Request::RegisterDeliveryProfile(foreign)
        )
        .await,
        Reply::Error(ErrorCode::Denied)
    ));
    f.broker.quiesce().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn completion_audit_failure_preserves_uncertainty_and_read_only_status() {
    let f = Fixture::new().await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let profile = DeliveryProfile {
        label: "HTTP fixture".into(),
        destination: DeliveryDestination::Http(HttpDestination {
            url: format!("http://{}/", listener.local_addr().unwrap()),
            method: "POST".into(),
            headers: vec![NamedValue {
                name: "Authorization".into(),
                value: f.value(),
            }],
            query: vec![],
            body: None,
            timeout_secs: 2,
        }),
    };
    let id = f.register(profile).await;
    let audit = f
        .root
        .path()
        .join("vault")
        .join(magicvault_core::store::SECRET_AUDIT_FILENAME);
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut request = Vec::new();
        let mut buffer = [0u8; 4096];
        while !request.windows(4).any(|w| w == b"\r\n\r\n") {
            let n = stream.read(&mut buffer).await.unwrap();
            assert!(n > 0);
            request.extend_from_slice(&buffer[..n]);
            assert!(request.len() < 16384);
        }
        assert!(String::from_utf8_lossy(&request).contains(CANARY));
        // Test-only fault injection, not atomic publication: retain the
        // synthetic journal, then replace its pathname with a directory so the
        // production durable audit append fails after the recipient has acted.
        fs::rename(&audit, audit.with_extension("saved")).unwrap();
        fs::create_dir(&audit).unwrap();
        stream
            .write_all(b"HTTP/1.1 204 No Content\r\nConnection: close\r\n\r\n")
            .await
            .unwrap();
    });
    let request = f.run(id, DeliveryKind::Http).await;
    let status = f.settled(request.operation_id).await;
    server.await.unwrap();
    assert_eq!(status.state, DeliveryState::Uncertain);
    assert_eq!(status.error, Some(ErrorCode::PersistenceUncertain));
    assert!(status.may_have_run);
    assert!(matches!(
        request_raw(
            &f.broker,
            Some(&f.token),
            Request::SecureNewHttp(SecureDelivery {
                operation_id: Uuid::new_v4(),
                profile_id: id
            })
        )
        .await,
        Reply::Error(ErrorCode::PersistenceUncertain)
    ));
    assert_eq!(f.status(request.operation_id).await, status);
    f.broker.quiesce().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn profiles_survive_restart_but_old_jobs_do_not_become_retry_authority() {
    let f = Fixture::new().await;
    let profile = f.register(f.process()).await;
    let request = f.run(profile, DeliveryKind::Process).await;
    f.settled(request.operation_id).await;
    f.broker.quiesce().await;
    let Fixture {
        root,
        broker,
        human,
        token,
        key,
        ..
    } = f;
    let epoch = broker.epoch;
    drop(broker);
    let store = SecretStore::new(Box::new(Key(key)), root.path().join("vault")).unwrap();
    let restarted =
        Broker::with_components(storage::open(root.path()).unwrap(), store, human).unwrap();
    assert_ne!(restarted.epoch, epoch);
    let Response::DeliveryProfiles(rows) =
        response(&restarted, Some(&token), Request::ListDeliveryProfiles).await
    else {
        panic!("profiles")
    };
    assert_eq!(rows[0].profile_id, profile);
    assert!(matches!(
        request_raw(
            &restarted,
            Some(&token),
            Request::DeliveryStatus(FillQuery {
                operation_id: request.operation_id
            })
        )
        .await,
        Reply::Error(ErrorCode::NotFound)
    ));
    assert!(matches!(
        restarted
            .execute(Envelope {
                version: VERSION,
                request_id: Uuid::new_v4(),
                epoch: Some(epoch),
                token: Some(token),
                request: Request::SecureNewProcess(request)
            })
            .await,
        Reply::Error(ErrorCode::StaleSession)
    ));
    assert_eq!(
        fs::read_to_string(root.path().join("runs")).unwrap(),
        "run\n"
    );
    restarted.quiesce().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn expired_receipts_keep_operation_ids_spent_and_shutdown_cancels_pending_work() {
    let f = Fixture::new().await;
    let profile = f.register(f.process()).await;
    let request = f.run(profile, DeliveryKind::Process).await;
    f.settled(request.operation_id).await;
    f.broker
        .transaction(|_, s| {
            s.deliveries.reap(Instant::now() + Duration::from_secs(601));
            Ok(())
        })
        .await
        .unwrap();
    assert!(matches!(
        request_raw(
            &f.broker,
            Some(&f.token),
            Request::DeliveryStatus(FillQuery {
                operation_id: request.operation_id
            })
        )
        .await,
        Reply::Error(ErrorCode::NotFound)
    ));
    assert!(matches!(
        request_raw(
            &f.broker,
            Some(&f.token),
            Request::SecureNewProcess(request)
        )
        .await,
        Reply::Error(ErrorCode::Conflict)
    ));
    f.human.mode.store(2, Ordering::SeqCst);
    let pending = f.run(profile, DeliveryKind::Process).await;
    tokio::time::timeout(Duration::from_secs(3), f.broker.quiesce())
        .await
        .unwrap();
    let status = f.status(pending.operation_id).await;
    assert!(!status.may_have_run);
    assert_eq!(
        fs::read_to_string(f.root.path().join("runs")).unwrap(),
        "run\n"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn registration_cap_wrong_fields_and_changed_executable_fail_closed() {
    let f = Fixture::new().await;
    let original = f.process();
    let mut wrong = original.clone();
    let DeliveryDestination::Process(ref mut p) = wrong.destination else {
        unreachable!()
    };
    let InputValue::Credential {
        ref mut credential_field,
        ..
    } = p.environment[0].value
    else {
        unreachable!()
    };
    *credential_field = "missing".into();
    assert!(matches!(
        request_raw(
            &f.broker,
            Some(&f.token),
            Request::RegisterDeliveryProfile(wrong)
        )
        .await,
        Reply::Error(ErrorCode::Denied)
    ));
    let mut first = Uuid::nil();
    for index in 0..MAX_DELIVERY_PROFILES {
        let mut profile = original.clone();
        profile.label = format!("Destination {index}");
        let id = f.register(profile).await;
        if index == 0 {
            first = id;
        }
    }
    assert!(matches!(
        request_raw(
            &f.broker,
            Some(&f.token),
            Request::RegisterDeliveryProfile(original)
        )
        .await,
        Reply::Error(ErrorCode::Capacity)
    ));
    fs::write(
        f.root.path().join("recipient"),
        "#!/bin/sh\nprintf 'bad' > runs\n",
    )
    .unwrap();
    let request = f.run(first, DeliveryKind::Process).await;
    let status = f.settled(request.operation_id).await;
    assert!(!status.may_have_run);
    assert!(!f.root.path().join("runs").exists());
    f.broker.quiesce().await;
}
