//! Per-user interactive scheduled task. Graceful stop uses a private named event.
use crate::{client::Client, storage};
use magicvault_primitives::{
    private_fs,
    windows::{wide, PrivateSecurity},
};
use magicvault_protocol::ErrorCode;
use serde::{Deserialize, Serialize};
use std::{
    io::{Read, Write},
    os::windows::{
        io::{AsRawHandle, FromRawHandle, OwnedHandle},
        process::CommandExt,
    },
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};
use tokio_util::sync::CancellationToken;
use windows_sys::Win32::{Foundation::*, System::Threading::*};

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    executable: PathBuf,
    exported_xml: Vec<u8>,
}
fn record(root: &Path) -> PathBuf {
    root.join("service-task.json")
}
fn task_name(root: &Path) -> Result<String, ErrorCode> {
    Ok(format!(
        "MagicVault-{}",
        storage::inspect_instance(root)?.id
    ))
}
fn xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}
fn literal_path(path: &Path) -> Result<&str, ErrorCode> {
    let text = path
        .to_str()
        .filter(|_| path.is_absolute())
        .ok_or(ErrorCode::InvalidRequest)?;
    // Task Scheduler expands percent-delimited variables without a shell.
    if text.chars().any(char::is_control) || text.contains(['"', '%']) {
        return Err(ErrorCode::InvalidRequest);
    }
    Ok(text)
}
fn argument(path: &Path) -> Result<String, ErrorCode> {
    let text = literal_path(path)?;
    // Windows command-line quoting doubles trailing slashes before the quote.
    let trailing = text.chars().rev().take_while(|c| *c == '\\').count();
    Ok(format!("\"{}{}\"", text, "\\".repeat(trailing)))
}
fn scheduler() -> Result<PathBuf, ErrorCode> {
    let root = PathBuf::from(std::env::var_os("SystemRoot").ok_or(ErrorCode::Unavailable)?);
    if !root.is_absolute() {
        return Err(ErrorCode::Unavailable);
    }
    Ok(root.join("System32/schtasks.exe"))
}
fn run(args: &[&str]) -> Result<(bool, Vec<u8>), ErrorCode> {
    let mut child = Command::new(scheduler()?)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .creation_flags(0x08000000)
        .spawn()
        .map_err(|_| ErrorCode::Unavailable)?;
    let stdout = child.stdout.take().ok_or(ErrorCode::Unavailable)?;
    let reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        stdout.take(65537).read_to_end(&mut bytes).map(|_| bytes)
    });
    let deadline = Instant::now() + Duration::from_secs(15);
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status.success()),
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break Err(ErrorCode::TransportUncertain);
            }
        }
    };
    let bytes = reader
        .join()
        .map_err(|_| ErrorCode::Unavailable)?
        .map_err(|_| ErrorCode::Unavailable)?;
    if bytes.len() > 65536 {
        return Err(ErrorCode::Capacity);
    }
    Ok((status?, bytes))
}
fn exported(root: &Path) -> Result<Option<Vec<u8>>, ErrorCode> {
    let (ok, bytes) = run(&["/Query", "/TN", &task_name(root)?, "/XML"])?;
    Ok(if ok { Some(bytes) } else { None })
}
fn managed(root: &Path) -> Result<Record, ErrorCode> {
    let saved: Record = serde_json::from_slice(&storage::read_private(&record(root), 128 * 1024)?)
        .map_err(|_| ErrorCode::Conflict)?;
    if !saved.executable.is_absolute()
        || exported(root)?.as_deref() != Some(saved.exported_xml.as_slice())
    {
        return Err(ErrorCode::Conflict);
    }
    Ok(saved)
}
pub fn install(root: &Path, executable: &Path) -> Result<PathBuf, ErrorCode> {
    if record(root).symlink_metadata().is_ok() || exported(root)?.is_some() {
        return Err(ErrorCode::Conflict);
    }
    let sid =
        magicvault_primitives::windows::current_sid_string().map_err(|_| ErrorCode::Unavailable)?;
    let command = literal_path(executable)?;
    let args = format!("--root {} serve", argument(root)?);
    let task = format!("<?xml version=\"1.0\" encoding=\"UTF-16\"?><Task version=\"1.2\" xmlns=\"http://schemas.microsoft.com/windows/2004/02/mit/task\"><Triggers><LogonTrigger><Enabled>true</Enabled><UserId>{}</UserId></LogonTrigger></Triggers><Principals><Principal id=\"User\"><UserId>{}</UserId><LogonType>InteractiveToken</LogonType><RunLevel>LeastPrivilege</RunLevel></Principal></Principals><Settings><MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy><DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries><StopIfGoingOnBatteries>false</StopIfGoingOnBatteries><ExecutionTimeLimit>PT0S</ExecutionTimeLimit></Settings><Actions Context=\"User\"><Exec><Command>{}</Command><Arguments>{}</Arguments></Exec></Actions></Task>", xml(&sid), xml(&sid), xml(command), xml(&args));
    let definition = root.join(format!("service-install-{}.xml", uuid::Uuid::new_v4()));
    let mut file = private_fs::create_file(&definition, 0o600).map_err(|_| ErrorCode::Conflict)?;
    let bytes: Vec<_> = std::iter::once(0xfeffu16)
        .chain(task.encode_utf16())
        .flat_map(u16::to_le_bytes)
        .collect();
    file.write_all(&bytes)
        .and_then(|_| file.sync_all())
        .map_err(|_| ErrorCode::PersistenceUncertain)?;
    drop(file);
    // No /F: a pre-existing task must never be overwritten.
    let created = run(&[
        "/Create",
        "/TN",
        &task_name(root)?,
        "/XML",
        definition.to_str().ok_or(ErrorCode::InvalidRequest)?,
    ]);
    let _ = std::fs::remove_file(&definition);
    if !created?.0 {
        return Err(ErrorCode::Conflict);
    }
    let saved = Record {
        executable: executable.to_owned(),
        exported_xml: exported(root)?.ok_or(ErrorCode::PersistenceUncertain)?,
    };
    let mut file =
        private_fs::create_file(&record(root), 0o600).map_err(|_| ErrorCode::Conflict)?;
    file.write_all(&serde_json::to_vec(&saved).map_err(|_| ErrorCode::Unavailable)?)
        .and_then(|_| file.sync_all())
        .map_err(|_| ErrorCode::PersistenceUncertain)?;
    Ok(record(root))
}
pub fn verify_executable(root: &Path, executable: &Path) -> Result<Option<PathBuf>, ErrorCode> {
    if !record(root)
        .try_exists()
        .map_err(|_| ErrorCode::Unavailable)?
    {
        return if exported(root)?.is_none() {
            Ok(None)
        } else {
            Err(ErrorCode::Conflict)
        };
    }
    if managed(root)?.executable != executable {
        return Err(ErrorCode::Conflict);
    }
    Ok(Some(record(root)))
}
pub async fn start(root: &Path) -> Result<(), ErrorCode> {
    let root = root.to_owned();
    tokio::task::spawn_blocking(move || {
        managed(&root)?;
        if run(&["/Run", "/TN", &task_name(&root)?])?.0 {
            Ok(())
        } else {
            Err(ErrorCode::Unavailable)
        }
    })
    .await
    .map_err(|_| ErrorCode::Unavailable)?
}
pub async fn loaded(root: &Path) -> Result<bool, ErrorCode> {
    let owned = root.to_owned();
    tokio::task::spawn_blocking(move || managed(&owned))
        .await
        .map_err(|_| ErrorCode::Unavailable)??;
    Ok(Client::unpaired(root.to_owned())?.status().await.is_ok())
}
fn event_name(root: &Path) -> Result<Vec<u16>, ErrorCode> {
    let name = format!(
        "Local\\MagicVault-Stop-{}-{}",
        storage::inspect_instance(root)?.id,
        magicvault_primitives::windows::current_sid_string().map_err(|_| ErrorCode::Unavailable)?
    );
    wide(std::ffi::OsStr::new(&name)).map_err(|_| ErrorCode::InvalidRequest)
}
pub async fn stop(root: &Path) -> Result<(), ErrorCode> {
    let owned = root.to_owned();
    tokio::task::spawn_blocking(move || managed(&owned))
        .await
        .map_err(|_| ErrorCode::Unavailable)??;
    let handle = unsafe { OpenEventW(EVENT_MODIFY_STATE, 0, event_name(root)?.as_ptr()) };
    if handle.is_null() {
        return Err(ErrorCode::Unavailable);
    }
    let handle = unsafe { OwnedHandle::from_raw_handle(handle) };
    if unsafe { SetEvent(handle.as_raw_handle()) } == 0 {
        return Err(ErrorCode::Unavailable);
    }
    // The caller waits for the writer lease before replacing an installation.
    Ok(())
}
fn stop_event(root: &Path) -> Result<OwnedHandle, ErrorCode> {
    let security = PrivateSecurity::new().map_err(|_| ErrorCode::Unavailable)?;
    let handle = unsafe { CreateEventW(&security.attributes(), 1, 0, event_name(root)?.as_ptr()) };
    if handle.is_null() {
        return Err(ErrorCode::Unavailable);
    }
    let existing = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;
    let handle = unsafe { OwnedHandle::from_raw_handle(handle) };
    if existing {
        return Err(ErrorCode::Conflict);
    }
    Ok(handle)
}
pub async fn wait_for_stop(root: &Path, cancel: CancellationToken) -> Result<(), ErrorCode> {
    let handle = stop_event(root)?;
    loop {
        match unsafe { WaitForSingleObject(handle.as_raw_handle(), 0) } {
            WAIT_OBJECT_0 => return Ok(()),
            WAIT_TIMEOUT => {}
            _ => return Err(ErrorCode::Unavailable),
        }
        tokio::select! { _ = cancel.cancelled() => return Ok(()), _ = tokio::time::sleep(Duration::from_millis(100)) => {} }
    }
}
pub fn remove(root: &Path) -> Result<(), ErrorCode> {
    managed(root)?;
    if !run(&["/Delete", "/TN", &task_name(root)?, "/F"])?.0 {
        return Err(ErrorCode::Unavailable);
    }
    std::fs::remove_file(record(root)).map_err(|_| ErrorCode::PersistenceUncertain)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn task_paths_are_literal_quoted_and_xml_escaped() {
        assert_eq!(
            argument(Path::new(r"C:\Vault & Tools\")).unwrap(),
            "\"C:\\Vault & Tools\\\\\""
        );
        assert_eq!(
            xml(&argument(Path::new(r"C:\Vault & Tools")).unwrap()),
            "&quot;C:\\Vault &amp; Tools&quot;"
        );
        for path in [
            "relative",
            "C:\\%USERNAME%\\vault",
            "C:\\vault\nnext",
            "C:\\vault\"next",
        ] {
            assert!(argument(Path::new(path)).is_err());
        }
    }
}
