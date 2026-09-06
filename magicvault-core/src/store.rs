use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, RwLock};

use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use tracing::{debug, warn};
use zeroize::{Zeroize, ZeroizeOnDrop, Zeroizing};

use crate::session::{SessionContext, SessionCookie};

use super::encryption::{decrypt, encrypt, MasterKeyProvider, SecretEncryptionError};
use super::injection::{detect_jwt_expiry, filter_cookies_for_url, CookieWithMetadata};
use super::policy::{
    check_policy, AccessRequest, ApprovalTable, BoundGrantRedemption, BoundGrantRedemptionError,
    DelegatedGrantAuthority, GrantBinding, GrantTable, PolicyResult, RedemptionPayload,
    SecretPolicy, UsageTracker,
};
use super::{
    cookie_field_value, CookieSpec, InjectionTarget, SameSite, SecretEntry, SecretRef, SecretSource,
};

pub const SECRET_AUDIT_FILENAME: &str = "secret_audit.jsonl";
const MCP_OAUTH_VAULT_FILENAME: &str = "mcp_oauth.vault";
const MCP_OAUTH_VAULT_VERSION: u32 = 1;
const MAX_MCP_OAUTH_RECORDS: usize = 256;
const MAX_MCP_OAUTH_RECORD_BYTES: usize = 2 * 1024 * 1024;
const MAX_MCP_OAUTH_ENCODED_RECORD_BYTES: usize = ((MAX_MCP_OAUTH_RECORD_BYTES + 2) / 3) * 4;
const MAX_MCP_OAUTH_TOTAL_BYTES: usize = 32 * 1024 * 1024;
const MAX_MCP_OAUTH_FILE_BYTES: u64 = 48 * 1024 * 1024;
const MCP_OAUTH_DIGEST_HEX_BYTES: usize = 64;
const MCP_OAUTH_CREDENTIAL_PREFIX: &str = "credentials:";
const MCP_OAUTH_STATE_PREFIX: &str = "authorization-state:";
const MCP_OAUTH_PENDING_PREFIX: &str = "pending-authorization:";

/// Non-secret metadata about captured browser auth state.
#[derive(Debug, Clone, Serialize, Default, PartialEq, Eq)]
pub struct AuthStatusMetadata {
    pub has_auth: bool,
    pub is_stale: bool,
    pub has_cookies: bool,
    pub has_headers: bool,
    pub has_query_params: bool,
    pub has_storage: bool,
}

/// Captured auth entry that contributed to a replay session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapturedSessionTarget {
    pub origin: String,
    pub generation: u64,
}

/// Provenance for a replay session assembled from captured auth state.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CapturedSessionLease {
    stale_mark_targets: Vec<CapturedSessionTarget>,
}

impl CapturedSessionLease {
    fn push_target(&mut self, origin: &str, generation: u64) {
        if self
            .stale_mark_targets
            .iter()
            .any(|target| target.origin == origin && target.generation == generation)
        {
            return;
        }
        self.stale_mark_targets.push(CapturedSessionTarget {
            origin: origin.to_string(),
            generation,
        });
    }

    pub fn stale_mark_targets(&self) -> &[CapturedSessionTarget] {
        &self.stale_mark_targets
    }
}

/// Minimal secret metadata safe to expose to LLM-facing planning surfaces.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SecretListEntry {
    pub id: String,
    pub label: String,
}

/// Pending provisioned-secret approval safe to expose to the localhost UI/API.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PendingSecretApproval {
    pub challenge_id: String,
    pub secret_id: String,
    pub secret_label: String,
    pub tool: String,
    pub action: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub domain: Option<String>,
    pub expires_at: i64,
}

/// Explicit opt-in for a trusted application's metadata-only audit projection.
///
/// Implementors must contain no credential material, raw process output, argv,
/// or provider diagnostics. Do not implement this for unstructured JSON or text.
/// MagicVault owns the outer journal schema and write ordering; the application
/// owns its typed receipt. There is deliberately no blanket implementation.
pub trait AuditReceipt: Serialize {}
impl AuditReceipt for () {}

/// Core-generated events carry no product receipt.
pub type SecretAuditEvent = AuditEvent<()>;

/// Append-only audit event for secret resolution, approval, and injection activity.
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(bound(deserialize = "Receipt: Deserialize<'de>"))]
pub struct AuditEvent<Receipt> {
    pub timestamp: i64,
    pub event: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub secret_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub domain: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub challenge_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    runtime_credential_receipt: Option<Receipt>,
}

// Preserve the pre-extraction diagnostic name as well as the serialized fields.
impl<Receipt: std::fmt::Debug> std::fmt::Debug for AuditEvent<Receipt> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("SecretAuditEvent")
            .field("timestamp", &self.timestamp)
            .field("event", &self.event)
            .field("secret_id", &self.secret_id)
            .field("tool", &self.tool)
            .field("action", &self.action)
            .field("domain", &self.domain)
            .field("challenge_id", &self.challenge_id)
            .field("detail", &self.detail)
            .field("runtime_credential_receipt", &self.runtime_credential_receipt)
            .finish()
    }
}

impl<Receipt> AuditEvent<Receipt> {
    pub fn new(event: impl Into<String>) -> Self {
        Self::new_at(event, Utc::now().timestamp())
    }

    pub fn new_at(event: impl Into<String>, timestamp: i64) -> Self {
        Self {
            timestamp,
            event: event.into(),
            secret_id: None,
            tool: None,
            action: None,
            domain: None,
            challenge_id: None,
            detail: None,
            runtime_credential_receipt: None,
        }
    }

    pub fn with_runtime_credential_receipt(
        mut self,
        receipt: Receipt,
    ) -> Self {
        self.runtime_credential_receipt = Some(receipt);
        self
    }

    pub fn runtime_credential_receipt(&self) -> Option<&Receipt> {
        self.runtime_credential_receipt.as_ref()
    }

    pub fn with_secret_id(mut self, secret_id: impl Into<String>) -> Self {
        self.secret_id = Some(secret_id.into());
        self
    }

    pub fn with_tool(mut self, tool: impl Into<String>) -> Self {
        self.tool = Some(tool.into());
        self
    }

    pub fn with_action(mut self, action: impl Into<String>) -> Self {
        self.action = Some(action.into());
        self
    }

    pub fn with_domain(mut self, domain: impl Into<String>) -> Self {
        self.domain = Some(domain.into());
        self
    }

    pub fn with_optional_domain(mut self, domain: Option<&str>) -> Self {
        self.domain = domain.map(ToString::to_string);
        self
    }

    pub fn with_challenge_id(mut self, challenge_id: impl Into<String>) -> Self {
        self.challenge_id = Some(challenge_id.into());
        self
    }

    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }
}

/// Secret source partitions that can be enabled or disabled independently.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum SecretSourceKind {
    Provisioned,
    Captured,
    Ephemeral,
}

impl SecretSourceKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Provisioned => "provisioned",
            Self::Captured => "captured",
            Self::Ephemeral => "ephemeral",
        }
    }
}

impl std::fmt::Display for SecretSourceKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Runtime support level for a secret source partition.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SecretFeatureSupport {
    Available,
    MemoryOnly,
    Disabled,
}

/// Public runtime status for a secret source partition.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SecretFeatureStatus {
    pub support: SecretFeatureSupport,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

impl SecretFeatureStatus {
    pub fn available() -> Self {
        Self {
            support: SecretFeatureSupport::Available,
            reason: None,
        }
    }

    pub fn memory_only(reason: impl Into<String>) -> Self {
        Self {
            support: SecretFeatureSupport::MemoryOnly,
            reason: Some(reason.into()),
        }
    }

    pub fn disabled(reason: impl Into<String>) -> Self {
        Self {
            support: SecretFeatureSupport::Disabled,
            reason: Some(reason.into()),
        }
    }

    pub fn is_available(&self) -> bool {
        self.support != SecretFeatureSupport::Disabled
    }
}

/// Runtime capability snapshot for the shared secret system.
///
/// This is explicit rather than inferred from the selected key provider so the
/// rest of the runtime can disable features cleanly when no durable backend is
/// available. That avoids "encrypted but not really persistent" false paths.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SecretRuntimeCapabilities {
    pub provider_name: String,
    pub provisioned: SecretFeatureStatus,
    pub captured: SecretFeatureStatus,
    pub ephemeral: SecretFeatureStatus,
}

impl SecretRuntimeCapabilities {
    pub fn fully_available(provider_name: impl Into<String>) -> Self {
        Self {
            provider_name: provider_name.into(),
            provisioned: SecretFeatureStatus::available(),
            captured: SecretFeatureStatus::available(),
            ephemeral: SecretFeatureStatus::available(),
        }
    }

    pub fn without_durable_storage(
        provider_name: impl Into<String>,
        reason: impl Into<String>,
    ) -> Self {
        let reason = reason.into();
        Self {
            provider_name: provider_name.into(),
            provisioned: SecretFeatureStatus::disabled(reason.clone()),
            captured: SecretFeatureStatus::disabled(reason),
            ephemeral: SecretFeatureStatus::available(),
        }
    }

    pub fn status_for(&self, source: SecretSourceKind) -> &SecretFeatureStatus {
        match source {
            SecretSourceKind::Provisioned => &self.provisioned,
            SecretSourceKind::Captured => &self.captured,
            SecretSourceKind::Ephemeral => &self.ephemeral,
        }
    }

    pub fn treasurer_enabled(&self) -> bool {
        self.provisioned.support == SecretFeatureSupport::Available
    }
}

#[derive(Zeroize, ZeroizeOnDrop)]
struct McpOAuthVaultRecord {
    document: Vec<u8>,
}

impl McpOAuthVaultRecord {
    fn new(document: &[u8]) -> Self {
        Self {
            document: document.to_vec(),
        }
    }

    fn as_bytes(&self) -> &[u8] {
        &self.document
    }
}

impl std::fmt::Debug for McpOAuthVaultRecord {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("McpOAuthVaultRecord([REDACTED])")
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PersistedMcpOAuthPartition {
    version: u32,
    records: HashMap<String, String>,
}

struct McpOAuthPersistFailure {
    source: SecretStoreError,
    commit_state_unknown: bool,
}

#[derive(Debug, Default)]
struct SecretStoreState {
    provisioned: HashMap<String, SecretEntry>,
    captured: HashMap<String, SecretEntry>,
    ephemeral: HashMap<String, SecretEntry>,
    mcp_oauth: HashMap<String, McpOAuthVaultRecord>,
    mcp_oauth_status: McpOAuthPartitionStatus,
    provisioned_status: SecretPartitionStatus,
    captured_status: SecretPartitionStatus,
}

/// Whether a durable partition's last load actually produced its contents.
///
/// An unreadable partition is read as **empty** rather than as a refusal to
/// start. Partitions are per-scope, so failing the load would take the whole
/// daemon down over one scope's damaged file, and an empty partition already
/// fails every lookup closed — which is the safe direction for a credential.
///
/// What empty does not do is tell anyone apart from "nothing was ever
/// provisioned here", and that is the expensive half: the operator is shown a
/// vault with no secrets, re-provisions one, and the surviving entries are only
/// in the quarantine file nobody thought to look for. This status is that
/// distinction, so a caller can say *the vault could not be read* instead of
/// *you have no secrets*.
///
/// Writes stay permitted while a partition is `Unavailable`. The bytes that
/// could not be read are already preserved next to the vault under a
/// `.corrupt-<uuid>` name, so a later write replaces nothing that quarantine has
/// not saved, and refusing writes would leave the scope with no way forward
/// short of manual repair.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum SecretPartitionStatus {
    /// Loaded — including the ordinary case of a partition that does not exist
    /// yet because nothing has ever been written to it.
    #[default]
    Loaded,
    /// The last load failed. The partition is empty in memory, and the file is
    /// quarantined on disk when it was unreadable rather than merely
    /// inaccessible.
    Unavailable,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum McpOAuthPartitionStatus {
    #[default]
    Ready,
    Unavailable,
    CommitUnknown,
}

struct PendingCapturedTask {
    execution_id: Option<String>,
    handle: tokio::task::JoinHandle<()>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistedPartition {
    version: u32,
    entries: HashMap<String, SecretEntry>,
}

#[derive(Debug)]
struct StoreConfig {
    captured_max_origins: usize,
    default_grant_ttl_secs: i64,
}

impl Default for StoreConfig {
    fn default() -> Self {
        Self {
            captured_max_origins: 500,
            default_grant_ttl_secs: 300,
        }
    }
}

/// Encrypted store shared by provisioned, captured, and ephemeral secret flows.
pub struct SecretStore {
    key_provider: Arc<dyn MasterKeyProvider>,
    base_dir: PathBuf,
    state: RwLock<SecretStoreState>,
    usage_tracker: UsageTracker,
    approval_table: ApprovalTable,
    grant_table: GrantTable,
    config: RwLock<StoreConfig>,
    capabilities: SecretRuntimeCapabilities,
    pending_captured_tasks: Mutex<Vec<PendingCapturedTask>>,
    audit_write_lock: Mutex<()>,
    /// Set to true when captured state changes in memory but hasn't been flushed to disk yet.
    captured_dirty: AtomicBool,
    /// Guards against concurrent background flush operations.
    flush_in_flight: AtomicBool,
    /// Serializes durable writes of the provisioned partition.
    ///
    /// The snapshot a persist writes is cloned under `state.read()`, which two
    /// writers can hold at once. Without this lock they clone different
    /// generations and may rename them in the opposite order, so the older
    /// snapshot lands last and the newer entries are gone from disk while still
    /// sitting in memory — a loss that only shows up after a restart.
    ///
    /// One lock per partition file rather than one for the store: a captured
    /// flush serializes, encrypts, and fsyncs every origin, and a provisioned
    /// write must not queue behind that.
    provisioned_write_lock: Mutex<()>,
    /// Serializes durable writes of the captured partition. See
    /// [`SecretStore::provisioned_write_lock`].
    captured_write_lock: Mutex<()>,
}

/// Take a persist lock, clearing poison rather than propagating it.
///
/// These locks guard write *ordering* and hold no data, so a writer that
/// panicked mid-write left nothing behind to observe — the durable write commits
/// by rename, and a panic before it leaves only a uniquely named temp file.
/// Propagating the poison would turn one panic into a vault that can never be
/// written again for the life of the process, which is strictly worse than the
/// contention the lock exists to remove.
fn lock_for_persist(lock: &Mutex<()>) -> std::sync::MutexGuard<'_, ()> {
    lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Publish `bytes` at `path` so a crash leaves either the previous file or the
/// new one, never a half-written one.
///
/// Four properties, and a vault needs all four:
///
/// * **A temp name unique per write.** Every writer of a partition shares its
///   directory, so a fixed `*.tmp` is two writers interleaving inside one file
///   and then renaming the mixture over the vault.
/// * **`sync_all` on the temp before the rename.** Without it the rename can be
///   durable while the contents are not, and the crash window leaves a vault
///   that is present, zero-length, and undecryptable.
/// * **`sync_all` on the parent directory after it.** The rename is a directory
///   mutation; until the directory entry is durable the old name may come back.
/// * **The temp removed on every failure path.** A unique name is only an
///   improvement if a failed write does not leak a file holding the encrypted
///   vault into the scope.
///
/// The shared `io::Result` writer is owned by `magicvault-primitives`. Magician's
/// historical durable-I/O path re-exports this same implementation, including
/// retry timing, temporary naming, sync ordering, and cleanup. This is a name
/// for the vault's intent, not a second copy of the mechanism. A partition path lives directly
/// inside the store's base directory, so the helper's parent-dir sync covers
/// exactly the directory the old signature took as an argument.
fn write_file_durably(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    magicvault_primitives::durable_io::write_bytes_durably_sync(path, bytes)
}

/// Open options that create vault files owner-read/write only. The mode
/// applies on creation; existing files are tightened by `restrict_vault_file`.
fn private_file_options() -> std::fs::OpenOptions {
    let mut options = std::fs::OpenOptions::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options
}

/// The vault directory is owner-only. Applied after every `create_dir_all`,
/// so a directory created under a permissive umask is tightened on first use.
#[cfg(unix)]
fn restrict_vault_dir(dir: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
}

#[cfg(not(unix))]
fn restrict_vault_dir(_dir: &Path) -> std::io::Result<()> {
    Ok(())
}

/// Vault files are owner-only. Applied after every write, which also tightens
/// a file that predates this rule.
#[cfg(unix)]
fn restrict_vault_file(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
}

#[cfg(not(unix))]
fn restrict_vault_file(_path: &Path) -> std::io::Result<()> {
    Ok(())
}

/// Trusted application-owned scope path mapping.
///
/// The host must validate/isolate principal and workspace identifiers using its
/// existing scope rules. This contract does not accept paths from a model-facing
/// tool. Constructing the layout must not open a second secret store.
pub trait SecretScopeLayout: Clone + Send + Sync {
    fn from_base_root(base_root: &Path) -> Self;
    fn secrets_root(&self, principal: &str, workspace: &str) -> PathBuf;
}

/// Scope-aware resolver with one canonical store and load slot per scope.
/// The application supplies its existing path layout; custody/cache/flush logic
/// stays shared, including recovery and quarantine status on first resolution.
#[derive(Clone)]
pub struct SecretStoreResolver<Layout: SecretScopeLayout> {
    workspace_layout: Layout,
    key_provider: Arc<dyn MasterKeyProvider>,
    capabilities: SecretRuntimeCapabilities,
    captured_max_origins: Arc<AtomicUsize>,
    stores: Arc<Mutex<HashMap<(String, String), Arc<ScopeStoreSlot>>>>,
}

#[derive(Default)]
struct ScopeStoreSlot {
    store: Mutex<Option<Arc<SecretStore>>>,
}

/// Errors produced by the shared secret store.
#[derive(Debug, thiserror::Error)]
pub enum SecretStoreError {
    #[error(transparent)]
    Encryption(#[from] SecretEncryptionError),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),

    #[error("secret audit persistence is unavailable")]
    AuditUnavailable,

    #[error("MCP OAuth vault record is invalid")]
    McpOAuthInvalidRecord,

    #[error("MCP OAuth vault record already exists")]
    McpOAuthConflict,

    #[error("MCP OAuth vault capacity is exhausted")]
    McpOAuthCapacity,

    #[error("MCP OAuth vault is unavailable")]
    McpOAuthUnavailable,

    #[error("MCP OAuth vault commit state is unknown; reconcile before retrying")]
    McpOAuthCommitStateUnknown,

    #[error("provisioned secrets do not support inline injection")]
    InlineProvisionedSecret,

    #[error("secret '{0}' was not found")]
    SecretNotFound(String),

    #[error("grant was not found or expired")]
    GrantNotFound(String),

    #[error("secret access denied: {0}")]
    PolicyDenied(String),

    #[error("secret access requires approval challenge '{0}'")]
    ApprovalRequired(String),

    #[error("{feature} secret storage is unavailable: {reason}")]
    FeatureDisabled {
        feature: SecretSourceKind,
        reason: String,
    },
}

impl SecretStore {
    /// Snapshot secret values solely for an in-process redaction pass. Values
    /// are zeroized when the returned snapshot is dropped and must never be
    /// logged, persisted, fingerprinted, or exposed through an API.
    pub fn redaction_values(&self) -> Vec<Zeroizing<String>> {
        let state = self.state.read().expect("secret store lock poisoned");
        state
            .provisioned
            .values()
            .chain(state.captured.values())
            .chain(state.ephemeral.values())
            .flat_map(|entry| entry.fields.values())
            .filter(|value| value.len() >= 4)
            .cloned()
            .map(Zeroizing::new)
            .collect()
    }

    pub fn new(
        key_provider: Box<dyn MasterKeyProvider>,
        base_dir: PathBuf,
    ) -> Result<Self, SecretStoreError> {
        let key_provider = Arc::<dyn MasterKeyProvider>::from(key_provider);
        let provider_name = key_provider.provider_name().to_string();
        let store = Self::new_empty_with_capabilities(
            key_provider,
            base_dir,
            SecretRuntimeCapabilities::fully_available(provider_name),
        );
        store.load_from_disk()?;
        Ok(store)
    }

    pub fn open(key_provider: Box<dyn MasterKeyProvider>, base_dir: PathBuf) -> Self {
        let key_provider = Arc::<dyn MasterKeyProvider>::from(key_provider);
        let provider_name = key_provider.provider_name().to_string();
        let store = Self::new_empty_with_capabilities(
            key_provider,
            base_dir,
            SecretRuntimeCapabilities::fully_available(provider_name),
        );
        if let Err(err) = store.load_from_disk_lenient() {
            warn!("secret store: recovered from startup load error: {}", err);
        }
        store
    }

    pub fn new_empty(key_provider: Box<dyn MasterKeyProvider>, base_dir: PathBuf) -> Self {
        let key_provider = Arc::<dyn MasterKeyProvider>::from(key_provider);
        let provider_name = key_provider.provider_name().to_string();
        Self::new_empty_with_capabilities(
            key_provider,
            base_dir,
            SecretRuntimeCapabilities::fully_available(provider_name),
        )
    }

    pub fn new_with_capabilities(
        key_provider: Box<dyn MasterKeyProvider>,
        base_dir: PathBuf,
        capabilities: SecretRuntimeCapabilities,
    ) -> Result<Self, SecretStoreError> {
        let store = Self::new_empty_with_capabilities(
            Arc::<dyn MasterKeyProvider>::from(key_provider),
            base_dir,
            capabilities,
        );
        store.load_from_disk()?;
        Ok(store)
    }

    pub fn open_with_capabilities(
        key_provider: Box<dyn MasterKeyProvider>,
        base_dir: PathBuf,
        capabilities: SecretRuntimeCapabilities,
    ) -> Self {
        let store = Self::new_empty_with_capabilities(
            Arc::<dyn MasterKeyProvider>::from(key_provider),
            base_dir,
            capabilities,
        );
        if let Err(err) = store.load_from_disk_lenient() {
            warn!("secret store: recovered from startup load error: {}", err);
        }
        store
    }

    pub fn new_empty_with_capabilities(
        key_provider: Arc<dyn MasterKeyProvider>,
        base_dir: PathBuf,
        capabilities: SecretRuntimeCapabilities,
    ) -> Self {
        Self {
            key_provider,
            base_dir,
            state: RwLock::new(SecretStoreState::default()),
            usage_tracker: UsageTracker::default(),
            approval_table: ApprovalTable::default(),
            grant_table: GrantTable::default(),
            config: RwLock::new(StoreConfig::default()),
            capabilities,
            pending_captured_tasks: Mutex::new(Vec::new()),
            audit_write_lock: Mutex::new(()),
            captured_dirty: AtomicBool::new(false),
            flush_in_flight: AtomicBool::new(false),
            provisioned_write_lock: Mutex::new(()),
            captured_write_lock: Mutex::new(()),
        }
    }

    /// Whether the durable partition behind `source` was readable at load.
    ///
    /// See [`SecretPartitionStatus`]. An empty partition and an unreadable one
    /// both return nothing from every lookup; this is how a caller tells them
    /// apart. `Ephemeral` has no durable partition and is always `Loaded`.
    pub fn partition_status(&self, source: SecretSourceKind) -> SecretPartitionStatus {
        let guard = self.state.read().expect("secret store state lock poisoned");
        match source {
            SecretSourceKind::Provisioned => guard.provisioned_status,
            SecretSourceKind::Captured => guard.captured_status,
            SecretSourceKind::Ephemeral => SecretPartitionStatus::Loaded,
        }
    }

    pub fn runtime_capabilities(&self) -> &SecretRuntimeCapabilities {
        &self.capabilities
    }

    pub fn feature_status(&self, source: SecretSourceKind) -> &SecretFeatureStatus {
        self.capabilities.status_for(source)
    }

    pub fn ensure_feature_enabled(&self, source: SecretSourceKind) -> Result<(), SecretStoreError> {
        let status = self.capabilities.status_for(source);
        if status.is_available() {
            return Ok(());
        }

        Err(SecretStoreError::FeatureDisabled {
            feature: source,
            reason: status
                .reason
                .clone()
                .unwrap_or_else(|| "feature disabled".to_string()),
        })
    }

    pub fn set_captured_max_origins(&self, value: usize) {
        self.config
            .write()
            .expect("secret store config lock poisoned")
            .captured_max_origins = value.max(1);
    }

    pub fn set_default_grant_ttl_secs(&self, value: i64) {
        self.config
            .write()
            .expect("secret store config lock poisoned")
            .default_grant_ttl_secs = value.max(1);
    }

    pub fn audit_event<Receipt: AuditReceipt>(&self, event: AuditEvent<Receipt>) {
        if let Err(err) = self.append_audit_event(&event) {
            warn!(
                "secret store: failed to append audit event '{}' to '{}': {}",
                event.event,
                self.audit_path().display(),
                err
            );
        }
    }

    pub fn try_audit_event<Receipt: AuditReceipt>(&self, event: AuditEvent<Receipt>) -> Result<(), SecretStoreError> {
        self.append_audit_event(&event)
    }

    pub fn store_provisioned(
        &self,
        id: impl Into<String>,
        label: impl Into<String>,
        fields: HashMap<String, String>,
        injection: InjectionTarget,
        policy: SecretPolicy,
    ) -> Result<SecretRef, SecretStoreError> {
        self.ensure_feature_enabled(SecretSourceKind::Provisioned)?;
        if matches!(injection, InjectionTarget::Inline) {
            return Err(SecretStoreError::InlineProvisionedSecret);
        }

        let id = id.into();
        let entry = SecretEntry {
            id: id.clone(),
            label: label.into(),
            fields,
            source: SecretSource::Provisioned,
            injection,
            policy: Some(policy),
            created_at: Utc::now().timestamp(),
        };

        self.state
            .write()
            .expect("secret store state lock poisoned")
            .provisioned
            .insert(id.clone(), entry);
        self.persist_provisioned()?;

        Ok(SecretRef::Provisioned(id))
    }

    pub fn list_available(&self) -> Vec<SecretListEntry> {
        if !self
            .feature_status(SecretSourceKind::Provisioned)
            .is_available()
        {
            return Vec::new();
        }
        let mut secrets = self
            .state
            .read()
            .expect("secret store state lock poisoned")
            .provisioned
            .values()
            .map(|entry| SecretListEntry {
                id: entry.id.clone(),
                label: entry.label.clone(),
            })
            .collect::<Vec<_>>();
        secrets.sort_by(|left, right| left.label.cmp(&right.label).then(left.id.cmp(&right.id)));
        secrets
    }

    pub fn list_provisioned_entries(&self) -> Vec<SecretEntry> {
        if !self
            .feature_status(SecretSourceKind::Provisioned)
            .is_available()
        {
            return Vec::new();
        }
        let mut entries = self
            .state
            .read()
            .expect("secret store state lock poisoned")
            .provisioned
            .values()
            .cloned()
            .collect::<Vec<_>>();
        entries.sort_by(|left, right| {
            left.label
                .cmp(&right.label)
                .then(left.id.cmp(&right.id))
                .then(left.created_at.cmp(&right.created_at))
        });
        entries
    }

    pub fn get_provisioned(&self, secret_id: &str) -> Option<SecretEntry> {
        if !self
            .feature_status(SecretSourceKind::Provisioned)
            .is_available()
        {
            return None;
        }
        self.state
            .read()
            .expect("secret store state lock poisoned")
            .provisioned
            .get(secret_id)
            .cloned()
    }

    /// Value-free exact-id presence check used by sealed credential adapters
    /// to choose canonical vault authority over a transitional legacy source.
    /// Availability is deliberately not consulted: an existing canonical
    /// record must continue to block fallback when its feature is unavailable.
    pub fn contains_provisioned(&self, secret_id: &str) -> bool {
        self.state
            .read()
            .expect("secret store state lock poisoned")
            .provisioned
            .contains_key(secret_id)
    }

    pub fn delete_provisioned(&self, secret_id: &str) -> Result<bool, SecretStoreError> {
        self.ensure_feature_enabled(SecretSourceKind::Provisioned)?;
        let removed = self
            .state
            .write()
            .expect("secret store state lock poisoned")
            .provisioned
            .remove(secret_id)
            .is_some();
        if removed {
            self.persist_provisioned()?;
        }
        Ok(removed)
    }

    /// Read one exact-binding MCP OAuth document from the scope-owned encrypted vault.
    ///
    /// The record identifier is an opaque domain-separated digest produced by
    /// `magician-mcp-client`; no profile, endpoint, issuer, CSRF token, or credential
    /// value is accepted as a filename.
    pub fn read_mcp_oauth(&self, record_id: &str) -> Result<Option<Vec<u8>>, SecretStoreError> {
        self.ensure_feature_enabled(SecretSourceKind::Provisioned)?;
        validate_mcp_oauth_record_id(record_id)?;
        let guard = self
            .state
            .read()
            .map_err(|_| SecretStoreError::McpOAuthCommitStateUnknown)?;
        ensure_mcp_oauth_partition_ready(&guard)?;
        Ok(guard
            .mcp_oauth
            .get(record_id)
            .map(|record| record.as_bytes().to_vec()))
    }

    /// Atomically create or replace one bounded MCP OAuth document.
    pub fn write_mcp_oauth(
        &self,
        record_id: &str,
        document: &[u8],
        create_only: bool,
    ) -> Result<(), SecretStoreError> {
        self.ensure_feature_enabled(SecretSourceKind::Provisioned)?;
        validate_mcp_oauth_record_id(record_id)?;
        validate_mcp_oauth_document(document)?;

        let mut guard = self
            .state
            .write()
            .map_err(|_| SecretStoreError::McpOAuthCommitStateUnknown)?;
        ensure_mcp_oauth_partition_ready(&guard)?;
        if create_only && guard.mcp_oauth.contains_key(record_id) {
            return Err(SecretStoreError::McpOAuthConflict);
        }
        validate_mcp_oauth_capacity(&guard.mcp_oauth, record_id, document.len())?;

        let previous = guard
            .mcp_oauth
            .insert(record_id.to_owned(), McpOAuthVaultRecord::new(document));
        match self.persist_mcp_oauth_partition(&guard.mcp_oauth) {
            Ok(()) => Ok(()),
            Err(failure) if failure.commit_state_unknown => {
                guard.mcp_oauth_status = McpOAuthPartitionStatus::CommitUnknown;
                Err(SecretStoreError::McpOAuthCommitStateUnknown)
            },
            Err(failure) => {
                if let Some(previous) = previous {
                    guard.mcp_oauth.insert(record_id.to_owned(), previous);
                } else {
                    guard.mcp_oauth.remove(record_id);
                }
                Err(failure.source)
            },
        }
    }

    /// Atomically consume one callback-state record.
    ///
    /// The document is returned only after the removal is durably published. A
    /// pre-commit failure restores the in-memory record; an ambiguous post-rename
    /// failure keeps it consumed and blocks the partition until restart reloads
    /// authoritative disk state.
    pub fn take_mcp_oauth(&self, record_id: &str) -> Result<Option<Vec<u8>>, SecretStoreError> {
        self.ensure_feature_enabled(SecretSourceKind::Provisioned)?;
        validate_mcp_oauth_record_id(record_id)?;
        let mut guard = self
            .state
            .write()
            .map_err(|_| SecretStoreError::McpOAuthCommitStateUnknown)?;
        ensure_mcp_oauth_partition_ready(&guard)?;
        let Some(record) = guard.mcp_oauth.remove(record_id) else {
            return Ok(None);
        };
        match self.persist_mcp_oauth_partition(&guard.mcp_oauth) {
            Ok(()) => Ok(Some(record.as_bytes().to_vec())),
            Err(failure) if failure.commit_state_unknown => {
                guard.mcp_oauth_status = McpOAuthPartitionStatus::CommitUnknown;
                Err(SecretStoreError::McpOAuthCommitStateUnknown)
            },
            Err(failure) => {
                guard.mcp_oauth.insert(record_id.to_owned(), record);
                Err(failure.source)
            },
        }
    }

    /// Idempotently remove an MCP OAuth record.
    pub fn delete_mcp_oauth(&self, record_id: &str) -> Result<(), SecretStoreError> {
        self.ensure_feature_enabled(SecretSourceKind::Provisioned)?;
        validate_mcp_oauth_record_id(record_id)?;
        let mut guard = self
            .state
            .write()
            .map_err(|_| SecretStoreError::McpOAuthCommitStateUnknown)?;
        ensure_mcp_oauth_partition_ready(&guard)?;
        let Some(record) = guard.mcp_oauth.remove(record_id) else {
            return Ok(());
        };
        match self.persist_mcp_oauth_partition(&guard.mcp_oauth) {
            Ok(()) => Ok(()),
            Err(failure) if failure.commit_state_unknown => {
                guard.mcp_oauth_status = McpOAuthPartitionStatus::CommitUnknown;
                Err(SecretStoreError::McpOAuthCommitStateUnknown)
            },
            Err(failure) => {
                guard.mcp_oauth.insert(record_id.to_owned(), record);
                Err(failure.source)
            },
        }
    }

    pub fn evaluate_access(
        &self,
        secret_id: &str,
        tool: &str,
        action: &str,
        domain: Option<&str>,
    ) -> Result<PolicyResult, SecretStoreError> {
        self.ensure_feature_enabled(SecretSourceKind::Provisioned)?;
        let entry = self
            .get_provisioned(secret_id)
            .ok_or_else(|| SecretStoreError::SecretNotFound(secret_id.to_string()))?;
        let request = AccessRequest::new(
            secret_id,
            tool,
            action,
            domain.map(|value| value.to_string()),
        );

        let result = match entry.policy.as_ref() {
            Some(policy) => {
                check_policy(policy, &request, &self.usage_tracker, &self.approval_table)
            },
            None => PolicyResult::Allowed,
        };
        Ok(result)
    }

    pub fn record_usage(
        &self,
        secret_id: &str,
        at_ts: Option<i64>,
    ) -> Result<(), SecretStoreError> {
        self.ensure_feature_enabled(SecretSourceKind::Provisioned)?;
        let entry = self
            .get_provisioned(secret_id)
            .ok_or_else(|| SecretStoreError::SecretNotFound(secret_id.to_string()))?;
        let limit = entry
            .policy
            .as_ref()
            .and_then(|policy| policy.max_uses_per_day);
        if !self.usage_tracker.try_record_use(
            secret_id,
            at_ts.unwrap_or_else(|| Utc::now().timestamp()),
            limit,
        ) {
            return Err(SecretStoreError::PolicyDenied(
                "daily credential usage limit exceeded".to_owned(),
            ));
        }
        Ok(())
    }

    pub fn issue_grant(
        &self,
        secret_id: &str,
        tool: &str,
        action: &str,
        domain: Option<&str>,
        ttl_secs: Option<i64>,
    ) -> Result<SecretRef, SecretStoreError> {
        self.ensure_feature_enabled(SecretSourceKind::Provisioned)?;
        match self.evaluate_access(secret_id, tool, action, domain)? {
            PolicyResult::Allowed => {},
            PolicyResult::Denied { reason } => return Err(SecretStoreError::PolicyDenied(reason)),
            PolicyResult::NeedsApproval { challenge } => {
                return Err(SecretStoreError::ApprovalRequired(challenge.id));
            },
        }

        let request = AccessRequest::new(
            secret_id,
            tool,
            action,
            domain.map(|value| value.to_string()),
        );
        self.issue_grant_with_binding(secret_id, GrantBinding::from_request(&request), ttl_secs)
            .map(SecretRef::Grant)
    }

    pub fn issue_delegated_grant(
        &self,
        secret_id: &str,
        tool: &str,
        action: &str,
        domain: Option<&str>,
        authority: DelegatedGrantAuthority,
        ttl_secs: Option<i64>,
    ) -> Result<String, SecretStoreError> {
        self.ensure_feature_enabled(SecretSourceKind::Provisioned)?;
        match self.evaluate_access(secret_id, tool, action, domain)? {
            PolicyResult::Allowed => {},
            PolicyResult::Denied { reason } => return Err(SecretStoreError::PolicyDenied(reason)),
            PolicyResult::NeedsApproval { challenge } => {
                return Err(SecretStoreError::ApprovalRequired(challenge.id));
            },
        }
        let request = AccessRequest::new(secret_id, tool, action, domain.map(ToOwned::to_owned));
        self.issue_grant_with_binding(
            secret_id,
            GrantBinding::delegated(&request, authority),
            ttl_secs,
        )
    }

    fn issue_grant_with_binding(
        &self,
        secret_id: &str,
        binding: GrantBinding,
        ttl_secs: Option<i64>,
    ) -> Result<String, SecretStoreError> {
        let entry = self
            .get_provisioned(secret_id)
            .ok_or_else(|| SecretStoreError::SecretNotFound(secret_id.to_string()))?;
        let ttl_secs = ttl_secs.unwrap_or_else(|| {
            self.config
                .read()
                .expect("secret store config lock poisoned")
                .default_grant_ttl_secs
        });

        let grant_id = self.grant_table.issue_grant(
            secret_id,
            entry.fields,
            entry.injection,
            binding,
            ttl_secs,
        );
        Ok(grant_id)
    }

    pub fn redeem_grant(&self, grant_id: &str) -> Result<RedemptionPayload, SecretStoreError> {
        self.ensure_feature_enabled(SecretSourceKind::Provisioned)?;
        self.grant_table
            .redeem_grant(grant_id)
            .ok_or_else(|| SecretStoreError::GrantNotFound(grant_id.to_string()))
    }

    pub fn redeem_bound_grants(
        &self,
        requests: &[BoundGrantRedemption<'_>],
    ) -> Result<Vec<RedemptionPayload>, BoundGrantRedemptionError> {
        self.grant_table.redeem_bound_batch(requests)
    }

    pub fn discard_grants(&self, grant_ids: &[&str]) {
        self.grant_table.discard_grants(grant_ids);
    }

    pub fn grant_approval(&self, challenge_id: &str) -> Result<bool, SecretStoreError> {
        self.ensure_feature_enabled(SecretSourceKind::Provisioned)?;
        let challenge = self.approval_table.grant(challenge_id);
        if let Some(challenge) = challenge {
            self.audit_event(
                SecretAuditEvent::new("approval_granted")
                    .with_secret_id(challenge.secret_id)
                    .with_tool(challenge.tool)
                    .with_action(challenge.action)
                    .with_optional_domain(challenge.domain.as_deref())
                    .with_challenge_id(challenge.id),
            );
            Ok(true)
        } else {
            self.audit_event(
                SecretAuditEvent::new("approval_grant_failed")
                    .with_challenge_id(challenge_id.to_string())
                    .with_detail("challenge_missing_or_expired"),
            );
            Ok(false)
        }
    }

    pub fn list_pending_approvals(&self) -> Vec<PendingSecretApproval> {
        if !self
            .feature_status(SecretSourceKind::Provisioned)
            .is_available()
        {
            return Vec::new();
        }
        self.approval_table
            .list_pending()
            .into_iter()
            .map(|challenge| PendingSecretApproval {
                challenge_id: challenge.id.clone(),
                secret_id: challenge.secret_id.clone(),
                secret_label: self
                    .get_provisioned(&challenge.secret_id)
                    .map(|entry| entry.label)
                    .unwrap_or_else(|| challenge.secret_id.clone()),
                tool: challenge.tool,
                action: challenge.action,
                domain: challenge.domain,
                expires_at: challenge.expires_at,
            })
            .collect()
    }

    pub fn approve_request(
        &self,
        secret_id: &str,
        tool: &str,
        action: &str,
        domain: Option<&str>,
    ) -> Result<(), SecretStoreError> {
        self.ensure_feature_enabled(SecretSourceKind::Provisioned)?;
        if self.get_provisioned(secret_id).is_none() {
            return Err(SecretStoreError::SecretNotFound(secret_id.to_string()));
        }

        let request = AccessRequest::new(secret_id, tool, action, domain.map(ToString::to_string));
        self.approval_table.approve_request(&request);
        self.audit_event(
            SecretAuditEvent::new("approval_granted_restored")
                .with_secret_id(secret_id.to_string())
                .with_tool(tool.to_string())
                .with_action(action.to_string())
                .with_optional_domain(domain)
                .with_detail("pause_resume"),
        );
        Ok(())
    }

    pub fn register_ephemeral(
        &self,
        task_id: &str,
        input_id: &str,
        value: String,
    ) -> Result<SecretRef, SecretStoreError> {
        self.ensure_feature_enabled(SecretSourceKind::Ephemeral)?;
        let entry = SecretEntry {
            id: input_id.to_string(),
            label: input_id.to_string(),
            fields: HashMap::from([("value".to_string(), value)]),
            source: SecretSource::Ephemeral {
                task_id: task_id.to_string(),
            },
            injection: InjectionTarget::Inline,
            policy: None,
            created_at: Utc::now().timestamp(),
        };
        self.state
            .write()
            .expect("secret store state lock poisoned")
            .ephemeral
            .insert(scoped_ephemeral_key(task_id, input_id), entry);
        Ok(SecretRef::Placeholder(input_id.to_string()))
    }

    pub fn get_ephemeral(&self, input_id: &str) -> Option<String> {
        if !self
            .feature_status(SecretSourceKind::Ephemeral)
            .is_available()
        {
            return None;
        }
        let guard = self.state.read().expect("secret store state lock poisoned");
        let mut matches = guard
            .ephemeral
            .values()
            .filter(|entry| entry.id == input_id)
            .filter_map(|entry| entry.fields.get("value"))
            .cloned();
        let first = matches.next()?;
        if matches.next().is_some() {
            return None;
        }
        Some(first)
    }

    pub fn get_ephemeral_scoped(&self, task_id: &str, input_id: &str) -> Option<String> {
        if !self
            .feature_status(SecretSourceKind::Ephemeral)
            .is_available()
        {
            return None;
        }
        self.state
            .read()
            .expect("secret store state lock poisoned")
            .ephemeral
            .get(&scoped_ephemeral_key(task_id, input_id))
            .and_then(|entry| entry.fields.get("value"))
            .cloned()
    }

    /// Whether this scope currently holds a user-typed secret.
    ///
    /// # Why anything asks
    ///
    /// Because this partition is the one piece of a run's state that cannot be
    /// handed to another process. `SecretStoreState.ephemeral` is a plain
    /// in-memory map and there is no ephemeral persistence path — this store
    /// persists the provisioned, captured and mcp_oauth partitions and nothing
    /// else. Meanwhile `resolve_invocation_secrets` substitutes placeholders at
    /// the transport and refuses the call if any are left unresolved. So a run
    /// that took a password or an OTP and then moved to another worker fails
    /// closed and asks the user again — the right failure, but under a stateless
    /// driver it becomes routine, and it reads as a run that re-prompts for the
    /// same OTP every time it is rescheduled.
    ///
    /// An execution answering `true` here is pinned to the worker holding it —
    /// see `LoopState::pin_to`. This is the acquisition signal that pin is taken
    /// on.
    ///
    /// # Why this does not consult the feature flag
    ///
    /// Because the map is the fact and the flag governs writes. Every other
    /// reader here is flag-gated — [`Self::get_ephemeral`],
    /// [`Self::get_ephemeral_scoped`] and [`Self::clear_ephemeral`] all answer
    /// as though the partition were empty when the feature is unavailable — and
    /// a *pin* must not. A false "holds nothing" releases a run to a worker that
    /// cannot resolve its placeholders, which is the failure this signal exists
    /// to prevent; a false "holds something" costs portability and nothing else.
    /// The two directions are not symmetric, so this reader is not written like
    /// the others.
    ///
    /// An earlier version of this comment justified it differently, by claiming
    /// that "a partition populated while the feature was enabled survives a
    /// later disable". That case is **not reachable**: `capabilities` is a plain
    /// field fixed at construction, and a store built with the ephemeral feature
    /// disabled refuses every [`Self::register_ephemeral`], so its map is empty
    /// for its whole life. The rule is right; the hazard it named was not real,
    /// and a comment that budgets for an unreachable case is how a later edit
    /// talks itself into "the flag is safe to read here after all".
    ///
    /// It stays unreachable only while capabilities stay immutable. Reading the
    /// map rather than the flag is what makes that a property of this function
    /// instead of a property of a field somewhere else.
    pub fn ephemeral_scope_holds_user_typed_secret(&self, task_id: &str) -> bool {
        self.state
            .read()
            .expect("secret store state lock poisoned")
            .ephemeral
            .values()
            .any(|entry| match &entry.source {
                SecretSource::Ephemeral {
                    task_id: entry_task_id,
                } => entry_task_id == task_id,
                _ => false,
            })
    }

    pub fn clear_ephemeral(&self, task_id: &str) -> usize {
        if !self
            .feature_status(SecretSourceKind::Ephemeral)
            .is_available()
        {
            return 0;
        }
        let mut guard = self
            .state
            .write()
            .expect("secret store state lock poisoned");
        let before = guard.ephemeral.len();
        guard.ephemeral.retain(|_, entry| match &entry.source {
            SecretSource::Ephemeral {
                task_id: entry_task_id,
            } => entry_task_id != task_id,
            _ => true,
        });
        before.saturating_sub(guard.ephemeral.len())
    }

    pub fn store_captured(
        &self,
        origin: &str,
        headers: HashMap<String, String>,
        cookies: Vec<CookieWithMetadata>,
        local_storage_tokens: HashMap<String, String>,
        session_storage_tokens: HashMap<String, String>,
    ) -> Result<(), SecretStoreError> {
        self.store_captured_inner(
            origin,
            headers,
            HashMap::new(),
            cookies,
            local_storage_tokens,
            session_storage_tokens,
        )?;
        self.persist_captured()
    }

    /// Store captured auth state in memory without persisting to disk.
    /// Call `flush_captured()` after a batch of inserts to persist once.
    pub fn store_captured_deferred(
        &self,
        origin: &str,
        headers: HashMap<String, String>,
        cookies: Vec<CookieWithMetadata>,
        local_storage_tokens: HashMap<String, String>,
        session_storage_tokens: HashMap<String, String>,
    ) -> Result<(), SecretStoreError> {
        self.store_captured_inner(
            origin,
            headers,
            HashMap::new(),
            cookies,
            local_storage_tokens,
            session_storage_tokens,
        )
    }

    /// Store captured auth state that includes URL query parameters in memory
    /// without persisting to disk. Call `flush_captured()` after a batch.
    pub fn store_captured_deferred_with_query_params(
        &self,
        origin: &str,
        headers: HashMap<String, String>,
        auth_query_params: HashMap<String, String>,
        cookies: Vec<CookieWithMetadata>,
        local_storage_tokens: HashMap<String, String>,
        session_storage_tokens: HashMap<String, String>,
    ) -> Result<(), SecretStoreError> {
        self.store_captured_inner(
            origin,
            headers,
            auth_query_params,
            cookies,
            local_storage_tokens,
            session_storage_tokens,
        )
    }

    /// Store browser-state-derived auth data in memory without persisting to disk.
    ///
    /// Browser cookies are treated as an authoritative snapshot for this origin:
    /// previously stored cookies absent from the latest capture are removed.
    /// Storage maps are authoritative only when provided via `Some(..)`. Passing
    /// `None` preserves the existing storage partition for that origin.
    pub fn store_captured_browser_state_deferred(
        &self,
        origin: &str,
        cookies: Vec<CookieWithMetadata>,
        local_storage_tokens: Option<HashMap<String, String>>,
        session_storage_tokens: Option<HashMap<String, String>>,
    ) -> Result<(), SecretStoreError> {
        self.ensure_feature_enabled(SecretSourceKind::Captured)?;
        let store_start = std::time::Instant::now();
        let cookie_count = cookies.len();

        // Read existing entry under a brief read lock, then drop it.
        // All expensive merge/flatten work happens lock-free.
        let existing = {
            let guard = self.state.read().expect("secret store state lock poisoned");
            guard.captured.get(origin).cloned()
        };

        let existing_headers = existing
            .as_ref()
            .map(extract_captured_headers)
            .unwrap_or_default();
        let existing_query_params = existing
            .as_ref()
            .map(extract_captured_query_params)
            .unwrap_or_default();
        let existing_local_storage = existing
            .as_ref()
            .map(extract_captured_local_storage)
            .unwrap_or_default();
        let existing_session_storage = existing
            .as_ref()
            .map(extract_captured_session_storage)
            .unwrap_or_default();
        let generation = existing
            .as_ref()
            .and_then(captured_generation)
            .unwrap_or_default()
            + 1;

        // All merge/flatten work happens without any lock held.
        let merge_start = std::time::Instant::now();
        let snapshot_cookies = merge_captured_cookies(Vec::new(), cookies);
        let merge_ms = merge_start.elapsed().as_millis();

        let merged_local_storage = local_storage_tokens.unwrap_or(existing_local_storage);
        let merged_session_storage = session_storage_tokens.unwrap_or(existing_session_storage);
        let expires_hint = existing_headers
            .values()
            .chain(existing_query_params.values())
            .chain(merged_local_storage.values())
            .chain(merged_session_storage.values())
            .filter_map(|value| detect_jwt_expiry(value.strip_prefix("Bearer ").unwrap_or(value)))
            .min();

        let flatten_start = std::time::Instant::now();
        let entry = SecretEntry {
            id: origin.to_string(),
            label: origin.to_string(),
            fields: flatten_captured_fields(
                &existing_headers,
                &existing_query_params,
                &snapshot_cookies,
                &merged_local_storage,
                &merged_session_storage,
            ),
            source: SecretSource::Captured {
                origin: origin.to_string(),
                generation,
                stale: false,
                expires_hint,
            },
            injection: InjectionTarget::Cookies(cookies_to_specs(&snapshot_cookies)),
            policy: None,
            created_at: Utc::now().timestamp(),
        };

        let flatten_ms = flatten_start.elapsed().as_millis();

        // Brief write lock: just insert + evict.
        {
            let mut guard = self
                .state
                .write()
                .expect("secret store state lock poisoned");
            guard.captured.insert(origin.to_string(), entry);
            self.evict_captured_if_needed(&mut guard);
        }
        self.captured_dirty.store(true, Ordering::Release);
        let total_ms = store_start.elapsed().as_millis();
        if total_ms > 50 {
            tracing::debug!(
                "[SECRET_STORE] store_captured_browser_state_deferred: origin={}, cookies={}, merge={}ms, flatten={}ms, total={}ms",
                origin, cookie_count, merge_ms, flatten_ms, total_ms
            );
        }
        Ok(())
    }

    /// Persist all captured entries to disk. Call after a batch of `store_captured_deferred`.
    ///
    /// Skips entirely when the in-memory state hasn't changed since the last flush.
    /// Serialization, encryption, and I/O run on a background thread so the caller
    /// is not blocked. Only one flush is in-flight at a time; concurrent calls are
    /// coalesced (the next flush will pick up the latest state).
    pub fn flush_captured(&self) -> Result<(), SecretStoreError> {
        // Nothing changed — skip.
        if !self.captured_dirty.load(Ordering::Acquire) {
            return Ok(());
        }
        // Another flush is already in-flight — it will pick up our writes.
        if self
            .flush_in_flight
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            debug!("[SECRET_STORE] flush_captured: coalesced (already in-flight)");
            return Ok(());
        }

        let flush_start = std::time::Instant::now();
        // `persist_captured` owns the dirty flag: it clears before snapshotting
        // and re-dirties on failure. The gate below is released only after that
        // has happened, so a concurrent caller cannot observe
        // dirty=false + in_flight=false and skip the retry.
        let result = self.persist_captured();
        let flush_ms = flush_start.elapsed().as_millis();

        if flush_ms > 50 {
            tracing::debug!("[SECRET_STORE] flush_captured: {}ms", flush_ms);
        }
        self.flush_in_flight.store(false, Ordering::Release);
        result
    }

    pub fn register_captured_task(
        &self,
        execution_id: Option<String>,
        handle: tokio::task::JoinHandle<()>,
    ) {
        let mut pending = self
            .pending_captured_tasks
            .lock()
            .expect("pending_captured_tasks lock poisoned");
        pending.retain(|task| !task.handle.is_finished());
        pending.push(PendingCapturedTask {
            execution_id,
            handle,
        });
    }

    pub async fn drain_captured_tasks_for_execution(&self, execution_id: &str) {
        let handles = {
            let mut pending = self
                .pending_captured_tasks
                .lock()
                .expect("pending_captured_tasks lock poisoned");
            let mut remaining = Vec::with_capacity(pending.len());
            let mut matching = Vec::new();
            for task in pending.drain(..) {
                if task.execution_id.as_deref() == Some(execution_id) {
                    matching.push(task.handle);
                } else {
                    remaining.push(task);
                }
            }
            *pending = remaining;
            matching
        };

        for handle in handles {
            if let Err(err) = handle.await {
                warn!(
                    execution_id = %execution_id,
                    error = %err,
                    "secret store: pending captured task join failed"
                );
            }
        }
    }

    pub async fn drain_all_captured_tasks(&self) {
        let handles = {
            let mut pending = self
                .pending_captured_tasks
                .lock()
                .expect("pending_captured_tasks lock poisoned");
            pending
                .drain(..)
                .map(|task| task.handle)
                .collect::<Vec<_>>()
        };

        for handle in handles {
            if let Err(err) = handle.await {
                warn!(error = %err, "secret store: pending captured task join failed");
            }
        }
    }

    pub async fn drain_captured_tasks_for_execution_and_flush(
        &self,
        execution_id: &str,
    ) -> Result<(), SecretStoreError> {
        self.drain_captured_tasks_for_execution(execution_id).await;
        if self
            .feature_status(SecretSourceKind::Captured)
            .is_available()
        {
            self.flush_captured()?;
        }
        Ok(())
    }

    pub async fn drain_all_captured_tasks_and_flush(&self) -> Result<(), SecretStoreError> {
        self.drain_all_captured_tasks().await;
        if self
            .feature_status(SecretSourceKind::Captured)
            .is_available()
        {
            self.flush_captured()?;
        }
        Ok(())
    }

    fn store_captured_inner(
        &self,
        origin: &str,
        headers: HashMap<String, String>,
        auth_query_params: HashMap<String, String>,
        cookies: Vec<CookieWithMetadata>,
        local_storage_tokens: HashMap<String, String>,
        session_storage_tokens: HashMap<String, String>,
    ) -> Result<(), SecretStoreError> {
        self.ensure_feature_enabled(SecretSourceKind::Captured)?;

        // Brief read lock to clone existing entry, then drop.
        let existing = {
            let guard = self.state.read().expect("secret store state lock poisoned");
            guard.captured.get(origin).cloned()
        };

        let existing_headers = existing
            .as_ref()
            .map(extract_captured_headers)
            .unwrap_or_default();
        let existing_query_params = existing
            .as_ref()
            .map(extract_captured_query_params)
            .unwrap_or_default();
        let existing_cookies = existing
            .as_ref()
            .map(extract_captured_cookies)
            .unwrap_or_default();
        let existing_local_storage = existing
            .as_ref()
            .map(extract_captured_local_storage)
            .unwrap_or_default();
        let existing_session_storage = existing
            .as_ref()
            .map(extract_captured_session_storage)
            .unwrap_or_default();
        let generation = existing
            .as_ref()
            .and_then(captured_generation)
            .unwrap_or_default()
            + 1;

        // All merge work happens lock-free.
        let merged_headers = merge_captured_headers(existing_headers, headers);
        let merged_query_params = merge_named_values(existing_query_params, auth_query_params);
        let merged_cookies = merge_captured_cookies(existing_cookies, cookies);
        let merged_local_storage = merge_named_values(existing_local_storage, local_storage_tokens);
        let merged_session_storage =
            merge_named_values(existing_session_storage, session_storage_tokens);
        let expires_hint = merged_headers
            .values()
            .chain(merged_query_params.values())
            .chain(merged_local_storage.values())
            .chain(merged_session_storage.values())
            .filter_map(|value| detect_jwt_expiry(value.strip_prefix("Bearer ").unwrap_or(value)))
            .min();

        let entry = SecretEntry {
            id: origin.to_string(),
            label: origin.to_string(),
            fields: flatten_captured_fields(
                &merged_headers,
                &merged_query_params,
                &merged_cookies,
                &merged_local_storage,
                &merged_session_storage,
            ),
            source: SecretSource::Captured {
                origin: origin.to_string(),
                generation,
                stale: false,
                expires_hint,
            },
            injection: InjectionTarget::Cookies(cookies_to_specs(&merged_cookies)),
            policy: None,
            created_at: Utc::now().timestamp(),
        };

        // Brief write lock: just insert + evict.
        {
            let mut guard = self
                .state
                .write()
                .expect("secret store state lock poisoned");
            guard.captured.insert(origin.to_string(), entry);
            self.evict_captured_if_needed(&mut guard);
        }
        self.captured_dirty.store(true, Ordering::Release);
        Ok(())
    }

    pub fn get_session(
        &self,
        origin: &str,
        replay_url: &str,
    ) -> Option<(SessionContext, CapturedSessionLease)> {
        if !self
            .feature_status(SecretSourceKind::Captured)
            .is_available()
        {
            return None;
        }
        let guard = self.state.read().expect("secret store state lock poisoned");
        let url = url::Url::parse(replay_url).ok()?;
        let exact_entry = guard.captured.get(origin);
        let mut exact_generation = None;
        let mut auth_headers = HashMap::new();
        let mut auth_query_params = HashMap::new();
        let mut local_storage = HashMap::new();
        let mut session_storage = HashMap::new();
        let mut applicable_cookies = Vec::new();
        let mut lease = CapturedSessionLease::default();

        if let Some(entry) = exact_entry.as_ref() {
            let SecretSource::Captured {
                generation, stale, ..
            } = &entry.source
            else {
                return None;
            };
            if *stale {
                return None;
            }

            exact_generation = Some(*generation);
            lease.push_target(origin, *generation);
            auth_headers = extract_captured_headers(entry);
            auth_query_params = extract_captured_query_params(entry);
            local_storage = extract_captured_local_storage(entry);
            session_storage = extract_captured_session_storage(entry);
            applicable_cookies = filter_cookies_for_url(&extract_captured_cookies(entry), &url);
        }

        for (candidate_origin, entry) in guard.captured.iter() {
            if candidate_origin == origin {
                continue;
            }
            let SecretSource::Captured {
                generation, stale, ..
            } = &entry.source
            else {
                continue;
            };
            if *stale {
                continue;
            }

            let fallback_cookies = filter_cookies_for_url(&extract_captured_cookies(entry), &url);
            if fallback_cookies.is_empty() {
                continue;
            }

            lease.push_target(candidate_origin, *generation);
            applicable_cookies = merge_captured_cookies(applicable_cookies, fallback_cookies);
        }

        if exact_generation.is_none() && applicable_cookies.is_empty() {
            return None;
        }

        let (cookie_header_values, cookies) = build_session_cookies(&applicable_cookies);
        Some((
            SessionContext {
                cookie_metadata: applicable_cookies,
                cookie_header_values,
                cookies,
                auth_headers,
                auth_query_params,
                local_storage,
                session_storage,
            },
            lease,
        ))
    }

    pub fn mark_stale_lease(&self, lease: &CapturedSessionLease) -> Result<bool, SecretStoreError> {
        self.ensure_feature_enabled(SecretSourceKind::Captured)?;
        if lease.stale_mark_targets.is_empty() {
            return Ok(false);
        }

        let mut guard = self
            .state
            .write()
            .expect("secret store state lock poisoned");
        let mut changed = false;

        for target in &lease.stale_mark_targets {
            let Some(entry) = guard.captured.get_mut(&target.origin) else {
                continue;
            };
            let SecretSource::Captured {
                generation, stale, ..
            } = &mut entry.source
            else {
                continue;
            };
            if *generation != target.generation || *stale {
                continue;
            }
            *stale = true;
            changed = true;
        }

        drop(guard);
        if changed {
            self.persist_captured()?;
        }
        Ok(changed)
    }

    pub fn mark_fresh_lease(&self, lease: &CapturedSessionLease) -> Result<bool, SecretStoreError> {
        self.ensure_feature_enabled(SecretSourceKind::Captured)?;
        if lease.stale_mark_targets.is_empty() {
            return Ok(false);
        }

        let mut guard = self
            .state
            .write()
            .expect("secret store state lock poisoned");
        let mut changed = false;

        for target in &lease.stale_mark_targets {
            let Some(entry) = guard.captured.get_mut(&target.origin) else {
                continue;
            };
            let SecretSource::Captured {
                generation, stale, ..
            } = &mut entry.source
            else {
                continue;
            };
            if *generation != target.generation || !*stale {
                continue;
            }
            *stale = false;
            changed = true;
        }

        drop(guard);
        if changed {
            self.persist_captured()?;
        }
        Ok(changed)
    }

    pub fn mark_stale(&self, origin: &str, generation: u64) -> Result<bool, SecretStoreError> {
        let mut lease = CapturedSessionLease::default();
        lease.push_target(origin, generation);
        self.mark_stale_lease(&lease)
    }

    pub fn mark_fresh(&self, origin: &str, generation: u64) -> Result<bool, SecretStoreError> {
        self.ensure_feature_enabled(SecretSourceKind::Captured)?;
        self.update_captured_state(origin, generation, false)
    }

    pub fn captured_status(&self, origin: &str) -> AuthStatusMetadata {
        if !self
            .feature_status(SecretSourceKind::Captured)
            .is_available()
        {
            return AuthStatusMetadata::default();
        }
        self.state
            .read()
            .expect("secret store state lock poisoned")
            .captured
            .get(origin)
            .map(auth_status_for_entry)
            .unwrap_or_default()
    }

    pub fn all_captured_statuses(&self) -> HashMap<String, AuthStatusMetadata> {
        if !self
            .feature_status(SecretSourceKind::Captured)
            .is_available()
        {
            return HashMap::new();
        }
        self.state
            .read()
            .expect("secret store state lock poisoned")
            .captured
            .iter()
            .map(|(origin, entry)| (origin.clone(), auth_status_for_entry(entry)))
            .collect()
    }

    pub fn has_captured(&self, origin: &str) -> bool {
        if !self
            .feature_status(SecretSourceKind::Captured)
            .is_available()
        {
            return false;
        }
        self.state
            .read()
            .expect("secret store state lock poisoned")
            .captured
            .contains_key(origin)
    }

    pub fn clear_captured(&self, origin: &str) -> Result<bool, SecretStoreError> {
        self.ensure_feature_enabled(SecretSourceKind::Captured)?;
        let removed = self
            .state
            .write()
            .expect("secret store state lock poisoned")
            .captured
            .remove(origin)
            .is_some();
        if removed {
            self.persist_captured()?;
        }
        Ok(removed)
    }

    pub fn clear_all_captured(&self) -> Result<(), SecretStoreError> {
        self.ensure_feature_enabled(SecretSourceKind::Captured)?;
        self.state
            .write()
            .expect("secret store state lock poisoned")
            .captured
            .clear();
        self.persist_captured()
    }

    pub fn base_dir(&self) -> &Path {
        &self.base_dir
    }

    fn update_captured_state(
        &self,
        origin: &str,
        generation: u64,
        stale_value: bool,
    ) -> Result<bool, SecretStoreError> {
        let mut guard = self
            .state
            .write()
            .expect("secret store state lock poisoned");
        let Some(entry) = guard.captured.get_mut(origin) else {
            return Ok(false);
        };
        let SecretSource::Captured {
            generation: current_generation,
            stale,
            ..
        } = &mut entry.source
        else {
            return Ok(false);
        };

        if *current_generation != generation {
            return Ok(false);
        }
        *stale = stale_value;
        drop(guard);
        self.persist_captured()?;
        Ok(true)
    }

    /// Publish the provisioned partition.
    ///
    /// Callers mutate under `state.write()` and release it before persisting, so
    /// the snapshot below is taken under `state.read()` — which is shared. The
    /// persist lock is held across *both* the snapshot and the rename, which is
    /// what makes the file monotone: a writer that observes another writer's
    /// entries cannot then be overtaken by the snapshot that predates them.
    fn persist_provisioned(&self) -> Result<(), SecretStoreError> {
        self.ensure_feature_enabled(SecretSourceKind::Provisioned)?;
        let _serialized = lock_for_persist(&self.provisioned_write_lock);
        let entries = self
            .state
            .read()
            .expect("secret store state lock poisoned")
            .provisioned
            .clone();
        self.persist_partition(&self.provisioned_file(), &entries)
    }

    /// Publish the captured partition. Ordering as in [`Self::persist_provisioned`].
    ///
    /// The dirty flag is cleared here rather than in [`Self::flush_captured`] so
    /// that every captured writer shares one discipline — `store_captured`
    /// persists directly and used to leave the flag set, costing a redundant
    /// flush per capture. Clearing *before* the snapshot is the safe order: an
    /// insert that lands after the clone re-dirties the flag and a later flush
    /// picks it up, whereas clearing afterwards would drop it.
    fn persist_captured(&self) -> Result<(), SecretStoreError> {
        self.ensure_feature_enabled(SecretSourceKind::Captured)?;
        let _serialized = lock_for_persist(&self.captured_write_lock);
        self.captured_dirty.store(false, Ordering::Release);
        let entries = self
            .state
            .read()
            .expect("secret store state lock poisoned")
            .captured
            .clone();
        let result = self.persist_partition(&self.captured_file(), &entries);
        if result.is_err() {
            // Re-dirty so a subsequent flush retries.
            self.captured_dirty.store(true, Ordering::Release);
        }
        result
    }

    fn persist_mcp_oauth_partition(
        &self,
        records: &HashMap<String, McpOAuthVaultRecord>,
    ) -> Result<(), McpOAuthPersistFailure> {
        self.ensure_feature_enabled(SecretSourceKind::Provisioned)
            .map_err(|source| McpOAuthPersistFailure {
                source,
                commit_state_unknown: false,
            })?;
        validate_mcp_oauth_collection(records).map_err(|source| McpOAuthPersistFailure {
            source,
            commit_state_unknown: false,
        })?;

        let encoded_records = records
            .iter()
            .map(|(record_id, record)| {
                (record_id.clone(), BASE64_STANDARD.encode(record.as_bytes()))
            })
            .collect();
        let payload = PersistedMcpOAuthPartition {
            version: MCP_OAUTH_VAULT_VERSION,
            records: encoded_records,
        };
        let json = Zeroizing::new(serde_json::to_vec(&payload).map_err(|error| {
            McpOAuthPersistFailure {
                source: error.into(),
                commit_state_unknown: false,
            }
        })?);
        let key = Zeroizing::new(self.key_provider.get_or_create_key().map_err(|error| {
            McpOAuthPersistFailure {
                source: error.into(),
                commit_state_unknown: false,
            }
        })?);
        let encrypted = encrypt(&key, json.as_slice()).map_err(|error| McpOAuthPersistFailure {
            source: error.into(),
            commit_state_unknown: false,
        })?;
        if encrypted.len() as u64 > MAX_MCP_OAUTH_FILE_BYTES {
            return Err(McpOAuthPersistFailure {
                source: SecretStoreError::McpOAuthCapacity,
                commit_state_unknown: false,
            });
        }

        std::fs::create_dir_all(&self.base_dir).map_err(|error| McpOAuthPersistFailure {
            source: error.into(),
            commit_state_unknown: false,
        })?;
        restrict_vault_dir(&self.base_dir).map_err(|error| McpOAuthPersistFailure {
            source: error.into(),
            commit_state_unknown: false,
        })?;
        let path = self.mcp_oauth_file();
        // A unique temp per writer. This partition was the last user of a fixed
        // `.tmp` name: two concurrent persists shared one temp file, so the
        // rename could publish an interleaved mixture of both.
        //
        // This stays hand-rolled rather than using the shared durable writer
        // because the error contract is richer than `io::Result`: every failure
        // up to and including the rename is `commit_state_unknown: false` (the
        // old file is intact, in-memory state may roll back), while a post-rename
        // directory-sync failure is `commit_state_unknown: true` (the caller must
        // fail closed on the new state). The helper collapses that distinction.
        let tmp_path = path.with_file_name(format!(
            "{}.{}.tmp",
            path.file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("mcp_oauth"),
            uuid::Uuid::new_v4().simple()
        ));
        let written = (|| {
            // Created 0600 so the rename publishes an owner-only file.
            let mut file = private_file_options()
                .create_new(true)
                .write(true)
                .open(&tmp_path)?;
            file.write_all(&encrypted)?;
            file.sync_all()?;
            drop(file);
            std::fs::rename(&tmp_path, &path)
        })();
        if let Err(error) = written {
            // A temp that cannot be cleaned holds an encrypted vault snapshot
            // under a unique name nothing will reuse or sweep — say so.
            // NotFound just means the write failed before creating it.
            if let Err(cleanup) = std::fs::remove_file(&tmp_path) {
                if cleanup.kind() != std::io::ErrorKind::NotFound {
                    warn!(
                        tmp_path = %tmp_path.display(),
                        error = %cleanup,
                        "failed to remove MCP-OAuth staging temp after a failed persist"
                    );
                }
            }
            return Err(McpOAuthPersistFailure {
                source: error.into(),
                commit_state_unknown: false,
            });
        }

        // After rename, a directory-sync failure makes crash durability ambiguous. The
        // caller must retain the new in-memory state and fail closed rather than rolling
        // back to a value that may no longer match disk.
        std::fs::File::open(&self.base_dir)
            .and_then(|directory| directory.sync_all())
            .map_err(|error| McpOAuthPersistFailure {
                source: error.into(),
                commit_state_unknown: true,
            })?;
        Ok(())
    }

    /// Encrypt a partition snapshot and publish it durably.
    ///
    /// Serialization runs through `Zeroizing`: between `to_vec` and `encrypt`
    /// the plaintext of every provider key in the scope is sitting in a heap
    /// buffer, and the MCP sibling already treats that window as worth closing.
    fn persist_partition(
        &self,
        path: &Path,
        entries: &HashMap<String, SecretEntry>,
    ) -> Result<(), SecretStoreError> {
        let payload = PersistedPartition {
            version: 1,
            entries: entries.clone(),
        };
        let json = Zeroizing::new(serde_json::to_vec(&payload)?);
        let key = Zeroizing::new(self.key_provider.get_or_create_key()?);
        let encrypted = encrypt(&key, json.as_slice())?;

        std::fs::create_dir_all(&self.base_dir)?;
        restrict_vault_dir(&self.base_dir)?;
        write_file_durably(path, &encrypted)?;
        restrict_vault_file(path)?;
        Ok(())
    }

    fn load_from_disk(&self) -> Result<(), SecretStoreError> {
        let provisioned = if self
            .feature_status(SecretSourceKind::Provisioned)
            .is_available()
        {
            self.load_partition(&self.provisioned_file())?
        } else {
            HashMap::new()
        };
        let captured = if self
            .feature_status(SecretSourceKind::Captured)
            .is_available()
        {
            self.load_partition(&self.captured_file())?
        } else {
            HashMap::new()
        };
        let mcp_oauth = if self
            .feature_status(SecretSourceKind::Provisioned)
            .is_available()
        {
            match self.load_mcp_oauth_partition() {
                Ok(records) => records,
                Err(error) if is_mcp_oauth_corruption(&error) => {
                    warn!("secret store: isolated corrupt MCP OAuth vault: {}", error);
                    HashMap::new()
                },
                Err(error) => return Err(error),
            }
        } else {
            HashMap::new()
        };
        let mut guard = self
            .state
            .write()
            .expect("secret store state lock poisoned");
        guard.provisioned = provisioned;
        guard.captured = captured;
        guard.mcp_oauth = mcp_oauth;
        guard.mcp_oauth_status = McpOAuthPartitionStatus::Ready;
        guard.provisioned_status = SecretPartitionStatus::Loaded;
        guard.captured_status = SecretPartitionStatus::Loaded;
        Ok(())
    }

    /// Load every partition, keeping the store usable when one of them is not.
    ///
    /// A failed partition is left **empty** and its
    /// [`SecretPartitionStatus`] set to `Unavailable`; the first error is still
    /// returned so a caller that can refuse — see
    /// [`SecretStoreResolver::resolve_for_scope`] — still gets the chance. The
    /// argument for empty over refusing is on [`SecretPartitionStatus`]; the
    /// argument for recording it is that empty is otherwise indistinguishable
    /// from a scope that never provisioned anything.
    fn load_from_disk_lenient(&self) -> Result<(), SecretStoreError> {
        let mut first_error = None;
        let mut mcp_oauth_status = McpOAuthPartitionStatus::Ready;
        let mut provisioned_status = SecretPartitionStatus::Loaded;
        let mut captured_status = SecretPartitionStatus::Loaded;
        let provisioned = if self
            .feature_status(SecretSourceKind::Provisioned)
            .is_available()
        {
            match self.load_partition(&self.provisioned_file()) {
                Ok(entries) => entries,
                Err(err) => {
                    provisioned_status = SecretPartitionStatus::Unavailable;
                    first_error = Some(err);
                    HashMap::new()
                },
            }
        } else {
            HashMap::new()
        };
        let captured = if self
            .feature_status(SecretSourceKind::Captured)
            .is_available()
        {
            match self.load_partition(&self.captured_file()) {
                Ok(entries) => entries,
                Err(err) => {
                    captured_status = SecretPartitionStatus::Unavailable;
                    if first_error.is_none() {
                        first_error = Some(err);
                    }
                    HashMap::new()
                },
            }
        } else {
            HashMap::new()
        };
        let mcp_oauth = if self
            .feature_status(SecretSourceKind::Provisioned)
            .is_available()
        {
            match self.load_mcp_oauth_partition() {
                Ok(records) => records,
                Err(error) if is_mcp_oauth_corruption(&error) => {
                    warn!("secret store: isolated corrupt MCP OAuth vault: {}", error);
                    HashMap::new()
                },
                Err(error) => {
                    mcp_oauth_status = McpOAuthPartitionStatus::Unavailable;
                    if first_error.is_none() {
                        first_error = Some(error);
                    }
                    HashMap::new()
                },
            }
        } else {
            HashMap::new()
        };

        let mut guard = self
            .state
            .write()
            .expect("secret store state lock poisoned");
        guard.provisioned = provisioned;
        guard.captured = captured;
        guard.mcp_oauth = mcp_oauth;
        guard.mcp_oauth_status = mcp_oauth_status;
        guard.provisioned_status = provisioned_status;
        guard.captured_status = captured_status;

        match first_error {
            Some(err) => Err(err),
            None => Ok(()),
        }
    }

    fn load_partition(
        &self,
        path: &Path,
    ) -> Result<HashMap<String, SecretEntry>, SecretStoreError> {
        if !path.exists() {
            return Ok(HashMap::new());
        }

        let data = std::fs::read(path)?;
        let key = self.key_provider.get_or_create_key()?;
        let decrypted = match decrypt(&key, &data) {
            Ok(bytes) => bytes,
            // Only `Corrupted` says the bytes are wrong. Anything else is the
            // cipher or the key failing us, and moving an intact vault aside
            // over a fault that a later attempt could get past is how a
            // recoverable outage becomes a permanent one. The MCP sibling
            // already draws the line here.
            Err(err @ SecretEncryptionError::Corrupted(_)) => {
                self.quarantine_partition(path);
                return Err(err.into());
            },
            Err(err) => return Err(err.into()),
        };
        let payload = match serde_json::from_slice::<PersistedPartition>(&decrypted) {
            Ok(payload) => payload,
            Err(err) => {
                self.quarantine_partition(path);
                return Err(err.into());
            },
        };
        Ok(payload.entries)
    }

    fn load_mcp_oauth_partition(
        &self,
    ) -> Result<HashMap<String, McpOAuthVaultRecord>, SecretStoreError> {
        let path = self.mcp_oauth_file();
        if !path.exists() {
            return Ok(HashMap::new());
        }
        let metadata = std::fs::metadata(&path)?;
        if metadata.len() > MAX_MCP_OAUTH_FILE_BYTES {
            self.quarantine_mcp_oauth_partition(&path)?;
            return Err(SecretStoreError::McpOAuthCapacity);
        }
        let data = std::fs::read(&path)?;
        let key = Zeroizing::new(self.key_provider.get_or_create_key()?);
        let decrypted = match decrypt(&key, &data) {
            Ok(bytes) => Zeroizing::new(bytes),
            Err(error @ SecretEncryptionError::Corrupted(_)) => {
                self.quarantine_mcp_oauth_partition(&path)?;
                return Err(error.into());
            },
            Err(error) => return Err(error.into()),
        };
        let payload =
            match serde_json::from_slice::<PersistedMcpOAuthPartition>(decrypted.as_slice()) {
                Ok(payload) => payload,
                Err(error) => {
                    self.quarantine_mcp_oauth_partition(&path)?;
                    return Err(error.into());
                },
            };
        if payload.version != MCP_OAUTH_VAULT_VERSION
            || payload.records.len() > MAX_MCP_OAUTH_RECORDS
        {
            self.quarantine_mcp_oauth_partition(&path)?;
            return Err(SecretStoreError::McpOAuthInvalidRecord);
        }

        let mut total_bytes = 0usize;
        let mut records = HashMap::with_capacity(payload.records.len());
        for (record_id, encoded) in &payload.records {
            if validate_mcp_oauth_record_id(record_id).is_err()
                || encoded.len() > MAX_MCP_OAUTH_ENCODED_RECORD_BYTES
            {
                self.quarantine_mcp_oauth_partition(&path)?;
                return Err(SecretStoreError::McpOAuthInvalidRecord);
            }
            let document = match BASE64_STANDARD.decode(encoded) {
                Ok(document) => document,
                Err(_) => {
                    self.quarantine_mcp_oauth_partition(&path)?;
                    return Err(SecretStoreError::McpOAuthInvalidRecord);
                },
            };
            if validate_mcp_oauth_document(&document).is_err() {
                self.quarantine_mcp_oauth_partition(&path)?;
                return Err(SecretStoreError::McpOAuthInvalidRecord);
            }
            let Some(next_total) = total_bytes.checked_add(document.len()) else {
                self.quarantine_mcp_oauth_partition(&path)?;
                return Err(SecretStoreError::McpOAuthCapacity);
            };
            total_bytes = next_total;
            if total_bytes > MAX_MCP_OAUTH_TOTAL_BYTES {
                self.quarantine_mcp_oauth_partition(&path)?;
                return Err(SecretStoreError::McpOAuthCapacity);
            }
            records.insert(record_id.clone(), McpOAuthVaultRecord { document });
        }
        Ok(records)
    }

    fn quarantine_mcp_oauth_partition(&self, path: &Path) -> Result<(), SecretStoreError> {
        let corrupt_path = path.with_extension(format!("corrupt-{}", uuid::Uuid::new_v4()));
        std::fs::rename(path, corrupt_path)?;
        Ok(())
    }

    /// Move an unreadable partition aside so the store can carry on without it.
    ///
    /// The name is unique per event, matching
    /// [`Self::quarantine_mcp_oauth_partition`]. A fixed `.corrupt` name means
    /// the second occurrence overwrites the first, and the first is the copy
    /// that still holds the entries — one repeat of a transient key-provider
    /// fault would destroy the only surviving ciphertext.
    ///
    /// Logged at `warn` with the destination on success too, because the
    /// partition is about to read as empty and the operator's next question is
    /// where the old one went.
    fn quarantine_partition(&self, path: &Path) {
        let corrupt_path = path.with_extension(format!("corrupt-{}", uuid::Uuid::new_v4()));
        match std::fs::rename(path, &corrupt_path) {
            Ok(()) => warn!(
                "secret store: quarantined unreadable vault {:?} to {:?}; the partition now reads as empty",
                path, corrupt_path
            ),
            Err(err) => warn!(
                "secret store: failed to quarantine unreadable vault {:?}: {}",
                path, err
            ),
        }
    }

    fn audit_path(&self) -> PathBuf {
        self.base_dir.join(SECRET_AUDIT_FILENAME)
    }

    fn append_audit_event<Receipt: AuditReceipt>(&self, event: &AuditEvent<Receipt>) -> Result<(), SecretStoreError> {
        let _guard = self
            .audit_write_lock
            .lock()
            .map_err(|_| SecretStoreError::AuditUnavailable)?;
        std::fs::create_dir_all(&self.base_dir)?;
        restrict_vault_dir(&self.base_dir)?;
        let mut encoded = serde_json::to_vec(event)?;
        encoded.push(b'\n');
        let path = self.audit_path();
        let mut file = private_file_options()
            .create(true)
            .append(true)
            .open(&path)?;
        // The journal is plaintext metadata; tighten a file that predates the
        // owner-only rule on its next append.
        restrict_vault_file(&path)?;
        file.write_all(&encoded)?;
        Ok(())
    }

    fn provisioned_file(&self) -> PathBuf {
        self.base_dir.join("provisioned_secrets.vault")
    }

    fn captured_file(&self) -> PathBuf {
        self.base_dir.join("captured_secrets.vault")
    }

    fn mcp_oauth_file(&self) -> PathBuf {
        self.base_dir.join(MCP_OAUTH_VAULT_FILENAME)
    }

    fn evict_captured_if_needed(&self, state: &mut SecretStoreState) {
        let max_origins = self
            .config
            .read()
            .expect("secret store config lock poisoned")
            .captured_max_origins;
        while state.captured.len() > max_origins {
            let oldest = state
                .captured
                .iter()
                .min_by_key(|(_, entry)| entry.created_at)
                .map(|(origin, _)| origin.clone());
            let Some(origin) = oldest else {
                break;
            };
            debug!("secret store: evicting captured origin {}", origin);
            state.captured.remove(&origin);
        }
    }
}

fn validate_mcp_oauth_record_id(record_id: &str) -> Result<(), SecretStoreError> {
    let digest = record_id
        .strip_prefix(MCP_OAUTH_CREDENTIAL_PREFIX)
        .or_else(|| record_id.strip_prefix(MCP_OAUTH_STATE_PREFIX))
        .or_else(|| record_id.strip_prefix(MCP_OAUTH_PENDING_PREFIX))
        .ok_or(SecretStoreError::McpOAuthInvalidRecord)?;
    if digest.len() != MCP_OAUTH_DIGEST_HEX_BYTES
        || !digest
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(SecretStoreError::McpOAuthInvalidRecord);
    }
    Ok(())
}

fn ensure_mcp_oauth_partition_ready(state: &SecretStoreState) -> Result<(), SecretStoreError> {
    match state.mcp_oauth_status {
        McpOAuthPartitionStatus::Ready => Ok(()),
        McpOAuthPartitionStatus::Unavailable => Err(SecretStoreError::McpOAuthUnavailable),
        McpOAuthPartitionStatus::CommitUnknown => Err(SecretStoreError::McpOAuthCommitStateUnknown),
    }
}

/// Whether a partition load failed in a way [`SecretStore::quarantine_partition`]
/// has already acted on.
///
/// The distinction is whether retrying can help. Corrupt bytes have been moved
/// aside, so the next load finds nothing and reports a scope that never
/// provisioned anything — the failure has to be carried forward instead. A key
/// backend that was momentarily unreachable leaves the vault untouched on disk,
/// and that one must stay retryable.
fn is_partition_corruption(error: &SecretStoreError) -> bool {
    matches!(
        error,
        SecretStoreError::Serde(_)
            | SecretStoreError::Encryption(SecretEncryptionError::Corrupted(_))
    )
}

fn is_mcp_oauth_corruption(error: &SecretStoreError) -> bool {
    matches!(
        error,
        SecretStoreError::McpOAuthInvalidRecord
            | SecretStoreError::McpOAuthCapacity
            | SecretStoreError::Serde(_)
            | SecretStoreError::Encryption(SecretEncryptionError::Corrupted(_))
    )
}

fn validate_mcp_oauth_document(document: &[u8]) -> Result<(), SecretStoreError> {
    if document.is_empty() || document.len() > MAX_MCP_OAUTH_RECORD_BYTES {
        return Err(SecretStoreError::McpOAuthInvalidRecord);
    }
    Ok(())
}

fn validate_mcp_oauth_capacity(
    records: &HashMap<String, McpOAuthVaultRecord>,
    record_id: &str,
    replacement_bytes: usize,
) -> Result<(), SecretStoreError> {
    let record_count = records.len() + usize::from(!records.contains_key(record_id));
    if record_count > MAX_MCP_OAUTH_RECORDS {
        return Err(SecretStoreError::McpOAuthCapacity);
    }
    let replaced_bytes = records
        .get(record_id)
        .map_or(0, |record| record.as_bytes().len());
    let current_total = records.values().try_fold(0usize, |total, record| {
        total.checked_add(record.as_bytes().len())
    });
    let Some(next_total) = current_total
        .and_then(|total| total.checked_sub(replaced_bytes))
        .and_then(|total| total.checked_add(replacement_bytes))
    else {
        return Err(SecretStoreError::McpOAuthCapacity);
    };
    if next_total > MAX_MCP_OAUTH_TOTAL_BYTES {
        return Err(SecretStoreError::McpOAuthCapacity);
    }
    Ok(())
}

fn validate_mcp_oauth_collection(
    records: &HashMap<String, McpOAuthVaultRecord>,
) -> Result<(), SecretStoreError> {
    if records.len() > MAX_MCP_OAUTH_RECORDS {
        return Err(SecretStoreError::McpOAuthCapacity);
    }
    let mut total = 0usize;
    for (record_id, record) in records {
        validate_mcp_oauth_record_id(record_id)?;
        validate_mcp_oauth_document(record.as_bytes())?;
        total = total
            .checked_add(record.as_bytes().len())
            .ok_or(SecretStoreError::McpOAuthCapacity)?;
        if total > MAX_MCP_OAUTH_TOTAL_BYTES {
            return Err(SecretStoreError::McpOAuthCapacity);
        }
    }
    Ok(())
}

impl<Layout: SecretScopeLayout> SecretStoreResolver<Layout> {
    pub fn new_with_capabilities(
        key_provider: Box<dyn MasterKeyProvider>,
        base_root: PathBuf,
        capabilities: SecretRuntimeCapabilities,
    ) -> Self {
        Self {
            workspace_layout: Layout::from_base_root(&base_root),
            key_provider: Arc::<dyn MasterKeyProvider>::from(key_provider),
            capabilities,
            captured_max_origins: Arc::new(AtomicUsize::new(
                StoreConfig::default().captured_max_origins,
            )),
            stores: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn resolve_for_scope(
        &self,
        principal: &str,
        workspace: &str,
    ) -> Result<Arc<SecretStore>, SecretStoreError> {
        let key = (principal.to_string(), workspace.to_string());
        let slot = {
            let mut stores = self
                .stores
                .lock()
                .expect("secret store resolver lock poisoned");
            Arc::clone(
                stores
                    .entry(key)
                    .or_insert_with(|| Arc::new(ScopeStoreSlot::default())),
            )
        };
        let mut resolved = slot
            .store
            .lock()
            .expect("scoped secret store slot lock poisoned");
        if let Some(store) = resolved.as_ref() {
            return Ok(Arc::clone(store));
        }

        let scoped_root = self.workspace_layout.secrets_root(principal, workspace);
        let store = Arc::new(SecretStore::new_empty_with_capabilities(
            Arc::clone(&self.key_provider),
            scoped_root,
            self.capabilities.clone(),
        ));
        store.set_captured_max_origins(self.captured_max_origins.load(Ordering::SeqCst));
        if let Err(error) = store.load_from_disk_lenient() {
            // Dropping the store on any failure used to make corruption read as
            // "one 500, then silently empty forever": the quarantine already
            // moved the file, so the next resolve builds a store that loads
            // nothing and reports the scope as merely unprovisioned. Keep the
            // store that knows better — its
            // [`SecretStore::partition_status`] says `Unavailable` — and keep
            // propagating everything a later attempt could still recover from.
            if !is_partition_corruption(&error) {
                return Err(error);
            }
            warn!(
                "secret store: scope '{}/{}' resolved with a quarantined partition: {}",
                principal, workspace, error
            );
        }
        *resolved = Some(Arc::clone(&store));
        Ok(store)
    }

    pub fn workspace_layout(&self) -> &Layout {
        &self.workspace_layout
    }

    pub fn runtime_capabilities(&self) -> &SecretRuntimeCapabilities {
        &self.capabilities
    }

    pub fn set_captured_max_origins(&self, value: usize) {
        let value = value.max(1);
        self.captured_max_origins.store(value, Ordering::SeqCst);
        for store in self.resolved_stores() {
            store.set_captured_max_origins(value);
        }
    }

    pub async fn drain_all_captured_tasks_and_flush(&self) -> Result<(), SecretStoreError> {
        for store in self.resolved_stores() {
            store.drain_all_captured_tasks_and_flush().await?;
        }
        Ok(())
    }

    pub async fn drain_captured_tasks_for_execution_and_flush(
        &self,
        execution_id: &str,
    ) -> Result<(), SecretStoreError> {
        for store in self.resolved_stores() {
            store
                .drain_captured_tasks_for_execution_and_flush(execution_id)
                .await?;
        }
        Ok(())
    }

    fn resolved_stores(&self) -> Vec<Arc<SecretStore>> {
        let slots = self
            .stores
            .lock()
            .expect("secret store resolver lock poisoned")
            .values()
            .cloned()
            .collect::<Vec<_>>();
        slots
            .into_iter()
            .filter_map(|slot| {
                slot.store
                    .lock()
                    .expect("scoped secret store slot lock poisoned")
                    .clone()
            })
            .collect()
    }

    #[cfg(any(test, feature = "test-fixtures"))]
    pub fn seed_store_for_scope(&self, principal: &str, workspace: &str, store: Arc<SecretStore>) {
        let slot = {
            let mut stores = self
                .stores
                .lock()
                .expect("secret store resolver lock poisoned");
            Arc::clone(
                stores
                    .entry((principal.to_string(), workspace.to_string()))
                    .or_insert_with(|| Arc::new(ScopeStoreSlot::default())),
            )
        };
        *slot
            .store
            .lock()
            .expect("scoped secret store slot lock poisoned") = Some(store);
    }
}

fn auth_status_for_entry(entry: &SecretEntry) -> AuthStatusMetadata {
    let is_stale = matches!(entry.source, SecretSource::Captured { stale: true, .. });
    AuthStatusMetadata {
        has_auth: true,
        is_stale,
        has_cookies: entry.fields.keys().any(|key| key.starts_with("cookie:")),
        has_headers: entry.fields.keys().any(|key| key.starts_with("header:")),
        has_query_params: entry
            .fields
            .keys()
            .any(|key| key.starts_with("query_param:")),
        has_storage: entry
            .fields
            .keys()
            .any(|key| key.starts_with("local_storage:") || key.starts_with("session_storage:")),
    }
}

fn captured_generation(entry: &SecretEntry) -> Option<u64> {
    match &entry.source {
        SecretSource::Captured { generation, .. } => Some(*generation),
        _ => None,
    }
}

fn extract_captured_headers(entry: &SecretEntry) -> HashMap<String, String> {
    extract_prefixed_fields(entry, "header:")
}

fn extract_captured_query_params(entry: &SecretEntry) -> HashMap<String, String> {
    extract_prefixed_fields(entry, "query_param:")
}

fn extract_captured_local_storage(entry: &SecretEntry) -> HashMap<String, String> {
    extract_prefixed_fields(entry, "local_storage:")
}

fn extract_captured_session_storage(entry: &SecretEntry) -> HashMap<String, String> {
    extract_prefixed_fields(entry, "session_storage:")
}

fn extract_prefixed_fields(entry: &SecretEntry, prefix: &str) -> HashMap<String, String> {
    entry
        .fields
        .iter()
        .filter_map(|(key, value)| {
            key.strip_prefix(prefix)
                .map(|stripped| (stripped.to_string(), value.clone()))
        })
        .collect()
}

fn extract_captured_cookies(entry: &SecretEntry) -> Vec<CookieWithMetadata> {
    let InjectionTarget::Cookies(specs) = &entry.injection else {
        return Vec::new();
    };
    specs
        .iter()
        .enumerate()
        .filter_map(|(idx, spec)| {
            cookie_field_value(&entry.fields, idx, &spec.name).map(|value| CookieWithMetadata {
                name: spec.name.clone(),
                value: value.to_string(),
                domain: spec.domain.clone(),
                path: spec.path.clone(),
                secure: spec.secure,
                http_only: spec.http_only,
                same_site: spec.same_site.clone().unwrap_or(SameSite::Lax),
                expires: spec.expires,
            })
        })
        .collect()
}

fn flatten_captured_fields(
    headers: &HashMap<String, String>,
    auth_query_params: &HashMap<String, String>,
    cookies: &[CookieWithMetadata],
    local_storage_tokens: &HashMap<String, String>,
    session_storage_tokens: &HashMap<String, String>,
) -> HashMap<String, String> {
    let mut fields = HashMap::new();
    for (name, value) in headers {
        fields.insert(format!("header:{name}"), value.clone());
    }
    for (name, value) in auth_query_params {
        fields.insert(format!("query_param:{name}"), value.clone());
    }
    for (idx, cookie) in cookies.iter().enumerate() {
        fields.insert(
            format!("cookie:{idx}:{}", cookie.name),
            cookie.value.clone(),
        );
    }
    for (name, value) in local_storage_tokens {
        fields.insert(format!("local_storage:{name}"), value.clone());
    }
    for (name, value) in session_storage_tokens {
        fields.insert(format!("session_storage:{name}"), value.clone());
    }
    fields
}

fn merge_captured_headers(
    mut existing: HashMap<String, String>,
    incoming: HashMap<String, String>,
) -> HashMap<String, String> {
    for (key, value) in incoming {
        if let Some(existing_key) = existing
            .keys()
            .find(|existing_key| existing_key.eq_ignore_ascii_case(&key))
            .cloned()
        {
            existing.remove(&existing_key);
        }
        existing.insert(key, value);
    }
    existing
}

fn merge_captured_cookies(
    mut existing: Vec<CookieWithMetadata>,
    incoming: Vec<CookieWithMetadata>,
) -> Vec<CookieWithMetadata> {
    for cookie in incoming {
        if let Some(idx) = existing.iter().position(|candidate| {
            candidate.name == cookie.name
                && candidate.domain == cookie.domain
                && candidate.path == cookie.path
        }) {
            existing[idx] = cookie;
        } else {
            existing.push(cookie);
        }
    }
    existing
}

fn merge_named_values(
    mut existing: HashMap<String, String>,
    incoming: HashMap<String, String>,
) -> HashMap<String, String> {
    existing.extend(incoming);
    existing
}

fn build_session_cookies(
    cookies: &[CookieWithMetadata],
) -> (Vec<SessionCookie>, HashMap<String, String>) {
    let mut cookie_header_values = Vec::with_capacity(cookies.len());
    let mut cookie_index = HashMap::new();

    for cookie in cookies {
        cookie_index
            .entry(cookie.name.clone())
            .or_insert_with(|| cookie.value.clone());
        cookie_header_values.push(SessionCookie {
            name: cookie.name.clone(),
            value: cookie.value.clone(),
        });
    }

    (cookie_header_values, cookie_index)
}

fn scoped_ephemeral_key(scope_id: &str, input_id: &str) -> String {
    format!("{scope_id}::{input_id}")
}

fn cookies_to_specs(cookies: &[CookieWithMetadata]) -> Vec<CookieSpec> {
    cookies
        .iter()
        .map(|cookie| CookieSpec {
            name: cookie.name.clone(),
            domain: cookie.domain.clone(),
            path: cookie.path.clone(),
            secure: cookie.secure,
            http_only: cookie.http_only,
            same_site: Some(cookie.same_site.clone()),
            expires: cookie.expires,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Barrier};
    use std::thread;

    use super::*;
    use crate::InMemoryKeyProvider;
    use crate::MasterKeyProvider;

    // The portable suite owns disposable fixture paths, never a host runtime root.
    #[derive(Clone)]
    struct FixtureScopeLayout(PathBuf);
    impl SecretScopeLayout for FixtureScopeLayout {
        fn from_base_root(base_root: &Path) -> Self {
            Self(base_root.to_path_buf())
        }
        fn secrets_root(&self, principal: &str, workspace: &str) -> PathBuf {
            self.0.join("scopes").join(principal).join(workspace).join("secrets")
        }
    }
    type SecretStoreResolver = super::SecretStoreResolver<FixtureScopeLayout>;

    struct FixedKeyProvider;

    impl MasterKeyProvider for FixedKeyProvider {
        fn get_or_create_key(&self) -> Result<[u8; 32], SecretEncryptionError> {
            Ok([9u8; 32])
        }

        fn delete_key(&self) -> Result<(), SecretEncryptionError> {
            Ok(())
        }

        fn provider_name(&self) -> &str {
            "fixed"
        }
    }

    struct UnavailableKeyProvider;

    impl MasterKeyProvider for UnavailableKeyProvider {
        fn get_or_create_key(&self) -> Result<[u8; 32], SecretEncryptionError> {
            Err(SecretEncryptionError::Keychain(
                "test key backend unavailable".to_owned(),
            ))
        }

        fn delete_key(&self) -> Result<(), SecretEncryptionError> {
            Ok(())
        }

        fn provider_name(&self) -> &str {
            "unavailable-test-provider"
        }
    }

    fn temp_store_path() -> PathBuf {
        std::env::temp_dir().join(format!("codex-secret-store-{}", uuid::Uuid::new_v4()))
    }

    fn store_with_path(path: PathBuf) -> SecretStore {
        SecretStore::new_empty(Box::new(InMemoryKeyProvider::new()), path)
    }

    fn disabled_store_with_path(path: PathBuf) -> SecretStore {
        SecretStore::new_empty_with_capabilities(
            Arc::new(InMemoryKeyProvider::new()),
            path,
            SecretRuntimeCapabilities::without_durable_storage(
                "macos_keychain",
                "OS keychain backend unavailable",
            ),
        )
    }

    fn mcp_credential_record_id(index: usize) -> String {
        format!("{MCP_OAUTH_CREDENTIAL_PREFIX}{index:064x}")
    }

    fn mcp_state_record_id(index: usize) -> String {
        format!("{MCP_OAUTH_STATE_PREFIX}{index:064x}")
    }

    fn mcp_pending_record_id(index: usize) -> String {
        format!("{MCP_OAUTH_PENDING_PREFIX}{index:064x}")
    }

    #[test]
    fn mcp_oauth_partition_survives_restart_and_take_is_durable() {
        let temp = tempfile::tempdir().expect("temporary OAuth vault root");
        let path = temp.path().join("secrets");
        let credential_id = mcp_credential_record_id(1);
        let state_id = mcp_state_record_id(2);
        let deleted_id = mcp_credential_record_id(8);

        let store = SecretStore::new_empty(Box::new(FixedKeyProvider), path.clone());
        store
            .write_mcp_oauth(&credential_id, b"credential-document", false)
            .expect("persist credentials");
        store
            .write_mcp_oauth(&state_id, b"authorization-state", true)
            .expect("persist authorization state");
        store
            .write_mcp_oauth(&deleted_id, b"delete-me", false)
            .expect("persist soon-deleted credentials");
        store
            .delete_mcp_oauth(&deleted_id)
            .expect("delete credentials");
        store
            .delete_mcp_oauth(&deleted_id)
            .expect("repeat idempotent delete");
        drop(store);

        let restarted =
            SecretStore::new(Box::new(FixedKeyProvider), path.clone()).expect("reload OAuth vault");
        assert_eq!(
            restarted
                .read_mcp_oauth(&credential_id)
                .expect("read credentials"),
            Some(b"credential-document".to_vec())
        );
        assert_eq!(
            restarted.take_mcp_oauth(&state_id).expect("consume state"),
            Some(b"authorization-state".to_vec())
        );
        assert!(restarted
            .read_mcp_oauth(&deleted_id)
            .expect("read deleted credentials")
            .is_none());
        drop(restarted);

        let after_consume = SecretStore::new(Box::new(FixedKeyProvider), path)
            .expect("reload consumed OAuth vault");
        assert!(after_consume
            .read_mcp_oauth(&state_id)
            .expect("read consumed state")
            .is_none());
        assert!(after_consume
            .read_mcp_oauth(&credential_id)
            .expect("read retained credentials")
            .is_some());
    }

    #[test]
    fn mcp_oauth_create_only_conflicts_without_overwriting_and_replace_updates() {
        let temp = tempfile::tempdir().expect("temporary OAuth vault root");
        let store = SecretStore::new_empty(Box::new(FixedKeyProvider), temp.path().join("secrets"));
        let record_id = mcp_state_record_id(3);
        store
            .write_mcp_oauth(&record_id, b"first", true)
            .expect("create record");

        assert!(matches!(
            store.write_mcp_oauth(&record_id, b"collision", true),
            Err(SecretStoreError::McpOAuthConflict)
        ));
        assert_eq!(
            store.read_mcp_oauth(&record_id).expect("read record"),
            Some(b"first".to_vec())
        );

        store
            .write_mcp_oauth(&record_id, b"replacement", false)
            .expect("replace record");
        assert_eq!(
            store.read_mcp_oauth(&record_id).expect("read record"),
            Some(b"replacement".to_vec())
        );
    }

    #[test]
    fn mcp_oauth_concurrent_take_has_exactly_one_winner() {
        const CONTENDERS: usize = 16;

        let temp = tempfile::tempdir().expect("temporary OAuth vault root");
        let store = Arc::new(SecretStore::new_empty(
            Box::new(FixedKeyProvider),
            temp.path().join("secrets"),
        ));
        let record_id = mcp_state_record_id(4);
        store
            .write_mcp_oauth(&record_id, b"one-shot-state", true)
            .expect("create one-shot state");
        let barrier = Arc::new(Barrier::new(CONTENDERS + 1));
        let mut joins = Vec::with_capacity(CONTENDERS);
        for _ in 0..CONTENDERS {
            let store = Arc::clone(&store);
            let barrier = Arc::clone(&barrier);
            let record_id = record_id.clone();
            joins.push(thread::spawn(move || {
                barrier.wait();
                store.take_mcp_oauth(&record_id)
            }));
        }
        barrier.wait();

        let winners = joins
            .into_iter()
            .map(|join| join.join().expect("OAuth take contender"))
            .collect::<Result<Vec<_>, _>>()
            .expect("OAuth take result")
            .into_iter()
            .filter(Option::is_some)
            .count();
        assert_eq!(winners, 1);
    }

    #[test]
    fn mcp_oauth_precommit_failure_rolls_back_memory() {
        let temp = tempfile::tempdir().expect("temporary OAuth vault root");
        let path = temp.path().join("not-a-directory");
        std::fs::write(&path, b"occupied").expect("create path collision");
        let store = SecretStore::new_empty(Box::new(FixedKeyProvider), path);
        let record_id = mcp_credential_record_id(5);

        assert!(matches!(
            store.write_mcp_oauth(&record_id, b"must-not-commit", false),
            Err(SecretStoreError::Io(_))
        ));
        assert!(store
            .read_mcp_oauth(&record_id)
            .expect("read rolled-back record")
            .is_none());
    }

    #[test]
    fn mcp_oauth_ambiguous_commit_blocks_all_operations_until_reload() {
        let temp = tempfile::tempdir().expect("temporary OAuth vault root");
        let path = temp.path().join("secrets");
        let store = SecretStore::new_empty(Box::new(FixedKeyProvider), path.clone());
        let record_id = mcp_credential_record_id(9);
        store
            .write_mcp_oauth(&record_id, b"cached-token", false)
            .expect("persist initial token");
        store
            .state
            .write()
            .expect("OAuth state lock")
            .mcp_oauth_status = McpOAuthPartitionStatus::CommitUnknown;

        assert!(matches!(
            store.read_mcp_oauth(&record_id),
            Err(SecretStoreError::McpOAuthCommitStateUnknown)
        ));
        assert!(matches!(
            store.write_mcp_oauth(&record_id, b"replacement", false),
            Err(SecretStoreError::McpOAuthCommitStateUnknown)
        ));
        assert!(matches!(
            store.take_mcp_oauth(&record_id),
            Err(SecretStoreError::McpOAuthCommitStateUnknown)
        ));
        assert!(matches!(
            store.delete_mcp_oauth(&record_id),
            Err(SecretStoreError::McpOAuthCommitStateUnknown)
        ));
        drop(store);

        let restarted = SecretStore::new(Box::new(FixedKeyProvider), path)
            .expect("reload authoritative OAuth state");
        assert_eq!(
            restarted
                .read_mcp_oauth(&record_id)
                .expect("read after reconciliation restart"),
            Some(b"cached-token".to_vec())
        );
    }

    #[test]
    fn corrupt_mcp_oauth_partition_isolated_from_provisioned_secrets() {
        let temp = tempfile::tempdir().expect("temporary OAuth vault root");
        let path = temp.path().join("secrets");
        let store = SecretStore::new_empty(Box::new(FixedKeyProvider), path.clone());
        store
            .store_provisioned(
                "survivor",
                "Surviving credential",
                HashMap::from([("value".to_owned(), "secret".to_owned())]),
                InjectionTarget::Header {
                    name: "Authorization".to_owned(),
                    prefix: Some("Bearer ".to_owned()),
                },
                SecretPolicy::default(),
            )
            .expect("persist provisioned partition");
        store
            .write_mcp_oauth(&mcp_credential_record_id(6), b"oauth", false)
            .expect("persist OAuth partition");
        drop(store);
        std::fs::write(path.join(MCP_OAUTH_VAULT_FILENAME), b"corrupt")
            .expect("corrupt OAuth partition");

        let restarted = SecretStore::new(Box::new(FixedKeyProvider), path.clone())
            .expect("isolate corrupt OAuth partition");
        assert!(restarted.get_provisioned("survivor").is_some());
        assert!(restarted
            .read_mcp_oauth(&mcp_credential_record_id(6))
            .expect("read isolated OAuth partition")
            .is_none());
        assert!(std::fs::read_dir(&path)
            .expect("read quarantined partition directory")
            .filter_map(Result::ok)
            .any(|entry| entry
                .file_name()
                .to_string_lossy()
                .starts_with("mcp_oauth.corrupt-")));
    }

    #[test]
    fn unavailable_key_backend_fails_closed_without_quarantining_oauth_file() {
        let temp = tempfile::tempdir().expect("temporary OAuth vault root");
        let path = temp.path().join("secrets");
        let store = SecretStore::new_empty(Box::new(FixedKeyProvider), path.clone());
        store
            .write_mcp_oauth(&mcp_credential_record_id(7), b"oauth", false)
            .expect("persist OAuth partition");
        drop(store);

        assert!(matches!(
            SecretStore::new(Box::new(UnavailableKeyProvider), path.clone()),
            Err(SecretStoreError::Encryption(
                SecretEncryptionError::Keychain(_)
            ))
        ));
        assert!(path.join(MCP_OAUTH_VAULT_FILENAME).exists());
        assert!(!std::fs::read_dir(&path)
            .expect("read retained partition directory")
            .filter_map(Result::ok)
            .any(|entry| entry
                .file_name()
                .to_string_lossy()
                .starts_with("mcp_oauth.corrupt-")));

        let lenient = SecretStore::open(Box::new(UnavailableKeyProvider), path);
        assert!(matches!(
            lenient.read_mcp_oauth(&mcp_credential_record_id(7)),
            Err(SecretStoreError::McpOAuthUnavailable)
        ));
    }

    #[test]
    fn mcp_oauth_bounds_reject_invalid_records_and_exhausted_capacity() {
        assert!(validate_mcp_oauth_record_id(&mcp_credential_record_id(1)).is_ok());
        assert!(validate_mcp_oauth_record_id(&mcp_state_record_id(1)).is_ok());
        assert!(validate_mcp_oauth_record_id(&mcp_pending_record_id(1)).is_ok());
        assert!(matches!(
            validate_mcp_oauth_record_id("credentials:not-a-digest"),
            Err(SecretStoreError::McpOAuthInvalidRecord)
        ));
        assert!(matches!(
            validate_mcp_oauth_document(&[]),
            Err(SecretStoreError::McpOAuthInvalidRecord)
        ));
        assert!(matches!(
            validate_mcp_oauth_document(&vec![0; MAX_MCP_OAUTH_RECORD_BYTES + 1]),
            Err(SecretStoreError::McpOAuthInvalidRecord)
        ));

        let mut records = HashMap::with_capacity(MAX_MCP_OAUTH_RECORDS);
        for index in 0..MAX_MCP_OAUTH_RECORDS {
            records.insert(
                mcp_credential_record_id(index),
                McpOAuthVaultRecord::new(b"x"),
            );
        }
        assert!(matches!(
            validate_mcp_oauth_capacity(
                &records,
                &mcp_credential_record_id(MAX_MCP_OAUTH_RECORDS),
                1,
            ),
            Err(SecretStoreError::McpOAuthCapacity)
        ));
        assert!(validate_mcp_oauth_capacity(&records, &mcp_credential_record_id(0), 1).is_ok());
    }

    #[test]
    fn mcp_oauth_debug_output_never_contains_document() {
        let record = McpOAuthVaultRecord::new(b"do-not-print-this");
        let rendered = format!("{record:?}");
        assert_eq!(rendered, "McpOAuthVaultRecord([REDACTED])");
        assert!(!rendered.contains("do-not-print-this"));
    }

    #[test]
    fn concurrent_first_scope_resolution_returns_one_canonical_store() {
        const CONTENDERS: usize = 32;

        let root = temp_store_path();
        let resolver = Arc::new(SecretStoreResolver::new_with_capabilities(
            Box::new(InMemoryKeyProvider::new()),
            root.clone(),
            SecretRuntimeCapabilities::fully_available("in_memory"),
        ));
        let barrier = Arc::new(Barrier::new(CONTENDERS + 1));
        let mut joins = Vec::new();
        for _ in 0..CONTENDERS {
            let resolver = Arc::clone(&resolver);
            let barrier = Arc::clone(&barrier);
            joins.push(thread::spawn(move || {
                barrier.wait();
                resolver
                    .resolve_for_scope("owner", "default")
                    .expect("scope store")
            }));
        }
        barrier.wait();

        let stores = joins
            .into_iter()
            .map(|join| join.join().expect("resolver contender"))
            .collect::<Vec<_>>();
        let first = stores.first().expect("resolved stores");
        assert!(stores.iter().all(|store| Arc::ptr_eq(first, store)));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn store_and_retrieve_provisioned_secret() {
        let store = store_with_path(temp_store_path());
        let mut fields = HashMap::new();
        fields.insert("value".to_string(), "secret".to_string());

        store
            .store_provisioned(
                "secret-1",
                "Stripe key",
                fields.clone(),
                InjectionTarget::Header {
                    name: "Authorization".to_string(),
                    prefix: Some("Bearer ".to_string()),
                },
                SecretPolicy::default(),
            )
            .unwrap();

        let entry = store.get_provisioned("secret-1").unwrap();
        assert_eq!(entry.label, "Stripe key");
        assert_eq!(entry.fields, fields);
    }

    #[test]
    fn reject_provisioned_inline() {
        let store = store_with_path(temp_store_path());

        let result = store.store_provisioned(
            "secret-1",
            "Inline secret",
            HashMap::from([("value".to_string(), "secret".to_string())]),
            InjectionTarget::Inline,
            SecretPolicy::default(),
        );

        assert!(matches!(
            result,
            Err(SecretStoreError::InlineProvisionedSecret)
        ));
    }

    #[test]
    fn list_returns_only_id_and_label() {
        let store = store_with_path(temp_store_path());
        store
            .store_provisioned(
                "secret-1",
                "Stripe key",
                HashMap::from([("value".to_string(), "secret".to_string())]),
                InjectionTarget::Header {
                    name: "Authorization".to_string(),
                    prefix: None,
                },
                SecretPolicy::default(),
            )
            .unwrap();

        assert_eq!(
            store.list_available(),
            vec![SecretListEntry {
                id: "secret-1".to_string(),
                label: "Stripe key".to_string()
            }]
        );
    }

    #[test]
    fn ephemeral_register_get_clear_round_trip() {
        let store = store_with_path(temp_store_path());
        store
            .register_ephemeral("task-1", "input-1", "secret".to_string())
            .unwrap();

        assert_eq!(
            store.get_ephemeral_scoped("task-1", "input-1"),
            Some("secret".to_string())
        );
        assert_eq!(store.get_ephemeral("input-1"), Some("secret".to_string()));
        assert_eq!(store.clear_ephemeral("task-1"), 1);
        assert_eq!(store.get_ephemeral("input-1"), None);
    }

    #[test]
    fn a_scope_reports_holding_a_user_typed_secret_only_while_it_does() {
        // The acquisition signal a stateless driver pins on. This partition is
        // in-memory only, so a run holding one cannot move worker; a signal that
        // answered `true` for the wrong scope would pin runs that are free to
        // move, and one that answered `false` for the right scope would release a
        // run whose placeholders only this process can resolve.
        let store = store_with_path(temp_store_path());
        assert!(
            !store.ephemeral_scope_holds_user_typed_secret("execution:a"),
            "a scope nobody has written to holds nothing"
        );

        store
            .register_ephemeral("execution:a", "password", "alpha-secret".to_string())
            .unwrap();

        assert!(store.ephemeral_scope_holds_user_typed_secret("execution:a"));
        assert!(
            !store.ephemeral_scope_holds_user_typed_secret("execution:b"),
            "one run's secret must not pin another run"
        );

        assert_eq!(store.clear_ephemeral("execution:a"), 1);
        assert!(
            !store.ephemeral_scope_holds_user_typed_secret("execution:a"),
            "and the pin is answerable again once the scope is cleared"
        );
    }

    #[test]
    fn a_store_with_the_ephemeral_feature_off_can_never_come_to_hold_one() {
        // The reachability claim that `ephemeral_scope_holds_user_typed_secret`
        // rests on. `capabilities` is fixed at construction and
        // `register_ephemeral` is the partition's only writer, so a store whose
        // ephemeral feature is off holds an empty map for its whole life — which
        // is why "populated while enabled, then disabled" is not a case that
        // function has to survive, and why reading the map instead of the flag
        // costs nothing today.
        //
        // If that ever stops being true — a settable capability, a second write
        // path — this fails, and the reasoning on that function has to be
        // rewritten rather than quietly outlived.
        let mut capabilities = SecretRuntimeCapabilities::fully_available("test-provider");
        capabilities.ephemeral = SecretFeatureStatus::disabled("ephemeral off for this test");
        let store = SecretStore::new_empty_with_capabilities(
            Arc::new(InMemoryKeyProvider::new()),
            temp_store_path(),
            capabilities,
        );

        assert!(matches!(
            store.register_ephemeral("execution:a", "password", "s3cret".to_string()),
            Err(SecretStoreError::FeatureDisabled {
                feature: SecretSourceKind::Ephemeral,
                ..
            })
        ));
        assert!(
            !store.ephemeral_scope_holds_user_typed_secret("execution:a"),
            "the partition has no other writer, so a disabled store's map stays empty"
        );
    }

    #[test]
    fn scoped_ephemeral_entries_do_not_collide() {
        let store = store_with_path(temp_store_path());
        store
            .register_ephemeral("execution:a", "password", "alpha-secret".to_string())
            .unwrap();
        store
            .register_ephemeral("execution:b", "password", "beta-secret".to_string())
            .unwrap();

        assert_eq!(
            store.get_ephemeral_scoped("execution:a", "password"),
            Some("alpha-secret".to_string())
        );
        assert_eq!(
            store.get_ephemeral_scoped("execution:b", "password"),
            Some("beta-secret".to_string())
        );
        assert_eq!(
            store.get_ephemeral("password"),
            None,
            "legacy lookup must not guess when multiple scoped entries exist"
        );
        assert_eq!(
            store.clear_ephemeral("execution:a"),
            1,
            "clearing one scope should not delete the other",
        );
        assert_eq!(
            store.get_ephemeral_scoped("execution:b", "password"),
            Some("beta-secret".to_string())
        );
        assert_eq!(
            store.get_ephemeral("password"),
            Some("beta-secret".to_string()),
            "single remaining scoped entry stays discoverable by legacy lookup"
        );
    }

    #[test]
    fn provisioned_persistence_round_trip() {
        let path = temp_store_path();
        let store = SecretStore::new_empty(Box::new(FixedKeyProvider), path.clone());
        store
            .store_provisioned(
                "secret-1",
                "Stripe key",
                HashMap::from([("value".to_string(), "secret".to_string())]),
                InjectionTarget::Header {
                    name: "Authorization".to_string(),
                    prefix: Some("Bearer ".to_string()),
                },
                SecretPolicy::default(),
            )
            .unwrap();

        let reloaded = SecretStore::new(Box::new(FixedKeyProvider), path).unwrap();
        assert!(reloaded.get_provisioned("secret-1").is_some());
    }

    #[test]
    fn corrupted_vault_file_returns_error() {
        let path = temp_store_path();
        std::fs::create_dir_all(&path).unwrap();
        std::fs::write(
            path.join("provisioned_secrets.vault"),
            b"not encrypted json",
        )
        .unwrap();

        let result = SecretStore::new(Box::new(InMemoryKeyProvider::new()), path.clone());
        assert!(result.is_err());

        let empty = SecretStore::new_empty(Box::new(InMemoryKeyProvider::new()), path);
        assert!(empty.list_available().is_empty());
    }

    #[test]
    fn concurrent_read_write_is_safe() {
        let store = Arc::new(store_with_path(temp_store_path()));
        let writer = {
            let store = Arc::clone(&store);
            thread::spawn(move || {
                for idx in 0..50 {
                    store
                        .register_ephemeral(
                            "task-1",
                            &format!("input-{idx}"),
                            format!("secret-{idx}"),
                        )
                        .unwrap();
                }
            })
        };
        let reader = {
            let store = Arc::clone(&store);
            thread::spawn(move || {
                for _ in 0..50 {
                    let _ = store.list_available();
                    let _ = store.get_ephemeral("input-1");
                }
            })
        };

        writer.join().unwrap();
        reader.join().unwrap();
        assert_eq!(store.get_ephemeral("input-1"), Some("secret-1".to_string()));
    }

    #[test]
    fn captured_store_and_get_session_round_trip() {
        let store = store_with_path(temp_store_path());
        store
            .store_captured(
                "https://api.example.com",
                HashMap::from([("Authorization".to_string(), "Bearer token".to_string())]),
                vec![CookieWithMetadata {
                    name: "session".to_string(),
                    value: "cookie-value".to_string(),
                    domain: "api.example.com".to_string(),
                    path: "/".to_string(),
                    secure: true,
                    http_only: true,
                    same_site: SameSite::Lax,
                    expires: None,
                }],
                HashMap::from([("refresh".to_string(), "refresh-token".to_string())]),
                HashMap::from([("csrf".to_string(), "session-csrf".to_string())]),
            )
            .unwrap();

        let (session, lease) = store
            .get_session("https://api.example.com", "https://api.example.com/pay")
            .unwrap();
        assert_eq!(
            lease.stale_mark_targets(),
            &[CapturedSessionTarget {
                origin: "https://api.example.com".to_string(),
                generation: 1,
            }]
        );
        assert_eq!(session.auth_headers["Authorization"], "Bearer token");
        assert_eq!(session.cookies["session"], "cookie-value");
        assert_eq!(
            session.cookie_header_values,
            vec![SessionCookie {
                name: "session".to_string(),
                value: "cookie-value".to_string(),
            }]
        );
        assert_eq!(session.local_storage["refresh"], "refresh-token");
        assert_eq!(session.session_storage["csrf"], "session-csrf");
    }

    #[test]
    fn captured_query_params_round_trip_to_session_context() {
        let store = store_with_path(temp_store_path());
        store
            .store_captured_deferred_with_query_params(
                "https://api.example.com",
                HashMap::new(),
                HashMap::from([("api_key".to_string(), "secret-query-key".to_string())]),
                Vec::new(),
                HashMap::new(),
                HashMap::new(),
            )
            .unwrap();

        let (session, _) = store
            .get_session("https://api.example.com", "https://api.example.com/search")
            .unwrap();
        assert_eq!(
            session.auth_query_params.get("api_key").map(String::as_str),
            Some("secret-query-key")
        );
    }

    #[test]
    fn captured_session_uses_same_site_cookies_from_other_origins() {
        let store = store_with_path(temp_store_path());
        store
            .store_captured(
                "https://app.example.com",
                HashMap::new(),
                vec![CookieWithMetadata {
                    name: "session".to_string(),
                    value: "shared-cookie".to_string(),
                    domain: ".example.com".to_string(),
                    path: "/".to_string(),
                    secure: true,
                    http_only: true,
                    same_site: SameSite::Lax,
                    expires: None,
                }],
                HashMap::from([("page-token".to_string(), "page-only".to_string())]),
                HashMap::from([("page-session".to_string(), "tab-only".to_string())]),
            )
            .unwrap();

        let (session, lease) = store
            .get_session("https://api.example.com", "https://api.example.com/pay")
            .expect("same-site cookie fallback should exist");

        assert_eq!(
            lease.stale_mark_targets(),
            &[CapturedSessionTarget {
                origin: "https://app.example.com".to_string(),
                generation: 1,
            }]
        );
        assert_eq!(
            session.cookies.get("session").map(String::as_str),
            Some("shared-cookie")
        );
        assert!(
            session.auth_headers.is_empty(),
            "exact-origin-only headers must not be fabricated from another origin"
        );
        assert!(
            session.local_storage.is_empty(),
            "browser storage remains scoped to the exact captured origin"
        );
        assert!(
            session.session_storage.is_empty(),
            "browser storage remains scoped to the exact captured origin"
        );
    }

    #[test]
    fn captured_browser_state_snapshot_clears_removed_cookies() {
        let store = store_with_path(temp_store_path());
        store
            .store_captured_browser_state_deferred(
                "https://app.example.com",
                vec![CookieWithMetadata {
                    name: "session".to_string(),
                    value: "cookie-value".to_string(),
                    domain: ".example.com".to_string(),
                    path: "/".to_string(),
                    secure: true,
                    http_only: true,
                    same_site: SameSite::Lax,
                    expires: None,
                }],
                Some(HashMap::from([(
                    "refresh".to_string(),
                    "refresh-token".to_string(),
                )])),
                Some(HashMap::from([(
                    "csrf".to_string(),
                    "csrf-token".to_string(),
                )])),
            )
            .unwrap();
        store.flush_captured().unwrap();

        let (session_before, _) = store
            .get_session("https://app.example.com", "https://app.example.com/home")
            .expect("captured session should exist before logout");
        assert_eq!(
            session_before.cookies.get("session").map(String::as_str),
            Some("cookie-value")
        );
        assert_eq!(
            session_before
                .local_storage
                .get("refresh")
                .map(String::as_str),
            Some("refresh-token")
        );

        store
            .store_captured_browser_state_deferred(
                "https://app.example.com",
                Vec::new(),
                Some(HashMap::new()),
                Some(HashMap::new()),
            )
            .unwrap();
        store.flush_captured().unwrap();

        let (session_after, lease) = store
            .get_session("https://app.example.com", "https://app.example.com/home")
            .expect("exact captured origin still exists, but cookies/storage should be cleared");
        assert_eq!(
            lease.stale_mark_targets(),
            &[CapturedSessionTarget {
                origin: "https://app.example.com".to_string(),
                generation: 2,
            }]
        );
        assert!(session_after.cookies.is_empty());
        assert!(session_after.cookie_header_values.is_empty());
        assert!(session_after.local_storage.is_empty());
        assert!(session_after.session_storage.is_empty());
    }

    #[test]
    fn captured_store_merges_partial_headers_cookies_and_storage() {
        let store = store_with_path(temp_store_path());
        store
            .store_captured(
                "https://api.example.com",
                HashMap::from([("Authorization".to_string(), "Bearer token".to_string())]),
                vec![CookieWithMetadata {
                    name: "session".to_string(),
                    value: "cookie-value".to_string(),
                    domain: "api.example.com".to_string(),
                    path: "/".to_string(),
                    secure: true,
                    http_only: true,
                    same_site: SameSite::Lax,
                    expires: None,
                }],
                HashMap::from([("refresh".to_string(), "refresh-token".to_string())]),
                HashMap::new(),
            )
            .unwrap();
        store
            .store_captured(
                "https://api.example.com",
                HashMap::from([("x-csrf-token".to_string(), "csrf-token".to_string())]),
                vec![CookieWithMetadata {
                    name: "session".to_string(),
                    value: "updated-cookie".to_string(),
                    domain: "api.example.com".to_string(),
                    path: "/".to_string(),
                    secure: true,
                    http_only: true,
                    same_site: SameSite::Lax,
                    expires: None,
                }],
                HashMap::new(),
                HashMap::from([("csrf".to_string(), "session-token".to_string())]),
            )
            .unwrap();

        let (session, lease) = store
            .get_session("https://api.example.com", "https://api.example.com/pay")
            .unwrap();
        assert_eq!(
            lease.stale_mark_targets(),
            &[CapturedSessionTarget {
                origin: "https://api.example.com".to_string(),
                generation: 2,
            }]
        );
        assert_eq!(
            session
                .auth_headers
                .get("Authorization")
                .map(String::as_str),
            Some("Bearer token")
        );
        assert_eq!(
            session.auth_headers.get("x-csrf-token").map(String::as_str),
            Some("csrf-token")
        );
        assert_eq!(
            session.cookies.get("session").map(String::as_str),
            Some("updated-cookie")
        );
        assert_eq!(
            session.local_storage.get("refresh").map(String::as_str),
            Some("refresh-token")
        );
        assert_eq!(
            session.session_storage.get("csrf").map(String::as_str),
            Some("session-token")
        );
    }

    #[test]
    fn captured_cookies_preserve_duplicate_names_across_paths() {
        let store = store_with_path(temp_store_path());
        store
            .store_captured(
                "https://api.example.com",
                HashMap::new(),
                vec![
                    CookieWithMetadata {
                        name: "session".to_string(),
                        value: "admin-cookie".to_string(),
                        domain: "api.example.com".to_string(),
                        path: "/admin".to_string(),
                        secure: true,
                        http_only: true,
                        same_site: SameSite::Lax,
                        expires: None,
                    },
                    CookieWithMetadata {
                        name: "session".to_string(),
                        value: "root-cookie".to_string(),
                        domain: "api.example.com".to_string(),
                        path: "/".to_string(),
                        secure: true,
                        http_only: true,
                        same_site: SameSite::Lax,
                        expires: None,
                    },
                ],
                HashMap::new(),
                HashMap::new(),
            )
            .unwrap();

        let (root_session, _) = store
            .get_session("https://api.example.com", "https://api.example.com/profile")
            .expect("root path session should exist");
        assert_eq!(root_session.cookie_metadata.len(), 1);
        assert_eq!(root_session.cookie_metadata[0].path, "/");
        assert_eq!(
            root_session.cookies.get("session").map(String::as_str),
            Some("root-cookie")
        );

        let (admin_session, _) = store
            .get_session(
                "https://api.example.com",
                "https://api.example.com/admin/dashboard",
            )
            .expect("admin path session should exist");
        assert_eq!(
            admin_session
                .cookie_metadata
                .iter()
                .map(|cookie| cookie.path.as_str())
                .collect::<Vec<_>>(),
            vec!["/admin", "/"]
        );
        assert_eq!(
            admin_session.cookies.get("session").map(String::as_str),
            Some("admin-cookie")
        );
        assert_eq!(
            admin_session.cookie_header_values,
            vec![
                SessionCookie {
                    name: "session".to_string(),
                    value: "admin-cookie".to_string(),
                },
                SessionCookie {
                    name: "session".to_string(),
                    value: "root-cookie".to_string(),
                },
            ]
        );
    }

    #[test]
    fn captured_cookies_do_not_match_sibling_path_prefixes() {
        let store = store_with_path(temp_store_path());
        store
            .store_captured(
                "https://api.example.com",
                HashMap::new(),
                vec![
                    CookieWithMetadata {
                        name: "session".to_string(),
                        value: "admin-cookie".to_string(),
                        domain: "api.example.com".to_string(),
                        path: "/admin".to_string(),
                        secure: true,
                        http_only: true,
                        same_site: SameSite::Lax,
                        expires: None,
                    },
                    CookieWithMetadata {
                        name: "session".to_string(),
                        value: "root-cookie".to_string(),
                        domain: "api.example.com".to_string(),
                        path: "/".to_string(),
                        secure: true,
                        http_only: true,
                        same_site: SameSite::Lax,
                        expires: None,
                    },
                ],
                HashMap::new(),
                HashMap::new(),
            )
            .unwrap();

        let (session, _) = store
            .get_session(
                "https://api.example.com",
                "https://api.example.com/administrator",
            )
            .expect("sibling path session should exist");

        assert_eq!(
            session.cookie_header_values,
            vec![SessionCookie {
                name: "session".to_string(),
                value: "root-cookie".to_string(),
            }]
        );
        assert_eq!(
            session.cookies.get("session").map(String::as_str),
            Some("root-cookie")
        );
    }

    #[test]
    fn captured_session_lease_marks_fallback_cookie_source_stale() {
        let store = store_with_path(temp_store_path());
        store
            .store_captured(
                "https://app.example.com",
                HashMap::new(),
                vec![CookieWithMetadata {
                    name: "session".to_string(),
                    value: "shared-cookie".to_string(),
                    domain: ".example.com".to_string(),
                    path: "/".to_string(),
                    secure: true,
                    http_only: true,
                    same_site: SameSite::Lax,
                    expires: None,
                }],
                HashMap::new(),
                HashMap::new(),
            )
            .unwrap();

        let (_session, lease) = store
            .get_session("https://api.example.com", "https://api.example.com/pay")
            .expect("fallback session should exist");

        assert_eq!(
            lease.stale_mark_targets(),
            &[CapturedSessionTarget {
                origin: "https://app.example.com".to_string(),
                generation: 1,
            }]
        );
        assert!(
            store
                .mark_stale_lease(&lease)
                .expect("stale marking should succeed"),
            "fallback source should be marked stale"
        );
        assert!(
            store
                .get_session("https://api.example.com", "https://api.example.com/pay")
                .is_none(),
            "stale fallback source must no longer satisfy the replay session"
        );
    }

    #[test]
    fn captured_session_lease_can_mark_matching_generation_fresh() {
        let store = store_with_path(temp_store_path());
        store
            .store_captured(
                "https://app.example.com",
                HashMap::new(),
                vec![CookieWithMetadata {
                    name: "session".to_string(),
                    value: "shared-cookie".to_string(),
                    domain: ".example.com".to_string(),
                    path: "/".to_string(),
                    secure: true,
                    http_only: true,
                    same_site: SameSite::Lax,
                    expires: None,
                }],
                HashMap::new(),
                HashMap::new(),
            )
            .unwrap();

        let (_session, lease) = store
            .get_session("https://api.example.com", "https://api.example.com/pay")
            .expect("fallback session should exist");

        assert!(store.mark_stale_lease(&lease).unwrap());
        assert!(store.captured_status("https://app.example.com").is_stale);
        assert!(store.mark_fresh_lease(&lease).unwrap());
        assert!(!store.captured_status("https://app.example.com").is_stale);
        assert!(
            store
                .get_session("https://api.example.com", "https://api.example.com/pay")
                .is_some(),
            "fresh fallback source should satisfy replay sessions again"
        );
    }

    #[test]
    fn evaluate_access_does_not_consume_budget_until_recorded() {
        let store = store_with_path(temp_store_path());
        store
            .store_provisioned(
                "secret-1",
                "Budgeted key",
                HashMap::from([("value".to_string(), "secret".to_string())]),
                InjectionTarget::Header {
                    name: "Authorization".to_string(),
                    prefix: None,
                },
                SecretPolicy {
                    max_uses_per_day: Some(1),
                    ..SecretPolicy::default()
                },
            )
            .unwrap();

        assert!(matches!(
            store
                .evaluate_access("secret-1", "http", "post", Some("api.example.com"))
                .unwrap(),
            PolicyResult::Allowed
        ));
        assert!(matches!(
            store
                .evaluate_access("secret-1", "http", "post", Some("api.example.com"))
                .unwrap(),
            PolicyResult::Allowed
        ));

        store.record_usage("secret-1", None).unwrap();

        assert!(matches!(
            store
                .evaluate_access("secret-1", "http", "post", Some("api.example.com"))
                .unwrap(),
            PolicyResult::Denied { .. }
        ));
    }

    #[test]
    fn disabled_provisioned_partition_keeps_ephemeral_flow_available() {
        let store = disabled_store_with_path(temp_store_path());

        let provisioned_result = store.store_provisioned(
            "secret-1",
            "Stripe key",
            HashMap::from([("value".to_string(), "secret".to_string())]),
            InjectionTarget::Header {
                name: "Authorization".to_string(),
                prefix: Some("Bearer ".to_string()),
            },
            SecretPolicy::default(),
        );
        assert!(matches!(
            provisioned_result,
            Err(SecretStoreError::FeatureDisabled {
                feature: SecretSourceKind::Provisioned,
                ..
            })
        ));

        store
            .register_ephemeral("task-1", "otp", "123456".to_string())
            .expect("ephemeral flow should remain available");
        assert_eq!(store.get_ephemeral("otp"), Some("123456".to_string()));
        assert!(store.list_available().is_empty());
    }

    #[test]
    fn canonical_static_secret_presence_blocks_fallback_when_feature_becomes_unavailable() {
        let mut store = store_with_path(temp_store_path());
        store
            .store_provisioned(
                "secret-1",
                "Canonical key",
                HashMap::from([("value".to_owned(), "canary".to_owned())]),
                InjectionTarget::Header {
                    name: "Authorization".to_owned(),
                    prefix: Some("Bearer ".to_owned()),
                },
                SecretPolicy::default(),
            )
            .expect("canonical secret");
        store.capabilities = SecretRuntimeCapabilities::without_durable_storage(
            "disabled-after-load",
            "test feature outage",
        );

        assert!(store.contains_provisioned("secret-1"));
        assert!(matches!(
            store.issue_grant("secret-1", "tool", "run", None, None),
            Err(SecretStoreError::FeatureDisabled {
                feature: SecretSourceKind::Provisioned,
                ..
            })
        ));
    }

    // ---- durability -------------------------------------------------------

    const DURABILITY_CONTENDERS: usize = 8;

    fn placeholder_header() -> InjectionTarget {
        InjectionTarget::Header {
            name: "Authorization".to_owned(),
            prefix: Some("Bearer ".to_owned()),
        }
    }

    fn files_matching(directory: &Path, needle: &str) -> Vec<PathBuf> {
        let Ok(entries) = std::fs::read_dir(directory) else {
            return Vec::new();
        };
        let mut matched = entries
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.contains(needle))
            })
            .collect::<Vec<_>>();
        matched.sort();
        matched
    }

    /// Every writer of a partition used to share one `provisioned_secrets.tmp`,
    /// so two of them were inside the same file and the rename published
    /// whatever mixture won. The snapshot was also taken outside any exclusion,
    /// so an older one could be renamed last and silently roll the vault back.
    ///
    /// Distinct ids make both failures visible: anything missing after a reload
    /// was either interleaved away or overtaken.
    #[test]
    fn concurrent_provisioned_writes_all_survive_a_reload() {
        let temp = tempfile::tempdir().expect("temporary vault root");
        let base = temp.path().join("secrets");
        let store = Arc::new(SecretStore::new_empty(
            Box::new(FixedKeyProvider),
            base.clone(),
        ));
        let barrier = Arc::new(Barrier::new(DURABILITY_CONTENDERS + 1));

        let mut joins = Vec::with_capacity(DURABILITY_CONTENDERS);
        for index in 0..DURABILITY_CONTENDERS {
            let store = Arc::clone(&store);
            let barrier = Arc::clone(&barrier);
            joins.push(thread::spawn(move || {
                barrier.wait();
                store.store_provisioned(
                    format!("placeholder-secret-{index}"),
                    format!("Placeholder {index}"),
                    HashMap::from([(
                        "value".to_owned(),
                        format!("PLACEHOLDER-NOT-A-REAL-VALUE-{index}"),
                    )]),
                    placeholder_header(),
                    SecretPolicy::default(),
                )
            }));
        }
        barrier.wait();
        for join in joins {
            join.join()
                .expect("provisioned writer thread")
                .expect("provisioned write");
        }
        drop(store);

        let reloaded = SecretStore::new(Box::new(FixedKeyProvider), base.clone())
            .expect("reload after concurrent writes");
        assert_eq!(
            reloaded.list_provisioned_entries().len(),
            DURABILITY_CONTENDERS,
            "a concurrent writer's entries were lost between memory and disk"
        );
        assert_eq!(
            reloaded.partition_status(SecretSourceKind::Provisioned),
            SecretPartitionStatus::Loaded
        );
        assert!(
            files_matching(&base, ".tmp").is_empty(),
            "a completed durable write must not leave its temp behind"
        );
    }

    /// The captured partition has its own writers — `store_captured` persists
    /// directly while `flush_captured` coalesces — and they shared the same
    /// fixed temp.
    #[test]
    fn concurrent_captured_writes_all_survive_a_reload() {
        let temp = tempfile::tempdir().expect("temporary vault root");
        let base = temp.path().join("secrets");
        let store = Arc::new(SecretStore::new_empty(
            Box::new(FixedKeyProvider),
            base.clone(),
        ));
        let barrier = Arc::new(Barrier::new(DURABILITY_CONTENDERS + 1));

        let mut joins = Vec::with_capacity(DURABILITY_CONTENDERS);
        for index in 0..DURABILITY_CONTENDERS {
            let store = Arc::clone(&store);
            let barrier = Arc::clone(&barrier);
            joins.push(thread::spawn(move || {
                barrier.wait();
                store.store_captured(
                    &format!("https://placeholder-{index}.invalid"),
                    HashMap::from([(
                        "authorization".to_owned(),
                        format!("Bearer PLACEHOLDER-{index}"),
                    )]),
                    Vec::new(),
                    HashMap::new(),
                    HashMap::new(),
                )
            }));
        }
        barrier.wait();
        for join in joins {
            join.join()
                .expect("captured writer thread")
                .expect("captured write");
        }
        drop(store);

        let reloaded = SecretStore::new(Box::new(FixedKeyProvider), base.clone())
            .expect("reload after concurrent captured writes");
        assert_eq!(
            reloaded.all_captured_statuses().len(),
            DURABILITY_CONTENDERS
        );
        assert!(
            files_matching(&base, ".tmp").is_empty(),
            "a completed durable write must not leave its temp behind"
        );
    }

    /// **The decision.** An unreadable vault reads as *empty*, not as a refusal
    /// to start.
    ///
    /// Partitions are per-scope, so erroring would take the daemon down over one
    /// scope's damaged file, and empty already fails every credential lookup
    /// closed — the safe direction. What empty must not be is *silent*: it is
    /// otherwise indistinguishable from a scope that never provisioned anything,
    /// which is how an operator ends up re-provisioning over recoverable data.
    /// So the status says `Unavailable` and the bytes stay on disk.
    #[test]
    fn an_unreadable_provisioned_vault_reads_as_empty_and_reports_it() {
        let temp = tempfile::tempdir().expect("temporary vault root");
        let base = temp.path().join("secrets");
        std::fs::create_dir_all(&base).expect("vault directory");
        std::fs::write(
            base.join("provisioned_secrets.vault"),
            b"PLACEHOLDER-NOT-CIPHERTEXT",
        )
        .expect("write an unreadable vault");

        let store = SecretStore::open(Box::new(FixedKeyProvider), base.clone());

        assert!(
            store.list_provisioned_entries().is_empty(),
            "an unreadable vault must not appear to hold entries"
        );
        assert_eq!(
            store.partition_status(SecretSourceKind::Provisioned),
            SecretPartitionStatus::Unavailable,
            "empty and unreadable must be distinguishable"
        );
        assert_eq!(
            store.partition_status(SecretSourceKind::Captured),
            SecretPartitionStatus::Loaded,
            "one damaged partition must not condemn its neighbour"
        );
        assert_eq!(
            files_matching(&base, ".corrupt-").len(),
            1,
            "the unreadable bytes must be kept, not deleted"
        );
        assert!(
            !base.join("provisioned_secrets.vault").exists(),
            "the unreadable file must be moved aside, not left to fail every load"
        );
    }

    /// A fixed `.corrupt` name meant the second occurrence renamed over the
    /// first — and the first is the copy that still held the entries. One repeat
    /// of a transient key-provider fault destroyed the only surviving
    /// ciphertext.
    #[test]
    fn a_second_quarantine_does_not_clobber_the_first() {
        let temp = tempfile::tempdir().expect("temporary vault root");
        let base = temp.path().join("secrets");
        std::fs::create_dir_all(&base).expect("vault directory");

        let first = b"PLACEHOLDER-FIRST".as_slice();
        let second = b"PLACEHOLDER-SECOND".as_slice();
        for body in [first, second] {
            std::fs::write(base.join("provisioned_secrets.vault"), body)
                .expect("write an unreadable vault");
            let store = SecretStore::open(Box::new(FixedKeyProvider), base.clone());
            assert_eq!(
                store.partition_status(SecretSourceKind::Provisioned),
                SecretPartitionStatus::Unavailable
            );
        }

        let quarantined = files_matching(&base, ".corrupt-");
        assert_eq!(
            quarantined.len(),
            2,
            "each quarantine needs its own name or the earlier forensic copy is gone"
        );
        let bodies = quarantined
            .iter()
            .map(|path| std::fs::read(path).expect("read quarantined vault"))
            .collect::<Vec<_>>();
        assert!(bodies.iter().any(|body| body.as_slice() == first));
        assert!(bodies.iter().any(|body| body.as_slice() == second));
    }

    /// Writes stay permitted after a quarantine: the unreadable bytes are
    /// already preserved beside the vault, so refusing would leave the scope
    /// with no way forward and destroy nothing extra by allowing it.
    #[test]
    fn a_quarantined_partition_can_still_be_written_and_reloaded() {
        let temp = tempfile::tempdir().expect("temporary vault root");
        let base = temp.path().join("secrets");
        std::fs::create_dir_all(&base).expect("vault directory");
        std::fs::write(
            base.join("provisioned_secrets.vault"),
            b"PLACEHOLDER-NOT-CIPHERTEXT",
        )
        .expect("write an unreadable vault");

        let store = SecretStore::open(Box::new(FixedKeyProvider), base.clone());
        store
            .store_provisioned(
                "placeholder-secret",
                "Placeholder",
                HashMap::from([(
                    "value".to_owned(),
                    "PLACEHOLDER-NOT-A-REAL-VALUE".to_owned(),
                )]),
                placeholder_header(),
                SecretPolicy::default(),
            )
            .expect("re-provisioning after a quarantine");
        drop(store);

        let reloaded = SecretStore::new(Box::new(FixedKeyProvider), base.clone())
            .expect("reload after re-provisioning");
        assert_eq!(reloaded.list_provisioned_entries().len(), 1);
        assert_eq!(
            reloaded.partition_status(SecretSourceKind::Provisioned),
            SecretPartitionStatus::Loaded
        );
        assert_eq!(
            files_matching(&base, ".corrupt-").len(),
            1,
            "the quarantined copy must survive the write that replaced it"
        );
    }

    /// The scoped resolver is the production path, and dropping the store on
    /// any load failure made corruption read as *one 500, then silently empty
    /// forever*: the quarantine has already moved the file, so the retry builds
    /// a store that loads nothing and reports the scope as unprovisioned.
    ///
    /// A key backend that was momentarily unreachable is the opposite case — it
    /// leaves the vault intact — and must still refuse, so a later attempt can
    /// find the real data.
    #[test]
    fn a_quarantined_scope_resolves_as_unavailable_rather_than_unprovisioned() {
        let temp = tempfile::tempdir().expect("temporary scope root");
        let root = temp.path().to_path_buf();

        let seeding = SecretStoreResolver::new_with_capabilities(
            Box::new(FixedKeyProvider),
            root.clone(),
            SecretRuntimeCapabilities::fully_available("fixed"),
        );
        let base = seeding
            .resolve_for_scope("placeholder-principal", "placeholder-workspace")
            .expect("resolve a fresh scope")
            .base_dir()
            .to_path_buf();
        drop(seeding);

        std::fs::create_dir_all(&base).expect("vault directory");
        std::fs::write(
            base.join("provisioned_secrets.vault"),
            b"PLACEHOLDER-NOT-CIPHERTEXT",
        )
        .expect("write an unreadable vault");

        let resolver = SecretStoreResolver::new_with_capabilities(
            Box::new(FixedKeyProvider),
            root.clone(),
            SecretRuntimeCapabilities::fully_available("fixed"),
        );
        let store = resolver
            .resolve_for_scope("placeholder-principal", "placeholder-workspace")
            .expect("a quarantined partition must not fail the whole scope");
        assert_eq!(
            store.partition_status(SecretSourceKind::Provisioned),
            SecretPartitionStatus::Unavailable,
            "the resolver must keep the store that knows the vault was unreadable"
        );
        assert!(store.list_provisioned_entries().is_empty());
    }

    /// The other half of the same decision: a key backend that cannot answer
    /// leaves the vault intact on disk, so the scope must refuse rather than
    /// present itself as empty — and the file must still be there for the
    /// attempt that succeeds.
    #[test]
    fn a_key_backend_outage_refuses_the_scope_and_keeps_the_vault() {
        let temp = tempfile::tempdir().expect("temporary scope root");
        let root = temp.path().to_path_buf();

        let seeding = SecretStoreResolver::new_with_capabilities(
            Box::new(FixedKeyProvider),
            root.clone(),
            SecretRuntimeCapabilities::fully_available("fixed"),
        );
        let store = seeding
            .resolve_for_scope("placeholder-principal", "placeholder-workspace")
            .expect("resolve a fresh scope");
        store
            .store_provisioned(
                "placeholder-secret",
                "Placeholder",
                HashMap::from([(
                    "value".to_owned(),
                    "PLACEHOLDER-NOT-A-REAL-VALUE".to_owned(),
                )]),
                placeholder_header(),
                SecretPolicy::default(),
            )
            .expect("seed a real vault");
        let base = store.base_dir().to_path_buf();
        drop(store);
        drop(seeding);

        let unavailable = SecretStoreResolver::new_with_capabilities(
            Box::new(UnavailableKeyProvider),
            root,
            SecretRuntimeCapabilities::fully_available("unavailable-test-provider"),
        );
        assert!(
            matches!(
                unavailable.resolve_for_scope("placeholder-principal", "placeholder-workspace"),
                Err(SecretStoreError::Encryption(
                    SecretEncryptionError::Keychain(_)
                ))
            ),
            "a key backend outage must not be mistaken for corruption"
        );
        assert!(
            base.join("provisioned_secrets.vault").exists(),
            "an intact vault must not be moved aside over a recoverable fault"
        );
        assert!(files_matching(&base, ".corrupt-").is_empty());
    }

    // ---- §15 item 2: vault files and journal are owner-only ------------------

    #[cfg(unix)]
    fn mode_of(path: &Path) -> u32 {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(path)
            .expect("metadata")
            .permissions()
            .mode()
            & 0o777
    }

    #[cfg(unix)]
    #[test]
    fn provisioned_vault_and_its_directory_are_written_owner_only() {
        let temp = tempfile::tempdir().expect("temporary vault root");
        let base = temp.path().join("secrets");
        let store = SecretStore::open(Box::new(FixedKeyProvider), base.clone());
        store
            .store_provisioned(
                "placeholder-secret",
                "Placeholder",
                HashMap::from([(
                    "value".to_owned(),
                    "PLACEHOLDER-NOT-A-REAL-VALUE".to_owned(),
                )]),
                placeholder_header(),
                SecretPolicy::default(),
            )
            .expect("provision");
        assert_eq!(mode_of(&base.join("provisioned_secrets.vault")), 0o600);
        assert_eq!(mode_of(&base), 0o700);
    }

    #[cfg(unix)]
    #[test]
    fn audit_journal_is_owner_only_and_a_loose_existing_journal_is_tightened() {
        use std::os::unix::fs::PermissionsExt;
        let temp = tempfile::tempdir().expect("temporary vault root");
        let base = temp.path().join("secrets");
        std::fs::create_dir_all(&base).expect("vault directory");
        let journal = base.join(SECRET_AUDIT_FILENAME);
        std::fs::write(&journal, b"{}\n").expect("pre-existing journal");
        std::fs::set_permissions(&journal, std::fs::Permissions::from_mode(0o644))
            .expect("loosen the fixture");
        assert_eq!(mode_of(&journal), 0o644, "fixture must start loose");

        let store = SecretStore::open(Box::new(FixedKeyProvider), base.clone());
        let event: SecretAuditEvent =
            serde_json::from_value(serde_json::json!({ "timestamp": 0, "event": "perm-test" }))
                .expect("minimal audit event");
        store.try_audit_event(event).expect("append audit event");

        assert_eq!(
            mode_of(&journal),
            0o600,
            "journal was not tightened on append"
        );
        assert_eq!(mode_of(&base), 0o700);
    }
}
