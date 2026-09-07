//! Explicit user-session service setup. Never removes a vault or key.
use std::path::{Path, PathBuf};
use magicvault_protocol::ErrorCode;
use crate::storage;

pub fn plist(executable: &Path, root: &Path, label: &str) -> Result<String, ErrorCode> {
    if !executable.is_absolute() || !root.is_absolute() { return Err(ErrorCode::InvalidRequest); }
    let executable = executable.to_str().ok_or(ErrorCode::InvalidRequest)?;
    let root = root.to_str().ok_or(ErrorCode::InvalidRequest)?;
    fn xml(value: &str) -> String {
        value.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&apos;")
    }
    Ok(format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!-- MagicVault managed LaunchAgent v1; no credential values -->\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\"><dict>\n<key>Label</key><string>{}</string>\n<key>ProgramArguments</key><array><string>{}</string><string>--root</string><string>{}</string><string>serve</string></array>\n<key>RunAtLoad</key><true/>\n<key>KeepAlive</key><false/>\n<key>LimitLoadToSessionType</key><string>Aqua</string>\n<key>StandardOutPath</key><string>/dev/null</string>\n<key>StandardErrorPath</key><string>/dev/null</string>\n</dict></plist>\n", xml(label), xml(executable), xml(root)))
}

#[cfg(target_os = "macos")]
fn location(root: &Path) -> Result<(PathBuf, String), ErrorCode> {
    let instance = storage::inspect_instance(root)?;
    let label = format!("ai.magicbeans.magicvault.{}", instance.id);
    let user_root = PathBuf::from(std::env::var_os("HOME").ok_or(ErrorCode::Unavailable)?);
    if !user_root.is_absolute() { return Err(ErrorCode::InvalidRequest); }
    Ok((user_root.join("Library/LaunchAgents").join(format!("{label}.plist")), label))
}

#[cfg(target_os = "macos")]
pub fn install(root: &Path, executable: &Path) -> Result<PathBuf, ErrorCode> {
    use std::{fs::OpenOptions, io::Write, os::unix::fs::OpenOptionsExt};
    let (path, label) = location(root)?;
    let definition = plist(executable, root, &label)?;
    let parent = path.parent().ok_or(ErrorCode::Unavailable)?;
    std::fs::create_dir_all(parent).map_err(|_| ErrorCode::Unavailable)?;
    // Never overwrite an existing installation or another application's file.
    let mut file = OpenOptions::new().write(true).create_new(true).mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC).open(&path).map_err(|_| ErrorCode::Conflict)?;
    file.write_all(definition.as_bytes()).and_then(|_| file.sync_all()).map_err(|_| ErrorCode::PersistenceUncertain)?;
    magicvault_primitives::durable_io::sync_parent_dir_blocking(&path).map_err(|_| ErrorCode::PersistenceUncertain)?;
    Ok(path)
}

#[cfg(target_os = "macos")]
fn managed(root: &Path) -> Result<(PathBuf, String), ErrorCode> {
    let (path, label) = location(root)?;
    let bytes = storage::read_private(&path, 16 * 1024)?;
    let text = std::str::from_utf8(&bytes).map_err(|_| ErrorCode::Unavailable)?;
    if !text.contains("<!-- MagicVault managed LaunchAgent v1; no credential values -->")
        || !text.contains(&format!("<key>Label</key><string>{label}</string>")) {
        return Err(ErrorCode::Conflict);
    }
    Ok((path, label))
}

#[cfg(target_os = "macos")]
async fn launchctl(arguments: &[String]) -> Result<(), ErrorCode> {
    use std::{process::Stdio, time::Duration};
    let mut child = tokio::process::Command::new("/bin/launchctl").args(arguments)
        .stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).kill_on_drop(true)
        .spawn().map_err(|_| ErrorCode::Unavailable)?;
    match tokio::time::timeout(Duration::from_secs(10), child.wait()).await {
        Ok(Ok(status)) if status.success() => Ok(()),
        Ok(_) => Err(ErrorCode::Unavailable),
        Err(_) => { let _ = child.kill().await; let _ = child.wait().await; Err(ErrorCode::TransportUncertain) },
    }
}

/// Load/start the saved definition. No kickstart -k or forced process replacement.
#[cfg(target_os = "macos")]
pub async fn start(root: &Path) -> Result<(), ErrorCode> {
    let (path, _) = managed(root)?;
    launchctl(&["bootstrap".into(), format!("gui/{}", unsafe { libc::geteuid() }), path.to_str().ok_or(ErrorCode::InvalidRequest)?.into()]).await
}

/// Unload the owned user-session job; SIGTERM follows the daemon's drain path.
#[cfg(target_os = "macos")]
pub async fn stop(root: &Path) -> Result<(), ErrorCode> {
    let (_, label) = managed(root)?;
    launchctl(&["bootout".into(), format!("gui/{}/{label}", unsafe { libc::geteuid() })]).await
}

/// Remove only the managed definition. Does not claim to stop a running job.
#[cfg(target_os = "macos")]
pub fn remove(root: &Path) -> Result<(), ErrorCode> {
    let (path, _) = managed(root)?;
    std::fs::remove_file(&path).map_err(|_| ErrorCode::Unavailable)?;
    magicvault_primitives::durable_io::sync_parent_dir_blocking(&path).map_err(|_| ErrorCode::PersistenceUncertain)
}

/// Stronger ownership check for managed-bundle lifecycle operations. Legacy
/// low-level commands retain their contract; upgrades require an exact definition.
#[cfg(target_os = "macos")]
pub fn verify_executable(root: &Path, executable: &Path) -> Result<Option<PathBuf>, ErrorCode> {
    let (path, label) = location(root)?;
    verify_definition(&path, &label, root, executable)
}

#[cfg(any(target_os = "macos", test))]
fn verify_definition(path: &Path, label: &str, root: &Path, executable: &Path) -> Result<Option<PathBuf>, ErrorCode> {
    match path.symlink_metadata() {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(ErrorCode::Unavailable),
        Ok(_) => {}
    }
    if storage::read_private(path, 16 * 1024)?.as_slice() != plist(executable, root, label)?.as_bytes() {
        return Err(ErrorCode::Conflict);
    }
    Ok(Some(path.to_owned()))
}

/// Read-only launchd query, no output or environment-derived program execution.
#[cfg(target_os = "macos")]
pub async fn loaded(root: &Path) -> Result<bool, ErrorCode> {
    use std::{process::Stdio, time::Duration};
    let (_, label) = managed(root)?;
    let mut child = tokio::process::Command::new("/bin/launchctl")
        .args(["print", &format!("gui/{}/{label}", unsafe { libc::geteuid() })])
        .stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).kill_on_drop(true)
        .spawn().map_err(|_| ErrorCode::Unavailable)?;
    match tokio::time::timeout(Duration::from_secs(10), child.wait()).await {
        Ok(Ok(status)) => Ok(status.success()),
        Ok(Err(_)) => Err(ErrorCode::Unavailable),
        Err(_) => { let _ = child.kill().await; let _ = child.wait().await; Err(ErrorCode::TransportUncertain) }
    }
}

#[cfg(not(target_os = "macos"))]
pub fn verify_executable(_: &Path, _: &Path) -> Result<Option<PathBuf>, ErrorCode> { Err(ErrorCode::Unavailable) }
#[cfg(not(target_os = "macos"))]
pub async fn loaded(_: &Path) -> Result<bool, ErrorCode> { Err(ErrorCode::Unavailable) }

#[cfg(test)]
mod ownership_tests {
    use super::*;
    use std::{fs, os::unix::fs::{symlink, PermissionsExt}};

    #[test]
    fn lifecycle_requires_exact_private_definition_not_just_a_marker() {
        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("fixture.plist");
        let vault = Path::new("/tmp/synthetic-vault");
        let exe = Path::new("/tmp/synthetic-app/current/bin/magicvault");
        assert!(verify_definition(&file, "fixture", vault, exe).unwrap().is_none());
        fs::write(&file, plist(exe, vault, "fixture").unwrap()).unwrap();
        fs::set_permissions(&file, fs::Permissions::from_mode(0o600)).unwrap();
        assert!(verify_definition(&file, "fixture", vault, exe).unwrap().is_some());
        assert!(verify_definition(&file, "fixture", vault, Path::new("/tmp/other-executable")).is_err());
        fs::set_permissions(&file, fs::Permissions::from_mode(0o644)).unwrap();
        assert!(verify_definition(&file, "fixture", vault, exe).is_err());
        fs::remove_file(&file).unwrap();
        symlink(temp.path().join("missing"), &file).unwrap();
        assert!(verify_definition(&file, "fixture", vault, exe).is_err());
    }
}

#[cfg(not(target_os = "macos"))]
pub fn install(_: &Path, _: &Path) -> Result<PathBuf, ErrorCode> { Err(ErrorCode::Unavailable) }
#[cfg(not(target_os = "macos"))]
pub async fn start(_: &Path) -> Result<(), ErrorCode> { Err(ErrorCode::Unavailable) }
#[cfg(not(target_os = "macos"))]
pub async fn stop(_: &Path) -> Result<(), ErrorCode> { Err(ErrorCode::Unavailable) }
#[cfg(not(target_os = "macos"))]
pub fn remove(_: &Path) -> Result<(), ErrorCode> { Err(ErrorCode::Unavailable) }
