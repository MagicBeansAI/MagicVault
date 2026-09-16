//! Private, immutable application bundles. This module never opens the keychain,
//! initializes a vault, starts a process or deletes application/vault contents.
//! Hashes detect corruption, not publisher identity; release signing is separate.
use crate::storage;
use fs2::FileExt;
use magicvault_primitives::durable_io::sync_parent_dir_blocking;
use magicvault_primitives::private_fs;
use magicvault_protocol::ErrorCode;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Component, Path, PathBuf},
};
use uuid::Uuid;

#[cfg(not(windows))]
const FILES: &[&str] = &[
    "bin/magicvault",
    "bin/magicvault-mcp",
    "bin/magicvault-native-host",
    "bin/magicvault-prompt",
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
#[cfg(windows)]
const FILES: &[&str] = &[
    "bin/magicvault.exe",
    "bin/magicvault-mcp.exe",
    "bin/magicvault-native-host.exe",
    "bin/magicvault-prompt.exe",
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
pub fn platform() -> &'static str {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => "darwin-arm64",
        ("macos", "x86_64") => "darwin-x64",
        ("linux", "aarch64") => "linux-arm64",
        ("linux", "x86_64") => "linux-x64",
        ("windows", "x86_64") => "win32-x64",
        ("windows", "aarch64") => "win32-arm64",
        _ => "unsupported",
    }
}
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
    let home = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
        .ok_or(ErrorCode::Unavailable)?;
    canonical_leaf(&PathBuf::from(home).join(".magicvault-app"))
}

/// Resolve an existing parent, but never follow/adopt a symlink at the leaf.
pub fn canonical_leaf(path: &Path) -> Result<PathBuf, ErrorCode> {
    if !path.is_absolute()
        || path.components().any(|c| {
            !matches!(
                c,
                Component::RootDir | Component::Normal(_) | Component::Prefix(_)
            )
        })
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
    private_fs::create_dir(path).map_err(|_| ErrorCode::Conflict)?;
    sync_parent_dir_blocking(path).map_err(|_| ErrorCode::PersistenceUncertain)
}
fn exclusive(path: &Path, bytes: &[u8], mode: u32) -> Result<(), ErrorCode> {
    let mut file = private_fs::create_file(path, mode).map_err(|_| ErrorCode::Conflict)?;
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
    #[cfg(unix)]
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK)
        .open(path)
        .map_err(|_| ErrorCode::Unavailable)?;
    #[cfg(windows)]
    let file = private_fs::read_only(&path).map_err(|_| ErrorCode::Unavailable)?;
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
        || bundle.platform != platform()
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
            private_fs::create_file(p, if entry.executable { 0o700 } else { 0o600 })
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
        #[cfg(unix)]
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(fresh)
            .truncate(false)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(&lock_path)
            .map_err(|_| ErrorCode::Unavailable)?;
        #[cfg(windows)]
        let lock = if fresh {
            private_fs::create_file(&lock_path, 0o600)
        } else {
            OpenOptions::new().read(true).write(true).open(&lock_path)
        }
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
        if ![
            "magicvault",
            "magicvault-mcp",
            "magicvault-native-host",
            "magicvault-prompt",
        ]
        .contains(&name)
        {
            return Err(ErrorCode::InvalidRequest);
        }
        Ok(self
            .app
            .join("current/bin")
            .join(format!("{name}{}", std::env::consts::EXE_SUFFIX)))
    }
    pub fn extension(&self) -> PathBuf {
        self.app.join("current/extension")
    }

    pub fn current(&self) -> Result<Option<Bundle>, ErrorCode> {
        let current = self.app.join("current");
        if !exists(&current)? {
            return Ok(None);
        }
        #[cfg(unix)]
        let target = fs::read_link(&current).map_err(|_| ErrorCode::Conflict)?;
        #[cfg(windows)]
        let target = {
            let absolute = junction::get_target(&current).map_err(|_| ErrorCode::Conflict)?;
            // Canonicalization strips Win32/NT path-prefix spelling differences.
            let absolute = absolute.canonicalize().map_err(|_| ErrorCode::Conflict)?;
            absolute
                .strip_prefix(&self.app)
                .map_err(|_| ErrorCode::Conflict)?
                .to_owned()
        };
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
        #[cfg(unix)]
        std::os::unix::fs::symlink(&staged.relative, &temporary)
            .map_err(|_| ErrorCode::Conflict)?;
        #[cfg(windows)]
        {
            junction::create(&target, &temporary).map_err(|_| ErrorCode::Conflict)?;
            let current = self.app.join("current");
            if exists(&current)? {
                self.current()?; // Verify an owned version before removing only its junction.
                junction::delete(&current).map_err(|_| ErrorCode::PersistenceUncertain)?;
            }
            // Windows cannot atomically replace a directory junction. A failure
            // retains the complete staged version/junction for explicit recovery.
        }
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
        #[cfg(windows)]
        let current_relative = if self.current()?.is_some() {
            let target = junction::get_target(self.app.join("current"))
                .and_then(|p| p.canonicalize())
                .map_err(|_| ErrorCode::Conflict)?;
            Some(
                target
                    .strip_prefix(&self.app)
                    .map_err(|_| ErrorCode::Conflict)?
                    .to_owned(),
            )
        } else {
            None
        };
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
        #[cfg(windows)]
        if let Some(relative) = current_relative {
            // Junctions are absolute: keep the retired bundle self-contained.
            let current = archive.join("current");
            junction::delete(&current).map_err(|_| ErrorCode::PersistenceUncertain)?;
            junction::create(archive.join(relative), &current)
                .map_err(|_| ErrorCode::PersistenceUncertain)?;
        }
        sync_parent_dir_blocking(&archive).map_err(|_| ErrorCode::PersistenceUncertain)?;
        Ok(archive)
    }
}

#[cfg(all(test, unix))]
mod tests;

#[cfg(all(test, windows))]
mod windows_tests {
    use super::*;

    #[test]
    fn private_junction_activation_preserves_versions_and_retirement() {
        let temp = tempfile::tempdir().unwrap();
        let base = temp.path().canonicalize().unwrap();
        let source = base.join("bundle");
        fs::create_dir(&source).unwrap();
        for child in ["bin", "extension", "examples"] {
            fs::create_dir(source.join(child)).unwrap();
        }
        let mut files = BTreeMap::new();
        for name in FILES {
            let bytes = format!("synthetic {name}");
            fs::write(source.join(name), &bytes).unwrap();
            files.insert(
                (*name).into(),
                Artifact {
                    sha256: hex::encode(Sha256::digest(bytes.as_bytes())),
                    bytes: bytes.len() as u64,
                    executable: name.starts_with("bin/"),
                },
            );
        }
        fs::write(
            source.join("bundle.json"),
            serde_json::to_vec(&Bundle {
                format_version: 1,
                version: "0.9.0".into(),
                platform: platform().into(),
                files,
            })
            .unwrap(),
        )
        .unwrap();
        let root = base.join("app");
        let vault = base.join("vault");
        let app = Installation::open(&root, &vault, true).unwrap();
        assert!(matches!(
            Installation::open(&root, &vault, true),
            Err(ErrorCode::Busy)
        ));
        let staged = app.stage(&source, "0.9.0").unwrap();
        let first = root.join(&staged.relative);
        app.activate(staged).unwrap();
        let executable = app.executable("magicvault").unwrap();
        assert!(executable.ends_with("magicvault.exe"));
        storage::private_path(&executable, false).unwrap();
        assert_eq!(app.current().unwrap().unwrap().platform, platform());
        app.activate(app.stage(&source, "0.9.0").unwrap()).unwrap();
        assert!(first.is_dir());
        let current = junction::get_target(root.join("current")).unwrap();
        fs::write(source.join("bin/magicvault.exe"), "tampered source").unwrap();
        assert!(app.stage(&source, "0.9.0").is_err());
        assert_eq!(junction::get_target(root.join("current")).unwrap(), current);
        fs::create_dir(&vault).unwrap();
        fs::write(vault.join("sentinel"), "keep").unwrap();
        let archived = app.retire().unwrap();
        assert!(!root.exists());
        assert!(archived.join("current/bin/magicvault.exe").is_file());
        assert_eq!(fs::read(vault.join("sentinel")).unwrap(), b"keep");
    }
}
