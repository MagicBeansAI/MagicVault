//! User-only Chromium registration; a private executable copy needs no shell.
use super::*;
use magicvault_primitives::{durable_io::write_bytes_durably_with_mode_sync, private_fs};
use sha2::{Digest, Sha256};
use std::{fs, io::Read};
use winreg::{enums::*, RegKey};

const KEYS: [&str; 3] = [
    "Software\\Google\\Chrome\\NativeMessagingHosts\\ai.magicbeans.magicvault",
    "Software\\Chromium\\NativeMessagingHosts\\ai.magicbeans.magicvault",
    "Software\\Microsoft\\Edge\\NativeMessagingHosts\\ai.magicbeans.magicvault",
];
fn manifest_path(root: &Path) -> PathBuf {
    root.join("native-host-manifest.json")
}
fn launcher(root: &Path) -> PathBuf {
    root.join("native-host-launch.exe")
}
fn digest_path(root: &Path) -> PathBuf {
    root.join("native-host-copy.sha256")
}
fn expected(config: &HostConfig) -> Result<Vec<u8>, ErrorCode> {
    serde_json::to_vec_pretty(&serde_json::json!({
        "name": HOST_NAME, "description": "MagicVault credential delivery bridge",
        "path": launcher(&config.root), "type": "stdio",
        "allowed_origins": [format!("chrome-extension://{}/", config.extension_id)]
    }))
    .map_err(|_| ErrorCode::Unavailable)
}
fn registration(key: &str) -> Result<Option<String>, ErrorCode> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    match hkcu.open_subkey(key) {
        Ok(key) => key.get_value("").map(Some).map_err(|_| ErrorCode::Conflict),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err(ErrorCode::Unavailable),
    }
}
fn validate_registry(root: &Path) -> Result<Vec<String>, ErrorCode> {
    let desired = manifest_path(root)
        .to_str()
        .ok_or(ErrorCode::InvalidRequest)?
        .to_owned();
    let mut absent = Vec::new();
    for key in KEYS {
        match registration(key)? {
            Some(value) if value == desired => {}
            Some(_) => return Err(ErrorCode::Conflict),
            None => absent.push(key.to_owned()),
        }
    }
    Ok(absent)
}
fn lock(root: &Path) -> Result<fs::File, ErrorCode> {
    use fs2::FileExt;
    storage::inspect_instance(root)?;
    let path = root.join("native-install.lock");
    let file = if exists(&path)? {
        storage::private_path(&path, false)?;
        fs::OpenOptions::new().read(true).write(true).open(&path)
    } else {
        private_fs::create_file(&path, 0o600)
    }
    .map_err(|_| ErrorCode::Unavailable)?;
    file.try_lock_exclusive().map_err(|_| ErrorCode::Busy)?;
    Ok(file)
}
fn previous(root: &Path) -> Result<Option<HostConfig>, ErrorCode> {
    if !exists(&config_path(root))? {
        return Ok(None);
    }
    let config: HostConfig =
        serde_json::from_slice(&storage::read_private(&config_path(root), 8192)?)
            .map_err(|_| ErrorCode::Conflict)?;
    load_config(
        &config_path(root),
        &format!("chrome-extension://{}/", config.extension_id),
    )
    .map(Some)
}
fn binary(path: &Path) -> Result<Vec<u8>, ErrorCode> {
    // Source must be an installed private executable, not an arbitrary shared path.
    storage::private_path(path, false)?;
    let mut bytes = Vec::new();
    private_fs::read_only(path)
        .map_err(|_| ErrorCode::Unavailable)?
        .take(128 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| ErrorCode::Unavailable)?;
    if bytes.len() > 128 * 1024 * 1024 || !bytes.starts_with(b"MZ") {
        return Err(ErrorCode::InvalidRequest);
    }
    Ok(bytes)
}
fn validate_copy(root: &Path) -> Result<(), ErrorCode> {
    let bytes = binary(&launcher(root))?;
    if storage::read_private(&digest_path(root), 64)?.as_slice()
        != hex::encode(Sha256::digest(&bytes)).as_bytes()
    {
        return Err(ErrorCode::Conflict);
    }
    Ok(())
}
fn put(path: &Path, bytes: &[u8]) -> Result<(), ErrorCode> {
    write_bytes_durably_with_mode_sync(path, bytes, Some(0o600))
        .map_err(|_| ErrorCode::PersistenceUncertain)
}
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
    let _lock = lock(root)?;
    let config = HostConfig {
        version: BRIDGE_VERSION,
        root: root.to_owned(),
        instance_id: storage::inspect_instance(root)?.id,
        profile: profile.into(),
        extension_id: extension_id.into(),
        executable: executable.to_owned(),
    };
    let _pairing = hello(&config)?;
    let old = previous(root)?;
    if let Some(old) = &old {
        if old.root != config.root
            || old.profile != config.profile
            || old.executable != config.executable
        {
            return Err(ErrorCode::Conflict);
        }
        validate_copy(root)?;
    } else if [launcher(root), digest_path(root), manifest_path(root)]
        .iter()
        .any(|p| p.symlink_metadata().is_ok())
    {
        return Err(ErrorCode::Conflict);
    }
    validate_registry(root)?;
    let manifest = expected(&config)?;
    if exists(&manifest_path(root))? {
        let bytes = storage::read_private(&manifest_path(root), 16384)?;
        if bytes.as_slice() != manifest
            && old.as_ref().map(expected).transpose()?.as_deref() != Some(bytes.as_slice())
        {
            return Err(ErrorCode::Conflict);
        }
    }
    let bytes = binary(executable)?;
    let digest = hex::encode(Sha256::digest(&bytes));
    if old.is_none()
        || storage::read_private(&digest_path(root), 64)?.as_slice() != digest.as_bytes()
    {
        put(&launcher(root), &bytes)?;
        put(&digest_path(root), digest.as_bytes())?;
    }
    put(&manifest_path(root), &manifest)?;
    let target = manifest_path(root)
        .to_str()
        .ok_or(ErrorCode::InvalidRequest)?
        .to_owned();
    for key in KEYS {
        // Check again before each registry write; never overwrite another root.
        if registration(key)?
            .as_deref()
            .is_some_and(|value| value != target)
        {
            return Err(ErrorCode::Conflict);
        }
        let (key, _) = RegKey::predef(HKEY_CURRENT_USER)
            .create_subkey(key)
            .map_err(|_| ErrorCode::Unavailable)?;
        key.set_value("", &target)
            .map_err(|_| ErrorCode::PersistenceUncertain)?;
        if unsafe { windows_sys::Win32::System::Registry::RegFlushKey(key.raw_handle() as _) } != 0
        {
            return Err(ErrorCode::PersistenceUncertain);
        }
    }
    put(
        &config_path(root),
        &serde_json::to_vec(&config).map_err(|_| ErrorCode::Unavailable)?,
    )
}
pub fn inspect_registration(root: &Path) -> Result<Option<RegistrationInspection>, ErrorCode> {
    let Some(config) = previous(root)? else {
        return Ok(None);
    };
    let _pairing = hello(&config)?;
    let mut missing_files = Vec::new();
    for path in [
        launcher(root),
        digest_path(root),
        manifest_path(root),
        config.executable.clone(),
    ] {
        if !exists(&path)? {
            missing_files.push(path);
        }
    }
    if exists(&launcher(root))? && exists(&digest_path(root))? {
        validate_copy(root)?;
    }
    if exists(&manifest_path(root))?
        && storage::read_private(&manifest_path(root), 16384)?.as_slice()
            != expected(&config)?.as_slice()
    {
        return Err(ErrorCode::Conflict);
    }
    // Registry omissions use a non-secret registry path in the same advisory list.
    for key in validate_registry(root)? {
        missing_files.push(PathBuf::from(format!("HKCU\\{key}")));
    }
    Ok(Some(RegistrationInspection {
        extension_id: config.extension_id,
        client_profile: config.profile,
        missing_files,
    }))
}
pub fn remove(root: &Path) -> Result<(), ErrorCode> {
    let _lock = lock(root)?;
    let config = previous(root)?.ok_or(ErrorCode::Conflict)?;
    validate_copy(root)?;
    validate_registry(root)?;
    if storage::read_private(&manifest_path(root), 16384)?.as_slice()
        != expected(&config)?.as_slice()
    {
        return Err(ErrorCode::Conflict);
    }
    for key in KEYS {
        if registration(key)?.is_some() {
            // Remove only our default value and then the empty key, not a tree.
            let opened = RegKey::predef(HKEY_CURRENT_USER)
                .open_subkey_with_flags(key, KEY_READ | KEY_WRITE)
                .map_err(|_| ErrorCode::Unavailable)?;
            if opened.enum_values().count() != 1 || opened.enum_keys().next().is_some() {
                return Err(ErrorCode::Conflict);
            }
            opened
                .delete_value("")
                .map_err(|_| ErrorCode::Unavailable)?;
            RegKey::predef(HKEY_CURRENT_USER)
                .delete_subkey(key)
                .map_err(|_| ErrorCode::Unavailable)?;
        }
    }
    for path in [
        launcher(root),
        digest_path(root),
        manifest_path(root),
        config_path(root),
    ] {
        fs::remove_file(path).map_err(|_| ErrorCode::PersistenceUncertain)?;
    }
    Ok(())
}
