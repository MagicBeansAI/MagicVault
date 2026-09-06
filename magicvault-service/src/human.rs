//! Daemon-owned native consent. Callers provide metadata, never a decision.
use async_trait::async_trait;
use magicvault_protocol::{ErrorCode, CONSENT_TTL_SECS};
use tokio_util::sync::CancellationToken;
use zeroize::Zeroizing;

#[async_trait]
pub trait HumanInteraction: Send + Sync {
    async fn confirm(&self, message: &str, cancel: CancellationToken) -> Result<bool, ErrorCode>;
    async fn secret(&self, message: &str, cancel: CancellationToken) -> Result<Zeroizing<String>, ErrorCode>;
}

pub struct NativeHuman;

#[cfg(target_os = "macos")]
async fn dialog(message: &str, secret: bool, cancel: CancellationToken) -> Result<Zeroizing<String>, ErrorCode> {
    use std::{process::Stdio, time::Duration};
    use tokio::io::AsyncReadExt;
    // The script is fixed; untrusted metadata is argv, not interpolated code.
    // Only metadata enters argv. The hidden answer travels in a private pipe.
    const CONFIRM: &str = "on run argv\nset r to display dialog (item 1 of argv) with title \"MagicVault — human decision\" buttons {\"Deny\", \"Allow\"} default button \"Deny\" cancel button \"Deny\" giving up after 120\nif gave up of r then error number -128\nreturn button returned of r\nend run";
    const SECRET: &str = "on run argv\nset r to display dialog (item 1 of argv) with title \"MagicVault — enroll credential\" default answer \"\" with hidden answer buttons {\"Cancel\", \"Save\"} default button \"Cancel\" cancel button \"Cancel\" giving up after 120\nif gave up of r then error number -128\nreturn text returned of r\nend run";
    let mut child = tokio::process::Command::new("/usr/bin/osascript")
        .arg("-e").arg(if secret { SECRET } else { CONFIRM }).arg("--").arg(message)
        .stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null())
        .kill_on_drop(true).spawn().map_err(|_| ErrorCode::Unavailable)?;
    let stdout = child.stdout.take().ok_or(ErrorCode::Unavailable)?;
    let work = async {
        let mut bytes = Zeroizing::new(Vec::new());
        stdout.take(4098).read_to_end(&mut bytes).await.map_err(|_| ErrorCode::Unavailable)?;
        if bytes.len() > 4097 { return Err(ErrorCode::Capacity); }
        let status = child.wait().await.map_err(|_| ErrorCode::Unavailable)?;
        if !status.success() { return Err(ErrorCode::Denied); }
        // osascript adds exactly one output newline. Preserve other whitespace.
        if bytes.last() == Some(&b'\n') { bytes.pop(); }
        if bytes.is_empty() || bytes.len() > 4096 { return Err(ErrorCode::InvalidRequest); }
        let text = std::str::from_utf8(&bytes).map_err(|_| ErrorCode::InvalidRequest)?;
        Ok(Zeroizing::new(text.to_owned()))
    };
    let result = tokio::select! {
        _ = cancel.cancelled() => Err(ErrorCode::Denied),
        result = tokio::time::timeout(Duration::from_secs(CONSENT_TTL_SECS), work) => {
            result.unwrap_or(Err(ErrorCode::Expired))
        },
    };
    if result.is_err() { let _ = child.kill().await; let _ = child.wait().await; }
    result
}

#[async_trait]
impl HumanInteraction for NativeHuman {
    async fn confirm(&self, message: &str, cancel: CancellationToken) -> Result<bool, ErrorCode> {
        #[cfg(target_os = "macos")]
        { return dialog(message, false, cancel).await.map(|v| &*v == "Allow"); }
        #[cfg(not(target_os = "macos"))]
        { let _ = (message, cancel); Err(ErrorCode::Unavailable) }
    }
    async fn secret(&self, message: &str, cancel: CancellationToken) -> Result<Zeroizing<String>, ErrorCode> {
        #[cfg(target_os = "macos")]
        { return dialog(message, true, cancel).await; }
        #[cfg(not(target_os = "macos"))]
        { let _ = (message, cancel); Err(ErrorCode::Unavailable) }
    }
}
