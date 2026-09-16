//! Explicit systemd user service; never a root daemon or system service.
use crate::storage;
use magicvault_primitives::{durable_io::sync_parent_dir_blocking, private_fs};
use magicvault_protocol::ErrorCode;
use std::{
    io::Write,
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};

fn location(root: &Path) -> Result<(PathBuf, String), ErrorCode> {
    let instance = storage::inspect_instance(root)?;
    let unit = format!("magicvault-{}.service", instance.id);
    let home = PathBuf::from(std::env::var_os("HOME").ok_or(ErrorCode::Unavailable)?);
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".config"));
    if !config.is_absolute() {
        return Err(ErrorCode::InvalidRequest);
    }
    Ok((config.join("systemd/user").join(&unit), unit))
}
fn quote(path: &Path) -> Result<String, ErrorCode> {
    let s = path
        .to_str()
        .filter(|_| path.is_absolute())
        .ok_or(ErrorCode::InvalidRequest)?;
    if s.chars().any(char::is_control) {
        return Err(ErrorCode::InvalidRequest);
    }
    Ok(format!(
        "\"{}\"",
        s.replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('%', "%%")
            .replace('$', "$$")
    ))
}
fn definition(root: &Path, executable: &Path) -> Result<String, ErrorCode> {
    Ok(format!("# MagicVault managed user service v1\n[Unit]\nDescription=MagicVault private credential broker\nAfter=graphical-session.target\nPartOf=graphical-session.target\n\n[Service]\nType=simple\nExecStart={} --root {} serve\nRestart=no\nTimeoutStopSec=210\nUMask=0077\nStandardOutput=null\nStandardError=null\n\n[Install]\nWantedBy=graphical-session.target\n", quote(executable)?, quote(root)?))
}
fn record(root: &Path) -> PathBuf {
    root.join("service-executable.json")
}
fn managed(root: &Path) -> Result<(PathBuf, String), ErrorCode> {
    let executable: PathBuf = serde_json::from_slice(&storage::read_private(&record(root), 8192)?)
        .map_err(|_| ErrorCode::Conflict)?;
    let (path, unit) = location(root)?;
    if storage::read_private(&path, 16 * 1024)?.as_slice()
        != definition(root, &executable)?.as_bytes()
    {
        return Err(ErrorCode::Conflict);
    }
    Ok((path, unit))
}
pub fn install(root: &Path, executable: &Path) -> Result<PathBuf, ErrorCode> {
    let (path, _) = location(root)?;
    let text = definition(root, executable)?;
    if path.symlink_metadata().is_ok() || record(root).symlink_metadata().is_ok() {
        return Err(ErrorCode::Conflict);
    }
    std::fs::create_dir_all(path.parent().ok_or(ErrorCode::InvalidRequest)?)
        .map_err(|_| ErrorCode::Unavailable)?;
    let mut file = private_fs::create_file(&path, 0o600).map_err(|_| ErrorCode::Conflict)?;
    file.write_all(text.as_bytes())
        .and_then(|_| file.sync_all())
        .map_err(|_| ErrorCode::PersistenceUncertain)?;
    sync_parent_dir_blocking(&path).map_err(|_| ErrorCode::PersistenceUncertain)?;
    let mut file =
        private_fs::create_file(&record(root), 0o600).map_err(|_| ErrorCode::Conflict)?;
    file.write_all(&serde_json::to_vec(executable).map_err(|_| ErrorCode::InvalidRequest)?)
        .and_then(|_| file.sync_all())
        .map_err(|_| ErrorCode::PersistenceUncertain)?;
    sync_parent_dir_blocking(&record(root)).map_err(|_| ErrorCode::PersistenceUncertain)?;
    Ok(path)
}
async fn systemctl(args: &[&str]) -> Result<bool, ErrorCode> {
    let mut child = tokio::process::Command::new("/usr/bin/systemctl")
        .arg("--user")
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .map_err(|_| ErrorCode::Unavailable)?;
    match tokio::time::timeout(Duration::from_secs(210), child.wait()).await {
        Ok(Ok(status)) => Ok(status.success()),
        Ok(Err(_)) => Err(ErrorCode::Unavailable),
        Err(_) => {
            let _ = child.kill().await;
            let _ = child.wait().await;
            Err(ErrorCode::TransportUncertain)
        }
    }
}
pub async fn start(root: &Path) -> Result<(), ErrorCode> {
    let (_, unit) = managed(root)?;
    // The daemon's prompt needs the logged-in graphical session. Import only
    // these desktop variables; no credentials or arbitrary environment dump.
    let names = [
        "DISPLAY",
        "WAYLAND_DISPLAY",
        "XAUTHORITY",
        "DBUS_SESSION_BUS_ADDRESS",
        "XDG_RUNTIME_DIR",
    ];
    let mut args = vec!["import-environment"];
    args.extend(
        names
            .into_iter()
            .filter(|name| std::env::var_os(name).is_some()),
    );
    if args.len() > 1 && !systemctl(&args).await? {
        return Err(ErrorCode::Unavailable);
    }
    if !systemctl(&["daemon-reload"]).await? || !systemctl(&["enable", "--now", &unit]).await? {
        return Err(ErrorCode::Unavailable);
    }
    Ok(())
}
pub async fn stop(root: &Path) -> Result<(), ErrorCode> {
    let (_, unit) = managed(root)?;
    if systemctl(&["stop", &unit]).await? {
        Ok(())
    } else {
        Err(ErrorCode::Unavailable)
    }
}
pub async fn loaded(root: &Path) -> Result<bool, ErrorCode> {
    let (_, unit) = managed(root)?;
    systemctl(&["is-active", "--quiet", &unit]).await
}
pub fn verify_executable(root: &Path, executable: &Path) -> Result<Option<PathBuf>, ErrorCode> {
    let (path, _) = location(root)?;
    if !path.try_exists().map_err(|_| ErrorCode::Unavailable)?
        && !record(root)
            .try_exists()
            .map_err(|_| ErrorCode::Unavailable)?
    {
        return Ok(None);
    }
    managed(root)?;
    if storage::read_private(&path, 16 * 1024)?.as_slice()
        != definition(root, executable)?.as_bytes()
    {
        return Err(ErrorCode::Conflict);
    }
    Ok(Some(path))
}
pub fn remove(root: &Path) -> Result<(), ErrorCode> {
    let (path, unit) = managed(root)?;
    // Remove only the exact symlink made by enable; refuse a foreign override.
    let link = path
        .parent()
        .ok_or(ErrorCode::Unavailable)?
        .join("graphical-session.target.wants")
        .join(unit);
    if link.symlink_metadata().is_ok() {
        if link.canonicalize().map_err(|_| ErrorCode::Conflict)? != path {
            return Err(ErrorCode::Conflict);
        }
        std::fs::remove_file(&link).map_err(|_| ErrorCode::Unavailable)?;
        sync_parent_dir_blocking(&link).map_err(|_| ErrorCode::PersistenceUncertain)?;
    }
    for p in [path, record(root)] {
        std::fs::remove_file(&p).map_err(|_| ErrorCode::Unavailable)?;
        sync_parent_dir_blocking(&p).map_err(|_| ErrorCode::PersistenceUncertain)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unit_quotes_literal_paths_and_never_expands_specifiers() {
        let text = definition(
            Path::new("/tmp/vault%u$HOME"),
            Path::new("/tmp/app space/cli"),
        )
        .unwrap();
        assert!(
            text.contains("ExecStart=\"/tmp/app space/cli\" --root \"/tmp/vault%%u$$HOME\" serve")
        );
        assert!(definition(Path::new("/tmp/a\nExecStart=bad"), Path::new("/tmp/cli")).is_err());
    }
}
