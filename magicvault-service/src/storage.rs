//! Standalone identity and single-writer lease; never adopts another product's root.
use std::{fs::{self, File, OpenOptions}, io::Read, path::{Path, PathBuf}, sync::Arc};
use fs2::FileExt;
use magicvault_core::encryption::{MasterKeyProvider, SecretEncryptionError};
use magicvault_primitives::durable_io::write_bytes_durably_with_mode_sync;
use magicvault_protocol::ErrorCode;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use zeroize::Zeroizing;

// 32 clients × 256 UUID references, plus bounded labels/hashes and JSON syntax.
pub const MAX_STATE_BYTES: u64 = 512 * 1024;
pub const KEYCHAIN_SERVICE: &str = "ai.magicbeans.magicvault";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Instance { pub format_version: u32, pub id: Uuid }

pub struct InstanceLock { root: PathBuf, _file: File, pub instance: Instance }
impl InstanceLock {
    pub fn root(&self) -> &Path { &self.root }
    pub fn socket(&self) -> PathBuf { self.root.join("rpc.sock") }
}

pub fn default_root() -> Result<PathBuf, ErrorCode> {
    let user_root = std::env::var_os("HOME").ok_or(ErrorCode::Unavailable)?;
    let user_root = PathBuf::from(user_root);
    if !user_root.is_absolute() { return Err(ErrorCode::InvalidRequest); }
    Ok(user_root.join(".magicvault"))
}

#[cfg(unix)]
pub fn private_path(path: &Path, directory: bool) -> Result<(), ErrorCode> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    let metadata = fs::symlink_metadata(path).map_err(|_| ErrorCode::Unavailable)?;
    if metadata.file_type().is_symlink() || metadata.is_dir() != directory
        || (!directory && !metadata.is_file())
        || metadata.uid() != unsafe { libc::geteuid() }
        || metadata.permissions().mode() & 0o077 != 0 {
        return Err(ErrorCode::Unavailable);
    }
    Ok(())
}
#[cfg(not(unix))]
pub fn private_path(_: &Path, _: bool) -> Result<(), ErrorCode> { Err(ErrorCode::Unavailable) }

fn validate_root(root: &Path) -> Result<(), ErrorCode> {
    if !root.is_absolute() || root.parent().is_none() || root.as_os_str().len() > 85 {
        return Err(ErrorCode::InvalidRequest);
    }
    private_path(root, true)
}

pub fn read_private(path: &Path, limit: u64) -> Result<Zeroizing<Vec<u8>>, ErrorCode> {
    private_path(path, false)?;
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    { use std::os::unix::fs::OpenOptionsExt; options.custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC); }
    let file = options.open(path).map_err(|_| ErrorCode::Unavailable)?;
    if file.metadata().map_err(|_| ErrorCode::Unavailable)?.len() > limit { return Err(ErrorCode::Unavailable); }
    let mut bytes = Zeroizing::new(Vec::new());
    file.take(limit + 1).read_to_end(&mut bytes).map_err(|_| ErrorCode::Unavailable)?;
    if bytes.len() as u64 > limit { return Err(ErrorCode::Unavailable); }
    Ok(bytes)
}

fn lock(root: &Path) -> Result<File, ErrorCode> {
    let path = root.join("daemon.lock");
    if path.exists() { private_path(&path, false)?; }
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true);
    #[cfg(unix)]
    { use std::os::unix::fs::OpenOptionsExt; options.mode(0o600).custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC); }
    let file = options.open(path).map_err(|_| ErrorCode::Unavailable)?;
    file.try_lock_exclusive().map_err(|_| ErrorCode::Busy)?;
    Ok(file)
}

/// Explicit setup only. Refuses any nonempty, unrecognized directory.
pub fn initialize(root: &Path) -> Result<Instance, ErrorCode> {
    initialize_with_key(root, create_key)
}

// Private dependency-injection seam for source-owned fixtures; no executable
// flag, protocol method or public API selects a fake key backend.
fn initialize_with_key(root: &Path, initialize_key: impl FnOnce(&Instance) -> Result<(), ErrorCode>) -> Result<Instance, ErrorCode> {
    if !root.is_absolute() || root.parent().is_none() { return Err(ErrorCode::InvalidRequest); }
    if !root.exists() {
        let mut builder = fs::DirBuilder::new();
        #[cfg(unix)]
        { use std::os::unix::fs::DirBuilderExt; builder.mode(0o700); }
        builder.create(root).map_err(|_| ErrorCode::Unavailable)?;
    }
    validate_root(root)?;
    if root.join("instance.json").exists() {
        return Ok(open(root)?.instance.clone());
    }
    for entry in fs::read_dir(root).map_err(|_| ErrorCode::Unavailable)? {
        if entry.map_err(|_| ErrorCode::Unavailable)?.file_name() != "daemon.lock" {
            return Err(ErrorCode::Conflict);
        }
    }
    let _lease = lock(root)?;
    // Recheck after the lease: another initializer may have won.
    if root.join("instance.json").exists() { return load_instance(root); }
    // Sync the entry naming the root, not only files inside it. This also runs
    // on a retry after an uncertain creation left an empty directory behind.
    magicvault_primitives::durable_io::sync_parent_dir_blocking(root)
        .map_err(|_| ErrorCode::PersistenceUncertain)?;
    let instance = Instance { format_version: 1, id: Uuid::new_v4() };
    initialize_key(&instance)?;
    let encoded = serde_json::to_vec(&instance).map_err(|_| ErrorCode::Unavailable)?;
    write_bytes_durably_with_mode_sync(&root.join("instance.json"), &encoded, Some(0o600))
        .map_err(|_| ErrorCode::PersistenceUncertain)?;
    Ok(instance)
}

fn load_instance(root: &Path) -> Result<Instance, ErrorCode> {
    let bytes = read_private(&root.join("instance.json"), 4096)?;
    let instance: Instance = serde_json::from_slice(&bytes).map_err(|_| ErrorCode::Unavailable)?;
    if instance.format_version != 1 { return Err(ErrorCode::Unavailable); }
    Ok(instance)
}

pub fn inspect_instance(root: &Path) -> Result<Instance, ErrorCode> {
    validate_root(root)?;
    load_instance(root)
}

/// Preserve core corruption evidence across daemon restarts. The shared core's
/// recovery policy is not changed for Magician; standalone requires explicit
/// operator reconciliation instead of treating a quarantined vault as empty.
pub fn validate_vault(root: &Path) -> Result<(), ErrorCode> {
    let vault = root.join("vault");
    match fs::symlink_metadata(&vault) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(_) => return Err(ErrorCode::Unavailable),
        Ok(_) => private_path(&vault, true)?,
    }
    for entry in fs::read_dir(&vault).map_err(|_| ErrorCode::Unavailable)? {
        let entry = entry.map_err(|_| ErrorCode::Unavailable)?;
        // Core's legacy append path may tighten/follow an existing journal.
        // Standalone must validate it before handing any path to that writer:
        // an imported symlink must never redirect writes/chmod outside this root.
        if entry.file_name() == magicvault_core::store::SECRET_AUDIT_FILENAME {
            private_path(&entry.path(), false)?;
        }
        if entry.file_name().to_string_lossy().contains(".corrupt-") {
            return Err(ErrorCode::Unavailable);
        }
        if entry.path().extension().is_some_and(|extension| extension == "vault") {
            private_path(&entry.path(), false)?;
            if entry.metadata().map_err(|_| ErrorCode::Unavailable)?.len() > 64 * 1024 * 1024 {
                return Err(ErrorCode::Capacity);
            }
        }
    }
    Ok(())
}

pub fn open(root: &Path) -> Result<Arc<InstanceLock>, ErrorCode> {
    validate_root(root)?;
    let file = lock(root)?;
    let instance = load_instance(root)?;
    Ok(Arc::new(InstanceLock { root: root.to_owned(), _file: file, instance }))
}

pub struct CachedKey(Zeroizing<[u8; 32]>);
impl MasterKeyProvider for CachedKey {
    fn get_or_create_key(&self) -> Result<[u8; 32], SecretEncryptionError> { Ok(*self.0) }
    fn delete_key(&self) -> Result<(), SecretEncryptionError> {
        Err(SecretEncryptionError::Keychain("standalone key rotation requires explicit recovery".into()))
    }
    fn provider_name(&self) -> &str { "magicvault_macos_keychain" }
}

#[cfg(target_os = "macos")]
fn key_entry(instance: &Instance) -> Result<keyring::Entry, ErrorCode> {
    keyring::Entry::new(KEYCHAIN_SERVICE, &format!("instance-{}", instance.id)).map_err(|_| ErrorCode::Unavailable)
}

#[cfg(target_os = "macos")]
fn create_key(instance: &Instance) -> Result<(), ErrorCode> {
    use rand::RngCore;
    let entry = key_entry(instance)?;
    match entry.get_password() {
        Err(keyring::Error::NoEntry) => {},
        Ok(value) => { let _guard = Zeroizing::new(value); return Err(ErrorCode::Conflict); },
        Err(_) => return Err(ErrorCode::Unavailable),
    }
    let mut key = Zeroizing::new([0u8; 32]);
    rand::rngs::OsRng.fill_bytes(&mut *key);
    let encoded = Zeroizing::new(hex::encode(*key));
    entry.set_password(&encoded).map_err(|_| ErrorCode::Unavailable)
}
#[cfg(not(target_os = "macos"))]
fn create_key(_: &Instance) -> Result<(), ErrorCode> { Err(ErrorCode::Unavailable) }

#[cfg(target_os = "macos")]
pub fn load_key(instance: &Instance) -> Result<CachedKey, ErrorCode> {
    // Missing entry is an outage, never permission to generate a replacement.
    let encoded = Zeroizing::new(key_entry(instance)?.get_password().map_err(|_| ErrorCode::Unavailable)?);
    let decoded = Zeroizing::new(hex::decode(&*encoded).map_err(|_| ErrorCode::Unavailable)?);
    let key: [u8; 32] = decoded.as_slice().try_into().map_err(|_| ErrorCode::Unavailable)?;
    Ok(CachedKey(Zeroizing::new(key)))
}
#[cfg(not(target_os = "macos"))]
pub fn load_key(_: &Instance) -> Result<CachedKey, ErrorCode> { Err(ErrorCode::Unavailable) }

#[cfg(all(test, unix))]
mod initialization_tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn key_failure_does_not_publish_identity_and_retry_recovers_only_the_empty_setup() {
        let parent = tempfile::tempdir().unwrap();
        let root = parent.path().join("vault");
        assert!(matches!(initialize_with_key(&root, |_| Err(ErrorCode::Unavailable)),Err(ErrorCode::Unavailable)));
        assert!(!root.join("instance.json").exists());
        assert_eq!(fs::metadata(&root).unwrap().permissions().mode() & 0o777,0o700);
        let created = initialize_with_key(&root, |_| Ok(())).unwrap();
        let existing = initialize_with_key(&root, |_| panic!("existing identity must never regenerate its key")).unwrap();
        assert_eq!(created.id,existing.id);
        assert_eq!(inspect_instance(&root).unwrap().id,created.id);
        assert_eq!(fs::metadata(root.join("instance.json")).unwrap().permissions().mode() & 0o777,0o600);
    }

    #[test]
    fn unrelated_state_is_never_adopted_or_sent_to_a_key_backend() {
        let root = tempfile::Builder::new().permissions(fs::Permissions::from_mode(0o700)).tempdir().unwrap();
        fs::write(root.path().join("retained"),b"existing state").unwrap();
        assert!(matches!(initialize_with_key(root.path(), |_| panic!("unrecognized root")),Err(ErrorCode::Conflict)));
        assert_eq!(fs::read(root.path().join("retained")).unwrap(),b"existing state");
    }
}
