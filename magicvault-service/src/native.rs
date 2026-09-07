//! Native messaging installation and trusted bridge authentication. No enrolled
//! values, capabilities or raw native diagnostics are printed by this module.
use crate::storage;
use magicvault_effect::bridge::{self, BRIDGE_VERSION, HOST_NAME};
use magicvault_protocol::ErrorCode;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use uuid::Uuid;
use zeroize::Zeroize;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeHello {
    pub version: u32,
    pub instance_id: Uuid,
    pub extension_id: String,
    pub token: String,
}
impl Drop for NativeHello {
    fn drop(&mut self) {
        self.token.zeroize();
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeReady {
    pub version: u32,
    pub browser_handle: Uuid,
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
        version: BRIDGE_VERSION,
        instance_id: config.instance_id,
        extension_id: config.extension_id.clone(),
        token: pairing.token.clone(),
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
    let config = HostConfig {
        version: BRIDGE_VERSION,
        root: root.to_owned(),
        instance_id: storage::inspect_instance(root)?.id,
        profile: profile.into(),
        extension_id: extension_id.into(),
        executable: executable.to_owned(),
    };
    let _pairing = hello(&config)?;
    let config_file = config_path(root);
    let wrapper_file = root.join("native-host-launch");
    let manifests = manifests()?;
    for path in [&config_file, &wrapper_file]
        .into_iter()
        .chain(manifests.iter())
    {
        if exists(path)? {
            return Err(ErrorCode::Conflict);
        }
    }
    // Refuse replacement and retain partial publication for explicit repair.
    exclusive_write(
        &config_file,
        &serde_json::to_vec(&config).map_err(|_| ErrorCode::Unavailable)?,
        0o600,
    )?;
    exclusive_write(&wrapper_file, wrapper(&config)?.as_bytes(), 0o700)?;
    let bytes = manifest(&config)?;
    for path in manifests {
        let parent = path.parent().ok_or(ErrorCode::InvalidRequest)?;
        std::fs::create_dir_all(parent).map_err(|_| ErrorCode::Unavailable)?;
        exclusive_write(&path, &bytes, 0o600)?;
    }
    Ok(())
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
