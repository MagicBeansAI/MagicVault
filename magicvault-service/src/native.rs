//! Native messaging installation and trusted bridge authentication. No enrolled
//! values, capabilities or raw native diagnostics are printed by this module.
use crate::storage;
use magicvault_effect::bridge::{self, BRIDGE_VERSION, HOST_NAME};
use magicvault_protocol::ErrorCode;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use uuid::Uuid;
use zeroize::Zeroize;

// Connection handshake only; effect commands and host configuration stay v1.
pub const NATIVE_VERSION: u32 = 2;
// Public unpacked-build identity, not a publisher signature or a secret.
pub const EXTENSION_ID: &str = "ljbccephkkklnlloibgcgcopcffbbcdc";

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BrowserHello {
    pub version: u32,
    pub profile_id: Uuid,
    pub capability: String,
    pub manual: bool,
}
impl BrowserHello {
    pub fn valid(&self) -> bool {
        self.version == NATIVE_VERSION
            && !self.profile_id.is_nil()
            && self.capability.len() == 64
            && self.capability.bytes().all(|b| b.is_ascii_hexdigit())
    }
}
impl Drop for BrowserHello {
    fn drop(&mut self) {
        self.capability.zeroize();
    }
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum NativeGreeting {
    Ready { version: u32, browser_handle: Uuid },
    Error { version: u32, code: ErrorCode },
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeHello {
    pub version: u32,
    pub instance_id: Uuid,
    pub extension_id: String,
    pub token: String,
    #[serde(default)]
    pub browser: Option<BrowserHello>,
}
impl Drop for NativeHello {
    fn drop(&mut self) {
        self.token.zeroize();
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostConfig {
    pub version: u32,
    pub root: PathBuf,
    pub instance_id: Uuid,
    pub profile: String,
    pub extension_id: String,
    pub executable: PathBuf,
}

pub fn config_path(root: &Path) -> PathBuf {
    root.join("native-host.json")
}

fn exists(path: &Path) -> Result<bool, ErrorCode> {
    match path.symlink_metadata() {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(_) => Err(ErrorCode::Unavailable),
    }
}

pub fn load_config(path: &Path, origin: &str) -> Result<HostConfig, ErrorCode> {
    let bytes = storage::read_private(path, 8192)?;
    let config: HostConfig = serde_json::from_slice(&bytes).map_err(|_| ErrorCode::Unavailable)?;
    // Accept the origin's two textual forms, never a path/query or another ID.
    // Manifest allowed_origins remains the canonical trailing-slash form.
    let expected_origin = format!("chrome-extension://{}", config.extension_id);
    if config.version != BRIDGE_VERSION
        || !bridge::valid_extension_id(&config.extension_id)
        || (origin != expected_origin && origin != format!("{expected_origin}/"))
        || !magicvault_protocol::valid_name(&config.profile)
        || config.instance_id != storage::inspect_instance(&config.root)?.id
        || config_path(&config.root) != path
    {
        return Err(ErrorCode::Unauthorized);
    }
    Ok(config)
}

pub fn hello(config: &HostConfig) -> Result<NativeHello, ErrorCode> {
    let bytes = storage::read_private(
        &config.root.join(format!("client-{}.json", config.profile)),
        4096,
    )?;
    let pairing: magicvault_protocol::Pairing =
        serde_json::from_slice(&bytes).map_err(|_| ErrorCode::Unauthorized)?;
    if pairing.token.len() != 64 || !pairing.token.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(ErrorCode::Unauthorized);
    }
    Ok(NativeHello {
        version: NATIVE_VERSION,
        instance_id: config.instance_id,
        extension_id: config.extension_id.clone(),
        token: pairing.token.clone(),
        browser: None,
    })
}

/// A manifest invokes a private wrapper because Chrome native host definitions
/// contain an executable path, not an argv array. Only paths enter the wrapper.
fn quote_shell(value: &str) -> Result<String, ErrorCode> {
    if value.chars().any(char::is_control) {
        return Err(ErrorCode::InvalidRequest);
    }
    Ok(format!("'{}'", value.replace('\'', "'\\''")))
}

pub fn wrapper(config: &HostConfig) -> Result<String, ErrorCode> {
    let exe = quote_shell(
        config
            .executable
            .to_str()
            .ok_or(ErrorCode::InvalidRequest)?,
    )?;
    let config = quote_shell(
        config_path(&config.root)
            .to_str()
            .ok_or(ErrorCode::InvalidRequest)?,
    )?;
    Ok(format!(
        "#!/bin/sh\nexec {exe} --config {config} -- \"$@\"\n"
    ))
}

pub fn manifest(config: &HostConfig) -> Result<Vec<u8>, ErrorCode> {
    serde_json::to_vec_pretty(&serde_json::json!({
        "name":HOST_NAME,"description":"MagicVault credential delivery bridge",
        "path":config.root.join("native-host-launch"),"type":"stdio",
        "allowed_origins":[format!("chrome-extension://{}/",config.extension_id)]
    }))
    .map_err(|_| ErrorCode::Unavailable)
}

#[cfg(target_os = "macos")]
fn manifests() -> Result<Vec<PathBuf>, ErrorCode> {
    let user_dir = std::env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .ok_or(ErrorCode::Unavailable)?;
    Ok(["Google/Chrome", "Chromium"]
        .iter()
        .map(|browser| {
            user_dir
                .join("Library/Application Support")
                .join(browser)
                .join("NativeMessagingHosts")
                .join(format!("{HOST_NAME}.json"))
        })
        .collect())
}

/// A read-only snapshot of this root's native registration, not an inventory of
/// installed browser extensions. Missing files are reported; foreign/unsafe
/// definitions fail closed. No lock, registration or pairing is created.
#[derive(Serialize)]
pub struct RegistrationInspection {
    pub extension_id: String,
    pub client_profile: String,
    pub missing_files: Vec<PathBuf>,
}

#[cfg(target_os = "macos")]
pub fn inspect_registration(root: &Path) -> Result<Option<RegistrationInspection>, ErrorCode> {
    if !exists(&config_path(root))? {
        return Ok(None);
    }
    inspect_definitions(root, &manifests()?)
}

#[cfg(not(target_os = "macos"))]
pub fn inspect_registration(_: &Path) -> Result<Option<RegistrationInspection>, ErrorCode> {
    Err(ErrorCode::Unavailable)
}

#[cfg(unix)]
fn inspect_definitions(
    root: &Path,
    manifests: &[PathBuf],
) -> Result<Option<RegistrationInspection>, ErrorCode> {
    use std::os::unix::fs::PermissionsExt;
    let path = config_path(root);
    if !exists(&path)? {
        return Ok(None);
    }
    let config: HostConfig = serde_json::from_slice(&storage::read_private(&path, 8192)?)
        .map_err(|_| ErrorCode::Unavailable)?;
    let config = load_config(
        &path,
        &format!("chrome-extension://{}/", config.extension_id),
    )?;
    let _pairing = hello(&config)?;
    let mut missing_files = Vec::new();
    if !config.executable.is_absolute() {
        return Err(ErrorCode::InvalidRequest);
    }
    if exists(&config.executable)? {
        validate_executable(&config.executable)?;
    } else {
        missing_files.push(config.executable.clone());
    }
    let expected_manifest = manifest(&config)?;
    let files = manifests
        .iter()
        .map(|p| (p.clone(), expected_manifest.clone(), 0o600))
        .chain(std::iter::once((
            root.join("native-host-launch"),
            wrapper(&config)?.into_bytes(),
            0o700,
        )));
    for (path, expected, mode) in files {
        if !exists(&path)? {
            missing_files.push(path);
            continue;
        }
        if storage::read_private(&path, 16384)?.as_slice() != expected.as_slice()
            || std::fs::symlink_metadata(&path)
                .map_err(|_| ErrorCode::Unavailable)?
                .permissions()
                .mode()
                & 0o777
                != mode
        {
            return Err(ErrorCode::Conflict);
        }
    }
    Ok(Some(RegistrationInspection {
        extension_id: config.extension_id,
        client_profile: config.profile,
        missing_files,
    }))
}

#[cfg(unix)]
fn validate_executable(executable: &Path) -> Result<(), ErrorCode> {
    let metadata = std::fs::symlink_metadata(executable).map_err(|_| ErrorCode::Unavailable)?;
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || metadata.uid() != unsafe { libc::geteuid() }
        || metadata.permissions().mode() & 0o022 != 0
        || metadata.permissions().mode() & 0o111 == 0
    {
        return Err(ErrorCode::Unavailable);
    }
    Ok(())
}

#[cfg(target_os = "macos")]
pub fn install(
    root: &Path,
    profile: &str,
    extension_id: &str,
    executable: &Path,
) -> Result<(), ErrorCode> {
    if !magicvault_protocol::valid_name(profile)
        || !bridge::valid_extension_id(extension_id)
        || !executable.is_absolute()
    {
        return Err(ErrorCode::InvalidRequest);
    }
    validate_executable(executable)?;
    let config = HostConfig {
        version: BRIDGE_VERSION,
        root: root.to_owned(),
        instance_id: storage::inspect_instance(root)?.id,
        profile: profile.into(),
        extension_id: extension_id.into(),
        executable: executable.to_owned(),
    };
    let _pairing = hello(&config)?;
    install_definitions(&config, &manifests()?)
}

/// Publish only exact managed definitions. Existing custom identities can move
/// to the bundled identity, but another root/client/executable is a conflict.
/// Config is last: interruption may deny connections, never broaden authority.
#[cfg(unix)]
fn install_definitions(config: &HostConfig, manifests: &[PathBuf]) -> Result<(), ErrorCode> {
    let _lock = definition_lock(&config.root)?;
    let config_file = config_path(&config.root);
    let previous = if exists(&config_file)? {
        let old = load_config(
            &config_file,
            &format!(
                "chrome-extension://{}/",
                serde_json::from_slice::<HostConfig>(&storage::read_private(&config_file, 8192)?)
                    .map_err(|_| ErrorCode::Conflict)?
                    .extension_id
            ),
        )?;
        if old.root != config.root
            || old.instance_id != config.instance_id
            || old.profile != config.profile
            || old.executable != config.executable
        {
            return Err(ErrorCode::Conflict);
        }
        Some(old)
    } else {
        None
    };
    let desired_manifest = manifest(config)?;
    let previous_manifest = previous.as_ref().map(manifest).transpose()?;
    let mut files = vec![(
        config.root.join("native-host-launch"),
        wrapper(config)?.into_bytes(),
        None,
        0o700,
    )];
    files.extend(manifests.iter().map(|p| {
        (
            p.clone(),
            desired_manifest.clone(),
            previous_manifest.clone(),
            0o600,
        )
    }));
    files.push((
        config_file,
        serde_json::to_vec(config).map_err(|_| ErrorCode::Unavailable)?,
        previous
            .as_ref()
            .map(serde_json::to_vec)
            .transpose()
            .map_err(|_| ErrorCode::Unavailable)?,
        0o600,
    ));
    // Preflight every file before modifying anything; permit either generation
    // for recovery from an interrupted identity migration, never foreign bytes.
    for (path, desired, old, mode) in &files {
        if exists(path)? {
            let bytes = storage::read_private(path, 16384)?;
            use std::os::unix::fs::PermissionsExt;
            if (bytes.as_slice() != desired.as_slice() && old.as_deref() != Some(bytes.as_slice()))
                || std::fs::symlink_metadata(path)
                    .map_err(|_| ErrorCode::Unavailable)?
                    .permissions()
                    .mode()
                    & 0o777
                    != *mode
            {
                return Err(ErrorCode::Conflict);
            }
        }
    }
    for (path, desired, old, mode) in files {
        std::fs::create_dir_all(path.parent().ok_or(ErrorCode::InvalidRequest)?)
            .map_err(|_| ErrorCode::Unavailable)?;
        if exists(&path)? {
            let bytes = storage::read_private(&path, 16384)?;
            if bytes.as_slice() == desired.as_slice() {
                continue;
            }
            if old.as_deref() != Some(bytes.as_slice()) {
                return Err(ErrorCode::Conflict);
            }
            magicvault_primitives::durable_io::write_bytes_durably_with_mode_sync(
                &path,
                &desired,
                Some(mode),
            )
            .map_err(|_| ErrorCode::PersistenceUncertain)?;
        } else {
            exclusive_write(&path, &desired, mode)?;
        }
    }
    Ok(())
}

#[cfg(unix)]
fn definition_lock(root: &Path) -> Result<std::fs::File, ErrorCode> {
    use fs2::FileExt;
    use std::{
        fs::OpenOptions,
        os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    };
    storage::inspect_instance(root)?;
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK)
        .open(root.join("native-install.lock"))
        .map_err(|_| ErrorCode::Unavailable)?;
    let metadata = file.metadata().map_err(|_| ErrorCode::Unavailable)?;
    if !metadata.is_file()
        || metadata.uid() != unsafe { libc::geteuid() }
        || metadata.permissions().mode() & 0o077 != 0
    {
        return Err(ErrorCode::Unavailable);
    }
    file.try_lock_exclusive().map_err(|_| ErrorCode::Busy)?;
    Ok(file)
}

#[cfg(unix)]
fn exclusive_write(path: &Path, bytes: &[u8], mode: u32) -> Result<(), ErrorCode> {
    use std::{fs::OpenOptions, io::Write, os::unix::fs::OpenOptionsExt};
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
    magicvault_primitives::durable_io::sync_parent_dir_blocking(path)
        .map_err(|_| ErrorCode::PersistenceUncertain)
}

#[cfg(target_os = "macos")]
pub fn remove(root: &Path) -> Result<(), ErrorCode> {
    let _lock = definition_lock(root)?;
    let config_file = config_path(root);
    let bytes = storage::read_private(&config_file, 8192)?;
    let config: HostConfig = serde_json::from_slice(&bytes).map_err(|_| ErrorCode::Unavailable)?;
    if config.root != root
        || config.instance_id != storage::inspect_instance(root)?.id
        || config.version != BRIDGE_VERSION
        || !bridge::valid_extension_id(&config.extension_id)
    {
        return Err(ErrorCode::Conflict);
    }
    let expected_manifest = manifest(&config)?;
    let expected_wrapper = wrapper(&config)?;
    let wrapper_file = root.join("native-host-launch");
    let files = manifests()?
        .into_iter()
        .map(|p| (p, expected_manifest.clone()))
        .chain(std::iter::once((
            wrapper_file,
            expected_wrapper.into_bytes(),
        )))
        .collect::<Vec<_>>();
    // Validate every exact file before deleting anything. Never remove another
    // installation's manifest, modified file, symlink, vault or pairing.
    for (path, expected) in &files {
        if exists(path)? && storage::read_private(path, 16384)?.as_slice() != expected.as_slice() {
            return Err(ErrorCode::Conflict);
        }
    }
    for (path, expected) in files {
        if exists(&path)? {
            if storage::read_private(&path, 16384)?.as_slice() != expected.as_slice() {
                return Err(ErrorCode::Conflict);
            }
            std::fs::remove_file(&path).map_err(|_| ErrorCode::Unavailable)?;
            magicvault_primitives::durable_io::sync_parent_dir_blocking(&path)
                .map_err(|_| ErrorCode::PersistenceUncertain)?;
        }
    }
    std::fs::remove_file(&config_file).map_err(|_| ErrorCode::Unavailable)?;
    magicvault_primitives::durable_io::sync_parent_dir_blocking(&config_file)
        .map_err(|_| ErrorCode::PersistenceUncertain)
}

#[cfg(not(target_os = "macos"))]
pub fn install(_: &Path, _: &str, _: &str, _: &Path) -> Result<(), ErrorCode> {
    Err(ErrorCode::Unavailable)
}
#[cfg(not(target_os = "macos"))]
pub fn remove(_: &Path) -> Result<(), ErrorCode> {
    Err(ErrorCode::Unavailable)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    fn definitions_fixture() -> (tempfile::TempDir, HostConfig, Vec<PathBuf>) {
        use std::{fs, os::unix::fs::PermissionsExt};
        let root = tempfile::Builder::new()
            .permissions(fs::Permissions::from_mode(0o700))
            .tempdir()
            .unwrap();
        let instance = storage::Instance {
            format_version: 1,
            id: Uuid::new_v4(),
        };
        exclusive_write(
            &root.path().join("instance.json"),
            &serde_json::to_vec(&instance).unwrap(),
            0o600,
        )
        .unwrap();
        let config = HostConfig {
            version: BRIDGE_VERSION,
            root: root.path().to_owned(),
            instance_id: instance.id,
            profile: "fixture".into(),
            extension_id: "a".repeat(32),
            executable: "/tmp/synthetic-native-host".into(),
        };
        let paths = vec![
            root.path().join("browser-a/native.json"),
            root.path().join("browser-b/native.json"),
        ];
        (root, config, paths)
    }

    #[cfg(unix)]
    #[test]
    fn managed_definitions_are_idempotent_and_identity_migration_recovers_partial_publication() {
        let (_root, mut config, paths) = definitions_fixture();
        install_definitions(&config, &paths).unwrap();
        install_definitions(&config, &paths).unwrap();
        config.extension_id = EXTENSION_ID.into();
        // A prior migration published one manifest, but not its config.
        std::fs::write(&paths[0], manifest(&config).unwrap()).unwrap();
        install_definitions(&config, &paths).unwrap();
        for path in &paths {
            assert_eq!(
                storage::read_private(path, 16384).unwrap().as_slice(),
                manifest(&config).unwrap()
            );
        }
        assert!(load_config(
            &config_path(&config.root),
            &format!("chrome-extension://{EXTENSION_ID}/")
        )
        .is_ok());
        assert!(load_config(
            &config_path(&config.root),
            &format!("chrome-extension://{}/", "a".repeat(32))
        )
        .is_err());
        install_definitions(&config, &paths).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn foreign_modified_and_other_client_definitions_are_never_overwritten() {
        let (_root, mut config, paths) = definitions_fixture();
        install_definitions(&config, &paths).unwrap();
        std::fs::write(&paths[1], b"foreign-definition").unwrap();
        config.extension_id = EXTENSION_ID.into();
        let before = storage::read_private(&paths[0], 16384).unwrap();
        assert_eq!(
            install_definitions(&config, &paths),
            Err(ErrorCode::Conflict)
        );
        assert_eq!(std::fs::read(&paths[1]).unwrap(), b"foreign-definition");
        assert_eq!(
            storage::read_private(&paths[0], 16384).unwrap().as_slice(),
            before.as_slice()
        );
        config.profile = "different-client".into();
        assert_eq!(
            install_definitions(&config, &paths),
            Err(ErrorCode::Conflict)
        );
    }

    #[cfg(unix)]
    #[test]
    fn missing_managed_files_are_repaired_but_locks_and_symlinks_fail_closed() {
        let (_root, config, paths) = definitions_fixture();
        let held = definition_lock(&config.root).unwrap();
        assert_eq!(install_definitions(&config, &paths), Err(ErrorCode::Busy));
        drop(held);
        install_definitions(&config, &paths).unwrap();
        std::fs::remove_file(&paths[0]).unwrap();
        install_definitions(&config, &paths).unwrap();
        std::fs::remove_file(&paths[0]).unwrap();
        std::os::unix::fs::symlink(&paths[1], &paths[0]).unwrap();
        assert!(install_definitions(&config, &paths).is_err());
    }

    #[cfg(unix)]
    fn inspection_fixture() -> (tempfile::TempDir, HostConfig, Vec<PathBuf>) {
        let (root, mut config, paths) = definitions_fixture();
        config.executable = root.path().join("synthetic-native-host");
        exclusive_write(
            &config.executable,
            b"synthetic executable; never run",
            0o700,
        )
        .unwrap();
        exclusive_write(
            &root.path().join("client-fixture.json"),
            &serde_json::to_vec(&magicvault_protocol::Pairing {
                client_id: Uuid::new_v4(),
                token: "c".repeat(64),
            })
            .unwrap(),
            0o600,
        )
        .unwrap();
        (root, config, paths)
    }

    #[cfg(unix)]
    #[test]
    fn inspection_reports_missing_and_complete_registration_without_writes() {
        let (root, config, paths) = inspection_fixture();
        assert!(inspect_definitions(root.path(), &paths).unwrap().is_none());
        install_definitions(&config, &paths).unwrap();
        // Inspection must not create or acquire the installer's lock.
        std::fs::remove_file(root.path().join("native-install.lock")).unwrap();
        let report = inspect_definitions(root.path(), &paths).unwrap().unwrap();
        assert!(report.missing_files.is_empty());
        assert_eq!(report.extension_id, config.extension_id);
        assert_eq!(report.client_profile, "fixture");
        assert!(!serde_json::to_string(&report)
            .unwrap()
            .contains(&"c".repeat(64)));
        std::fs::remove_file(&paths[0]).unwrap();
        std::fs::remove_file(&config.executable).unwrap();
        let report = inspect_definitions(root.path(), &paths).unwrap().unwrap();
        assert_eq!(
            report.missing_files,
            vec![config.executable, paths[0].clone()]
        );
        assert!(!paths[0].exists());
        assert!(!root.path().join("native-install.lock").exists());
    }

    #[cfg(unix)]
    #[test]
    fn inspection_rejects_foreign_unsafe_and_broken_registration_without_repair() {
        use std::{fs, os::unix::fs::PermissionsExt};
        let (root, config, paths) = inspection_fixture();
        install_definitions(&config, &paths).unwrap();
        fs::write(&paths[0], b"foreign manifest").unwrap();
        assert!(matches!(
            inspect_definitions(root.path(), &paths),
            Err(ErrorCode::Conflict)
        ));
        assert_eq!(fs::read(&paths[0]).unwrap(), b"foreign manifest");
        fs::write(&paths[0], manifest(&config).unwrap()).unwrap();
        let wrapper = root.path().join("native-host-launch");
        fs::set_permissions(&wrapper, fs::Permissions::from_mode(0o600)).unwrap();
        assert!(inspect_definitions(root.path(), &paths).is_err());
        fs::set_permissions(&wrapper, fs::Permissions::from_mode(0o700)).unwrap();
        fs::set_permissions(&config.executable, fs::Permissions::from_mode(0o722)).unwrap();
        assert!(inspect_definitions(root.path(), &paths).is_err());
        fs::set_permissions(&config.executable, fs::Permissions::from_mode(0o700)).unwrap();
        fs::remove_file(&paths[0]).unwrap();
        std::os::unix::fs::symlink(&paths[1], &paths[0]).unwrap();
        assert!(inspect_definitions(root.path(), &paths).is_err());
        fs::remove_file(&paths[0]).unwrap();
        exclusive_write(&paths[0], &manifest(&config).unwrap(), 0o600).unwrap();
        fs::write(root.path().join("client-fixture.json"), b"broken pairing").unwrap();
        assert!(inspect_definitions(root.path(), &paths).is_err());
    }

    #[test]
    fn native_definitions_are_identity_scoped_quoted_and_capability_free() {
        let config = HostConfig {
            version: BRIDGE_VERSION,
            root: PathBuf::from("/tmp/mv fixture"),
            instance_id: Uuid::nil(),
            profile: "agent".into(),
            extension_id: "a".repeat(32),
            executable: PathBuf::from("/tmp/host's folder/native-host"),
        };
        let script = wrapper(&config).unwrap();
        assert!(script.contains("'\\''"));
        assert!(script.contains("\"$@\""));
        assert!(!script.contains("token"));
        let value: serde_json::Value = serde_json::from_slice(&manifest(&config).unwrap()).unwrap();
        assert_eq!(
            value["allowed_origins"],
            serde_json::json!([format!("chrome-extension://{}/", config.extension_id)])
        );
        assert_eq!(value["type"], "stdio");
        assert!(value.get("token").is_none());
        assert!(quote_shell("/tmp/bad\npath").is_err());
        assert!(!bridge::valid_extension_id("*"));
        assert!(!bridge::valid_extension_id(&"z".repeat(32)));
    }

    #[cfg(unix)]
    #[test]
    fn host_configuration_refuses_wrong_origin_instance_and_permissions() {
        use std::{fs, os::unix::fs::PermissionsExt};
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
        let mut config = HostConfig {
            version: BRIDGE_VERSION,
            root: root.path().to_owned(),
            instance_id: instance.id,
            profile: "agent".into(),
            extension_id: "a".repeat(32),
            executable: "/tmp/synthetic-native-host".into(),
        };
        let file = config_path(root.path());
        fs::write(&file, serde_json::to_vec(&config).unwrap()).unwrap();
        fs::set_permissions(&file, fs::Permissions::from_mode(0o600)).unwrap();
        let origin = format!("chrome-extension://{}/", config.extension_id);
        assert!(load_config(&file, &origin).is_ok());
        assert!(load_config(&file, origin.trim_end_matches('/')).is_ok());
        for suffix in ["path", "?query", "#fragment", "/"] {
            assert!(load_config(&file, &format!("{origin}{suffix}")).is_err());
        }
        assert!(load_config(&file, &format!("chrome-extension://{}/", "b".repeat(32))).is_err());
        assert!(load_config(&file, "https://example.com/").is_err());
        fs::set_permissions(&file, fs::Permissions::from_mode(0o644)).unwrap();
        assert!(load_config(&file, &origin).is_err());
        fs::set_permissions(&file, fs::Permissions::from_mode(0o600)).unwrap();
        config.instance_id = Uuid::new_v4();
        fs::write(&file, serde_json::to_vec(&config).unwrap()).unwrap();
        assert!(load_config(&file, &origin).is_err());
    }
}
