#![cfg(unix)]
use std::{collections::VecDeque, fs, os::unix::fs::PermissionsExt, sync::{Arc, Mutex}, time::Duration};
use async_trait::async_trait;
use magicvault_core::{InMemoryKeyProvider, encryption::{MasterKeyProvider, SecretEncryptionError}, store::SecretStore};
use magicvault_service::{broker::Broker, client::Client, human::HumanInteraction, ipc, protocol::*, storage};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;
use zeroize::Zeroizing;

const CANARY: &str = "SYNTHETIC-P2-NOT-A-REAL-CREDENTIAL";

struct FixtureHuman { answers: Mutex<VecDeque<bool>> }
#[async_trait]
impl HumanInteraction for FixtureHuman {
    async fn confirm(&self, _: &str, _: CancellationToken) -> Result<bool, ErrorCode> {
        Ok(self.answers.lock().unwrap().pop_front().unwrap_or(false))
    }
    async fn secret(&self, _: &str, _: CancellationToken) -> Result<Zeroizing<String>, ErrorCode> {
        Ok(Zeroizing::new(CANARY.into()))
    }
}
struct SharedKey(Arc<InMemoryKeyProvider>);
impl MasterKeyProvider for SharedKey {
    fn get_or_create_key(&self) -> Result<[u8;32],SecretEncryptionError> { self.0.get_or_create_key() }
    fn delete_key(&self) -> Result<(),SecretEncryptionError> { self.0.delete_key() }
    fn provider_name(&self) -> &str { "fixture" }
}
fn fixture() -> (tempfile::TempDir, Arc<InMemoryKeyProvider>) {
    let root = tempfile::Builder::new().permissions(fs::Permissions::from_mode(0o700)).tempdir().unwrap();
    let instance = storage::Instance { format_version:1, id:Uuid::new_v4() };
    fs::write(root.path().join("instance.json"), serde_json::to_vec(&instance).unwrap()).unwrap();
    fs::set_permissions(root.path().join("instance.json"), fs::Permissions::from_mode(0o600)).unwrap();
    (root, Arc::new(InMemoryKeyProvider::new()))
}
fn broker(root: &std::path::Path, key: Arc<InMemoryKeyProvider>, answers: Vec<bool>) -> Arc<Broker> {
    let lease = storage::open(root).unwrap();
    let store = SecretStore::new(Box::new(SharedKey(key)), root.join("vault")).unwrap();
    Broker::with_components(lease, store, Arc::new(FixtureHuman { answers:Mutex::new(answers.into()) })).unwrap()
}
fn envelope(broker: &Broker, request: Request, token: Option<&str>, id: Uuid) -> Envelope {
    Envelope { version:VERSION, request_id:id, epoch:Some(broker.epoch), token:token.map(str::to_owned), request }
}
async fn call(broker: &Arc<Broker>, request: Request, token: Option<&str>) -> Response {
    match broker.execute(envelope(broker, request, token, Uuid::new_v4())).await {
        Reply::Ok(response) => response, Reply::Error(error) => panic!("fixture call: {error:?}"),
    }
}
async fn pair(broker: &Arc<Broker>, label: &str) -> Pairing {
    match call(broker, Request::Pair(PairRequest { label:label.into() }), None).await {
        Response::Paired(pairing) => pairing, _ => panic!("pair reply"),
    }
}
async fn enrolled(broker: &Arc<Broker>, token: &str) -> CredentialMetadata {
    match call(broker, Request::Enroll(EnrollRequest { label:"Fixture".into(), field_names:vec!["password".into()] }), Some(token)).await {
        Response::Enrolled(metadata) => metadata, _ => panic!("enroll reply"),
    }
}

#[tokio::test]
async fn core_enrollment_metadata_isolation_human_consent_and_restart_are_one_flow() {
    let (root, key) = fixture();
    let service = broker(root.path(), Arc::clone(&key), vec![true, true, true, true]);
    let owner = pair(&service,"owner").await;
    let agent = pair(&service,"agent").await;
    let metadata = enrolled(&service, &owner.token).await;
    assert!(!serde_json::to_string(&metadata).unwrap().contains(CANARY));
    let Response::Credentials(rows) = call(&service, Request::ListCredentials, Some(&agent.token)).await else { panic!("metadata"); };
    assert!(rows.is_empty());
    let Response::Approval(pending) = call(&service, Request::RequestAccess(AccessRequest { credential_ref:metadata.credential_ref.clone() }), Some(&agent.token)).await else { panic!("approval"); };
    assert_eq!(pending.operation,"metadata_connection");
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let Response::Approval(status) = call(&service, Request::ApprovalStatus(ApprovalQuery { approval_id:pending.approval_id }), Some(&agent.token)).await else { panic!("status"); };
            if status.decision != Decision::Pending { assert_eq!(status.decision,Decision::Allowed); break; }
            tokio::task::yield_now().await;
        }
    }).await.unwrap();
    let Response::Credentials(rows) = call(&service, Request::ListCredentials, Some(&agent.token)).await else { panic!("metadata"); };
    assert_eq!(rows,[metadata.clone()]);
    let epoch = service.epoch;
    service.quiesce().await;
    drop(service);
    let restarted = broker(root.path(), key, vec![true]);
    assert_ne!(restarted.epoch,epoch);
    let Response::Credentials(rows) = call(&restarted, Request::ListCredentials, Some(&agent.token)).await else { panic!("metadata"); };
    assert_eq!(rows,[metadata]);
    let mut stale = envelope(&restarted, Request::ListCredentials, Some(&agent.token),Uuid::new_v4());
    stale.epoch = Some(epoch);
    assert!(matches!(restarted.execute(stale).await,Reply::Error(ErrorCode::StaleSession)));
    call(&restarted,Request::RevokeClient(RevokeRequest {client_id:agent.client_id}),Some(&owner.token)).await;
    assert!(matches!(restarted.execute(envelope(&restarted,Request::ListCredentials,Some(&agent.token),Uuid::new_v4())).await,Reply::Error(ErrorCode::Unauthorized)));
}

#[tokio::test]
async fn denial_and_invalid_auth_never_release_metadata_or_enroll() {
    let (root,key) = fixture();
    let service = broker(root.path(),key,vec![true,false]);
    let peer = pair(&service,"agent").await;
    let request = Request::Enroll(EnrollRequest {label:"Fixture".into(),field_names:vec!["password".into()]});
    assert!(matches!(service.execute(envelope(&service,request,Some(&peer.token),Uuid::new_v4())).await,Reply::Error(ErrorCode::Denied)));
    assert!(matches!(service.execute(envelope(&service,Request::ListCredentials,Some(&"f".repeat(64)),Uuid::new_v4())).await,Reply::Error(ErrorCode::Unauthorized)));
    assert!(!root.path().join("vault/provisioned_secrets.vault").exists());
}

#[tokio::test]
async fn real_ipc_and_shared_client_pair_enroll_and_list_without_stdout_material() {
    let (root,key) = fixture();
    let service = broker(root.path(),key,vec![true,true]);
    let server = tokio::spawn(ipc::serve(Arc::clone(&service)));
    tokio::time::timeout(Duration::from_secs(2),async { while !service.socket().exists() { tokio::task::yield_now().await; } }).await.unwrap();
    Client::pair(root.path().to_owned(),"agent","Fixture client".into()).await.unwrap();
    let client = Client::load(root.path().to_owned(),"agent").unwrap();
    let response = client.call(Request::Enroll(EnrollRequest {label:"Fixture".into(),field_names:vec!["password".into()]})).await.unwrap();
    assert!(matches!(response,Response::Enrolled(_)));
    let response = client.call(Request::ListCredentials).await.unwrap();
    let wire = serde_json::to_string(&response).unwrap();
    assert!(wire.contains("password"));
    assert!(!wire.contains(CANARY));
    service.shutdown.cancel();
    server.await.unwrap().unwrap();
    assert!(!service.socket().exists());
}

#[test]
fn unsafe_roots_second_writers_and_unrecognized_state_fail_closed() {
    let (root,_) = fixture();
    let lease = storage::open(root.path()).unwrap();
    assert!(matches!(storage::open(root.path()),Err(ErrorCode::Busy)));
    drop(lease);
    fs::set_permissions(root.path(),fs::Permissions::from_mode(0o755)).unwrap();
    assert!(storage::open(root.path()).is_err());
    fs::set_permissions(root.path(),fs::Permissions::from_mode(0o700)).unwrap();
    fs::write(root.path().join("instance.json"),br#"{"format_version":999,"id":"00000000-0000-0000-0000-000000000000"}"#).unwrap();
    assert!(storage::open(root.path()).is_err());
}

#[test]
fn launch_agent_definition_escapes_paths_and_never_contains_credentials() {
    let text = magicvault_service::launch_agent::plist(std::path::Path::new("/tmp/a&b/magicvault"),std::path::Path::new("/tmp/vault<1>"),"ai.magicbeans.magicvault.fixture").unwrap();
    assert!(text.contains("a&amp;b"));
    assert!(text.contains("vault&lt;1&gt;"));
    assert!(text.contains("<key>KeepAlive</key><false/>"));
    assert!(!text.contains(CANARY));
}

#[tokio::test]
async fn persistence_failure_poisoning_prevents_optimistic_success_or_further_access() {
    let (root,key) = fixture();
    let service = broker(root.path(),key,vec![true,true]);
    let peer = pair(&service,"owner").await;
    // The root is a disposable fixture. Preserve the first registry as evidence;
    // make publication fail by replacing its destination with a directory.
    fs::rename(root.path().join("clients.json"),root.path().join("clients.previous.json")).unwrap();
    fs::create_dir(root.path().join("clients.json")).unwrap();
    let request = Request::Enroll(EnrollRequest {label:"Fixture".into(),field_names:vec!["password".into()]});
    assert!(matches!(service.execute(envelope(&service,request,Some(&peer.token),Uuid::new_v4())).await,Reply::Error(ErrorCode::PersistenceUncertain)));
    assert!(matches!(service.execute(envelope(&service,Request::ListCredentials,Some(&peer.token),Uuid::new_v4())).await,Reply::Error(ErrorCode::PersistenceUncertain)));
    let Response::Status(status) = call(&service,Request::Status,Some(&peer.token)).await else {panic!("status");};
    assert!(!status.ready);
    assert!(root.path().join("vault/provisioned_secrets.vault").exists());
}

#[tokio::test]
async fn reused_enrollment_request_id_cannot_overwrite_a_credential() {
    let (root,key) = fixture();
    let service = broker(root.path(),key,vec![true,true]);
    let peer = pair(&service,"owner").await;
    let request = Request::Enroll(EnrollRequest {label:"Fixture".into(),field_names:vec!["password".into()]});
    let id = Uuid::new_v4();
    assert!(matches!(service.execute(envelope(&service,request.clone(),Some(&peer.token),id)).await,Reply::Ok(Response::Enrolled(_))));
    assert!(matches!(service.execute(envelope(&service,request,Some(&peer.token),id)).await,Reply::Error(ErrorCode::Conflict)));
}

#[tokio::test]
async fn malformed_and_oversized_ipc_frames_do_not_dispatch() {
    use tokio::io::{AsyncReadExt,AsyncWriteExt};
    let (root,key) = fixture();
    let service = broker(root.path(),key,vec![]);
    let daemon = tokio::spawn(ipc::serve(Arc::clone(&service)));
    tokio::time::timeout(Duration::from_secs(2),async {while !service.socket().exists(){tokio::task::yield_now().await;}}).await.unwrap();
    let mut stream = tokio::net::UnixStream::connect(service.socket()).await.unwrap();
    stream.write_u32_le((MAX_FRAME_BYTES + 1) as u32).await.unwrap();
    let length = stream.read_u32_le().await.unwrap();
    assert!(length < 256);
    let mut bytes = vec![0;length as usize];
    stream.read_exact(&mut bytes).await.unwrap();
    assert!(matches!(serde_json::from_slice::<Reply>(&bytes).unwrap(),Reply::Error(ErrorCode::InvalidRequest)));
    assert!(!root.path().join("clients.json").exists());
    service.shutdown.cancel();
    daemon.await.unwrap().unwrap();
}

#[tokio::test]
async fn quarantine_and_missing_vault_cannot_turn_into_an_empty_ready_service() {
    let (root,key) = fixture();
    let service = broker(root.path(),Arc::clone(&key),vec![true,true]);
    let peer = pair(&service,"owner").await;
    enrolled(&service,&peer.token).await;
    service.quiesce().await;
    drop(service);
    let vault = root.path().join("vault/provisioned_secrets.vault");
    fs::rename(&vault,root.path().join("vault/provisioned_secrets.corrupt-fixture")).unwrap();
    assert!(storage::validate_vault(root.path()).is_err());
    // Even after an operator moves evidence out for inspection, ACL references
    // with missing material must not yield a misleading healthy empty service.
    fs::rename(root.path().join("vault/provisioned_secrets.corrupt-fixture"),root.path().join("saved-fixture.vault")).unwrap();
    let lease = storage::open(root.path()).unwrap();
    let empty = SecretStore::new(Box::new(SharedKey(key)),root.path().join("vault")).unwrap();
    assert!(Broker::with_components(lease,empty,Arc::new(FixtureHuman {answers:Mutex::new(VecDeque::new())})).is_err());
}

struct WaitingHuman {
    immediate: std::sync::atomic::AtomicUsize,
    waiting: tokio::sync::Notify,
}
#[async_trait]
impl HumanInteraction for WaitingHuman {
    async fn confirm(&self, _: &str, cancel: CancellationToken) -> Result<bool,ErrorCode> {
        use std::sync::atomic::Ordering;
        if self.immediate.fetch_update(Ordering::SeqCst,Ordering::SeqCst,|n| n.checked_sub(1)).is_ok() { return Ok(true); }
        self.waiting.notify_one();
        cancel.cancelled().await;
        Err(ErrorCode::Denied)
    }
    async fn secret(&self, _: &str, _: CancellationToken) -> Result<Zeroizing<String>,ErrorCode> { Ok(Zeroizing::new(CANARY.into())) }
}

#[tokio::test]
async fn pending_consent_is_caller_scoped_and_shutdown_never_grants_it() {
    let (root,key) = fixture();
    let human = Arc::new(WaitingHuman {immediate:std::sync::atomic::AtomicUsize::new(3),waiting:tokio::sync::Notify::new()});
    let lease = storage::open(root.path()).unwrap();
    let store = SecretStore::new(Box::new(SharedKey(Arc::clone(&key))),root.path().join("vault")).unwrap();
    let service = Broker::with_components(lease,store,human.clone()).unwrap();
    let owner = pair(&service,"owner").await;
    let agent = pair(&service,"agent").await;
    let metadata = enrolled(&service,&owner.token).await;
    let Response::Approval(pending) = call(&service,Request::RequestAccess(AccessRequest {credential_ref:metadata.credential_ref}),Some(&agent.token)).await else {panic!("pending");};
    tokio::time::timeout(Duration::from_secs(2),human.waiting.notified()).await.unwrap();
    let query = Request::ApprovalStatus(ApprovalQuery {approval_id:pending.approval_id});
    assert!(matches!(service.execute(envelope(&service,query.clone(),Some(&owner.token),Uuid::new_v4())).await,Reply::Error(ErrorCode::NotFound)));
    assert!(matches!(service.execute(envelope(&service,Request::Pair(PairRequest {label:"busy".into()}),None,Uuid::new_v4())).await,Reply::Error(ErrorCode::Busy)));
    tokio::time::timeout(Duration::from_secs(2),service.quiesce()).await.unwrap();
    drop(service);
    let restarted = broker(root.path(),key,vec![]);
    let Response::Credentials(rows) = call(&restarted,Request::ListCredentials,Some(&agent.token)).await else {panic!("list");};
    assert!(rows.is_empty());
    assert!(matches!(restarted.execute(envelope(&restarted,query,Some(&agent.token),Uuid::new_v4())).await,Reply::Error(ErrorCode::NotFound)));
}

#[tokio::test(flavor = "multi_thread")]
async fn concurrent_approval_replays_keep_one_request_binding_and_one_human_prompt() {
    let (root,key) = fixture();
    let human = Arc::new(WaitingHuman {immediate:std::sync::atomic::AtomicUsize::new(3),waiting:tokio::sync::Notify::new()});
    let lease = storage::open(root.path()).unwrap();
    let store = SecretStore::new(Box::new(SharedKey(key)),root.path().join("vault")).unwrap();
    let service = Broker::with_components(lease,store,human.clone()).unwrap();
    let owner = pair(&service,"owner").await;
    let agent = pair(&service,"agent").await;
    let metadata = enrolled(&service,&owner.token).await;
    let request = Request::RequestAccess(AccessRequest {credential_ref:metadata.credential_ref.clone()});
    let id = Uuid::new_v4();
    let mut calls = tokio::task::JoinSet::new();
    let start = Arc::new(tokio::sync::Barrier::new(16));
    for _ in 0..16 {
        let service = Arc::clone(&service);
        let envelope = envelope(&service,request.clone(),Some(&agent.token),id);
        let start = Arc::clone(&start);
        calls.spawn(async move {start.wait().await; service.execute(envelope).await});
    }
    tokio::time::timeout(Duration::from_secs(5),async {
        while let Some(reply) = calls.join_next().await {
            let Reply::Ok(Response::Approval(status)) = reply.unwrap() else {panic!("a replay must return the original approval, not Busy or a replacement");};
            assert_eq!(status.approval_id,id);
            assert_eq!(status.decision,Decision::Pending);
        }
    }).await.unwrap();
    tokio::time::timeout(Duration::from_secs(2),human.waiting.notified()).await.unwrap();
    assert!(matches!(service.execute(envelope(&service,request,Some(&owner.token),id)).await,Reply::Error(ErrorCode::Conflict)));
    let changed = Request::RequestAccess(AccessRequest {credential_ref:format!("cred_{}",Uuid::new_v4())});
    assert!(matches!(service.execute(envelope(&service,changed,Some(&agent.token),id)).await,Reply::Error(ErrorCode::Conflict)));
    service.quiesce().await;
}
