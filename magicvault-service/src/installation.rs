//! Private, immutable application bundles. This module never opens the keychain,
//! initializes a vault, starts a process or deletes application/vault contents.
//! Hashes detect corruption, not publisher identity; release signing is separate.
use crate::storage;
use fs2::FileExt;
use magicvault_primitives::durable_io::sync_parent_dir_blocking;
use magicvault_protocol::ErrorCode;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    os::unix::fs::{DirBuilderExt, OpenOptionsExt},
    path::{Component, Path, PathBuf},
};
use uuid::Uuid;

const FILES: &[&str] = &[
    "bin/magicvault",
    "bin/magicvault-mcp",
    "bin/magicvault-native-host",
    "extension/manifest.json",
    "extension/worker.js",
    "extension/options.html",
    "extension/options.js",
    "extension/options.css",
    "extension/fill.js",
    "examples/secure-fill.json",
    "examples/http-profile.json",
    "examples/process-profile.json",
    "LICENSE-MIT",
    "LICENSE-APACHE",
];
const MAX_BINARY: u64 = 128 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Bundle {
    pub format_version: u32,
    pub version: String,
    pub platform: String,
    pub files: BTreeMap<String, Artifact>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Artifact {
    pub sha256: String,
    pub bytes: u64,
    pub executable: bool,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Owner {
    format_version: u32,
    vault_root: PathBuf,
}

pub struct Installation {
    app: PathBuf,
    _lock: File,
}
pub struct Staged {
    relative: PathBuf,
    pub bundle: Bundle,
}

pub fn default_app_dir() -> Result<PathBuf, ErrorCode> {
    let home = std::env::var_os("HOME").ok_or(ErrorCode::Unavailable)?;
    canonical_leaf(&PathBuf::from(home).join(".magicvault-app"))
}

/// Resolve an existing parent, but never follow/adopt a symlink at the leaf.
pub fn canonical_leaf(path: &Path) -> Result<PathBuf, ErrorCode> {
    if !path.is_absolute()
        || path
            .components()
            .any(|c| !matches!(c, Component::RootDir | Component::Normal(_)))
    {
        return Err(ErrorCode::InvalidRequest);
    }
    let name = path.file_name().ok_or(ErrorCode::InvalidRequest)?;
    let parent = path
        .parent()
        .ok_or(ErrorCode::InvalidRequest)?
        .canonicalize()
        .map_err(|_| ErrorCode::Unavailable)?;
    let result = parent.join(name);
    if exists(&result)?
        && result
            .symlink_metadata()
            .map_err(|_| ErrorCode::Unavailable)?
            .file_type()
            .is_symlink()
    {
        return Err(ErrorCode::Conflict);
    }
    Ok(result)
}

pub fn bundled_source() -> Result<PathBuf, ErrorCode> {
    let exe = std::env::current_exe().map_err(|_| ErrorCode::Unavailable)?;
    let bin = exe.parent().ok_or(ErrorCode::Unavailable)?;
    if bin.file_name().and_then(|v| v.to_str()) != Some("bin") {
        return Err(ErrorCode::Unavailable);
    }
    Ok(bin.parent().ok_or(ErrorCode::Unavailable)?.to_owned())
}

fn exists(path: &Path) -> Result<bool, ErrorCode> {
    match path.symlink_metadata() {
        Ok(_) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(_) => Err(ErrorCode::Unavailable),
    }
}
fn directory(path: &Path) -> Result<(), ErrorCode> {
    fs::DirBuilder::new()
        .mode(0o700)
        .create(path)
        .map_err(|_| ErrorCode::Conflict)?;
    sync_parent_dir_blocking(path).map_err(|_| ErrorCode::PersistenceUncertain)
}
fn exclusive(path: &Path, bytes: &[u8], mode: u32) -> Result<(), ErrorCode> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(mode)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)
        .map_err(|_| ErrorCode::Conflict)?;
    file.write_all(bytes)
        .and_then(|_| file.sync_all())
        .map_err(|_| ErrorCode::PersistenceUncertain)?;
    sync_parent_dir_blocking(path).map_err(|_| ErrorCode::PersistenceUncertain)
}

fn source_file(root: &Path, relative: &str, max: u64) -> Result<File, ErrorCode> {
    // Every relative path is from the closed allowlist, never from arbitrary JSON.
    let path = root.join(relative);
    if let Some(parent) = Path::new(relative)
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
    {
        let meta = root
            .join(parent)
            .symlink_metadata()
            .map_err(|_| ErrorCode::Unavailable)?;
        if !meta.is_dir() || meta.file_type().is_symlink() {
            return Err(ErrorCode::Conflict);
        }
    }
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK)
        .open(path)
        .map_err(|_| ErrorCode::Unavailable)?;
    let meta = file.metadata().map_err(|_| ErrorCode::Unavailable)?;
    if !meta.is_file() || meta.len() > max {
        return Err(ErrorCode::InvalidRequest);
    }
    Ok(file)
}
fn manifest(root: &Path) -> Result<Bundle, ErrorCode> {
    let mut bytes = Vec::new();
    source_file(root, "bundle.json", 64 * 1024)?
        .take(64 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| ErrorCode::Unavailable)?;
    if bytes.len() > 64 * 1024 {
        return Err(ErrorCode::Capacity);
    }
    let bundle: Bundle = serde_json::from_slice(&bytes).map_err(|_| ErrorCode::InvalidRequest)?;
    if bundle.format_version != 1
        || bundle.platform != "darwin-arm64"
        || bundle.version.len() > 32
        || bundle.version.split('.').count() != 3
        || bundle
            .version
            .split('.')
            .any(|s| s.is_empty() || s.len() > 8 || !s.bytes().all(|b| b.is_ascii_digit()))
        || bundle.files.len() != FILES.len()
    {
        return Err(ErrorCode::InvalidRequest);
    }
    for name in FILES {
        let entry = bundle.files.get(*name).ok_or(ErrorCode::InvalidRequest)?;
        if entry.executable != name.starts_with("bin/")
            || entry.bytes == 0
            || entry.bytes
                > if entry.executable {
                    MAX_BINARY
                } else {
                    1024 * 1024
                }
            || entry.sha256.len() != 64
            || !entry
                .sha256
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(ErrorCode::InvalidRequest);
        }
    }
    Ok(bundle)
}

fn transfer(
    root: &Path,
    name: &str,
    entry: &Artifact,
    destination: Option<&Path>,
) -> Result<(), ErrorCode> {
    let mut input = source_file(root, name, entry.bytes)?;
    let mut output = destination
        .map(|p| {
            OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(if entry.executable { 0o700 } else { 0o600 })
                .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
                .open(p)
                .map_err(|_| ErrorCode::Conflict)
        })
        .transpose()?;
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    let mut total = 0;
    loop {
        let count = input
            .read(&mut buffer)
            .map_err(|_| ErrorCode::Unavailable)?;
        if count == 0 {
            break;
        }
        total += count as u64;
        if total > entry.bytes {
            return Err(ErrorCode::InvalidRequest);
        }
        hash.update(&buffer[..count]);
        if let Some(file) = output.as_mut() {
            file.write_all(&buffer[..count])
                .map_err(|_| ErrorCode::PersistenceUncertain)?;
        }
    }
    if total != entry.bytes || hex::encode(hash.finalize()) != entry.sha256 {
        return Err(ErrorCode::Conflict);
    }
    if let Some(file) = output {
        file.sync_all()
            .map_err(|_| ErrorCode::PersistenceUncertain)?;
    }
    Ok(())
}

impl Installation {
    /// The lock serializes setup/upgrade/uninstall, independently of daemon work.
    /// Unknown/partial app directories are never silently adopted or swept.
    pub fn open(app: &Path, vault_root: &Path, create: bool) -> Result<Self, ErrorCode> {
        let app = canonical_leaf(app)?;
        let vault_root = canonical_leaf(vault_root)?;
        if app.starts_with(&vault_root) || vault_root.starts_with(&app) {
            return Err(ErrorCode::InvalidRequest);
        }
        let fresh = !exists(&app)?;
        if fresh {
            if !create {
                return Err(ErrorCode::Unavailable);
            }
            directory(&app)?;
        }
        storage::private_path(&app, true)?;
        if !fresh {
            let owner: Owner = serde_json::from_slice(&storage::read_private(
                &app.join("installation.json"),
                4096,
            )?)
            .map_err(|_| ErrorCode::Conflict)?;
            if owner.format_version != 1 || owner.vault_root != vault_root {
                return Err(ErrorCode::Conflict);
            }
            storage::private_path(&app.join("install.lock"), false)?;
        }
        let lock_path = app.join("install.lock");
        if exists(&lock_path)? {
            storage::private_path(&lock_path, false)?;
        }
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(fresh)
            .truncate(false)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(&lock_path)
            .map_err(|_| ErrorCode::Unavailable)?;
        lock.try_lock_exclusive().map_err(|_| ErrorCode::Busy)?;
        if fresh {
            directory(&app.join("versions"))?;
            exclusive(
                &app.join("installation.json"),
                &serde_json::to_vec(&Owner {
                    format_version: 1,
                    vault_root: vault_root.clone(),
                })
                .map_err(|_| ErrorCode::Unavailable)?,
                0o600,
            )?;
        }
        let owner: Owner = serde_json::from_slice(&storage::read_private(
            &app.join("installation.json"),
            4096,
        )?)
        .map_err(|_| ErrorCode::Conflict)?;
        if owner.format_version != 1 || owner.vault_root != vault_root {
            return Err(ErrorCode::Conflict);
        }
        storage::private_path(&app.join("versions"), true)?;
        Ok(Self { app, _lock: lock })
    }
    pub fn directory(&self) -> &Path {
        &self.app
    }
    pub fn executable(&self, name: &str) -> Result<PathBuf, ErrorCode> {
        if !["magicvault", "magicvault-mcp", "magicvault-native-host"].contains(&name) {
            return Err(ErrorCode::InvalidRequest);
        }
        Ok(self.app.join("current/bin").join(name))
    }
    pub fn extension(&self) -> PathBuf {
        self.app.join("current/extension")
    }

    pub fn current(&self) -> Result<Option<Bundle>, ErrorCode> {
        let current = self.app.join("current");
        if !exists(&current)? {
            return Ok(None);
        }
        let target = fs::read_link(&current).map_err(|_| ErrorCode::Conflict)?;
        let components: Vec<_> = target.components().collect();
        if components.len() != 2
            || components[0] != Component::Normal("versions".as_ref())
            || !matches!(components[1], Component::Normal(_))
        {
            return Err(ErrorCode::Conflict);
        }
        let root = self.app.join(target);
        storage::private_path(&root, true)?;
        let bundle = manifest(&root)?;
        for (name, entry) in &bundle.files {
            storage::private_path(&root.join(name), false)?;
            transfer(&root, name, entry, None)?;
        }
        Ok(Some(bundle))
    }

    pub fn stage(&self, source: &Path, expected_version: &str) -> Result<Staged, ErrorCode> {
        let source = canonical_leaf(source)?;
        if source.starts_with(&self.app) {
            return Err(ErrorCode::Conflict);
        }
        let bundle = manifest(&source)?;
        if bundle.version != expected_version {
            return Err(ErrorCode::Conflict);
        }
        let relative =
            PathBuf::from("versions").join(format!("{}-{}", bundle.version, Uuid::new_v4()));
        let destination = self.app.join(&relative);
        directory(&destination)?;
        for child in ["bin", "extension", "examples"] {
            directory(&destination.join(child))?;
        }
        for (name, entry) in &bundle.files {
            transfer(&source, name, entry, Some(&destination.join(name)))?;
        }
        for child in ["bin", "extension", "examples"] {
            sync_parent_dir_blocking(&destination.join(child).join("unused"))
                .map_err(|_| ErrorCode::PersistenceUncertain)?;
        }
        // Completion marker is durable only after every file and directory.
        exclusive(
            &destination.join("bundle.json"),
            &serde_json::to_vec(&bundle).map_err(|_| ErrorCode::Unavailable)?,
            0o600,
        )?;
        Ok(Staged { relative, bundle })
    }

    /// Caller must drain its owned daemon before replacing a live installation.
    pub fn activate(&self, staged: Staged) -> Result<(), ErrorCode> {
        let target = self.app.join(&staged.relative);
        storage::private_path(&target, true)?;
        let bundle = manifest(&target)?;
        for (name, entry) in &bundle.files {
            transfer(&target, name, entry, None)?;
        }
        let temporary = self.app.join(format!(".activation-{}", Uuid::new_v4()));
        std::os::unix::fs::symlink(&staged.relative, &temporary)
            .map_err(|_| ErrorCode::Conflict)?;
        // Atomic symlink publication, not a byte-store write. Never follows the
        // destination. Keep complete old versions and ambiguous temps for repair.
        fs::rename(&temporary, self.app.join("current"))
            .map_err(|_| ErrorCode::PersistenceUncertain)?;
        sync_parent_dir_blocking(&self.app.join("current"))
            .map_err(|_| ErrorCode::PersistenceUncertain)
    }

    /// Recoverable app-only retirement, never recursive deletion or keychain I/O.
    /// Caller first unloads the exact owned service and removes its native bridge.
    pub fn retire(self) -> Result<PathBuf, ErrorCode> {
        let name = self
            .app
            .file_name()
            .and_then(|v| v.to_str())
            .ok_or(ErrorCode::InvalidRequest)?;
        let archive = self
            .app
            .with_file_name(format!("{name}.uninstalled-{}", Uuid::new_v4()));
        if exists(&archive)? {
            return Err(ErrorCode::Conflict);
        }
        fs::rename(&self.app, &archive).map_err(|_| ErrorCode::PersistenceUncertain)?;
        sync_parent_dir_blocking(&archive).map_err(|_| ErrorCode::PersistenceUncertain)?;
        Ok(archive)
    }
}

#[cfg(test)]
mod tests;
