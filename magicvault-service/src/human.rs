//! Daemon-owned native consent. Callers provide metadata, never a decision.
use async_trait::async_trait;
use magicvault_protocol::{ErrorCode, CONSENT_TTL_SECS};
use tokio_util::sync::CancellationToken;
use zeroize::Zeroizing;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UseDecision {
    Deny,
    AllowOnce,
    AlwaysAllow,
}

#[async_trait]
pub trait HumanInteraction: Send + Sync {
    async fn confirm(&self, message: &str, cancel: CancellationToken) -> Result<bool, ErrorCode>;
    /// Trusted human seam only. Existing embedders remain one-use by default.
    async fn confirm_use(
        &self,
        message: &str,
        cancel: CancellationToken,
    ) -> Result<UseDecision, ErrorCode> {
        self.confirm(message, cancel).await.map(|allow| {
            if allow {
                UseDecision::AllowOnce
            } else {
                UseDecision::Deny
            }
        })
    }
    async fn secret(
        &self,
        message: &str,
        cancel: CancellationToken,
    ) -> Result<Zeroizing<String>, ErrorCode>;
    /// One-time collection is an explicit opt-in for trusted custom hosts.
    /// Never silently reuse an enrollment UI that promises to save the input.
    async fn secret_once(
        &self,
        _message: &str,
        _cancel: CancellationToken,
    ) -> Result<Zeroizing<String>, ErrorCode> {
        Err(ErrorCode::Unavailable)
    }
    /// One operation only; cannot create or reuse an Always allow grant.
    async fn confirm_once(
        &self,
        message: &str,
        cancel: CancellationToken,
    ) -> Result<bool, ErrorCode> {
        self.confirm(message, cancel).await
    }
}

pub struct NativeHuman;

// Summary and exact details share one bound; the renderer never truncates them.
pub(crate) const MAX_PROMPT_BYTES: usize = magicvault_prompt::MAX_MESSAGE_BYTES;
use magicvault_prompt::{Kind, Prompt, Reply};

async fn dialog(message: &str, kind: Kind, cancel: CancellationToken) -> Result<Reply, ErrorCode> {
    let executable = std::env::current_exe().map_err(|_| ErrorCode::Unavailable)?;
    // Resolve only a sibling shipped with this daemon. Never search PATH or
    // accept an agent/environment-selected input provider.
    let helper = executable
        .parent()
        .ok_or(ErrorCode::Unavailable)?
        .join(format!("magicvault-prompt{}", std::env::consts::EXE_SUFFIX));
    dialog_at(&helper, message, kind, cancel).await
}

async fn dialog_at(
    helper: &std::path::Path,
    message: &str,
    kind: Kind,
    cancel: CancellationToken,
) -> Result<Reply, ErrorCode> {
    use std::{process::Stdio, time::Duration};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    if message.len() > MAX_PROMPT_BYTES {
        return Err(ErrorCode::Capacity);
    }
    if cancel.is_cancelled() {
        return Err(ErrorCode::Cancelled);
    }
    let prompt = Prompt::new(kind, message.to_owned()).map_err(|_| ErrorCode::InvalidRequest)?;
    let mut request = Vec::new();
    prompt
        .write(&mut request)
        .map_err(|_| ErrorCode::InvalidRequest)?;
    let metadata = std::fs::symlink_metadata(helper).map_err(|_| ErrorCode::Unavailable)?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(ErrorCode::Unavailable);
    }
    let mut command = tokio::process::Command::new(helper);
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    #[cfg(windows)]
    command.creation_flags(0x08000000); // CREATE_NO_WINDOW; the helper creates its own GUI.
    let mut child = command.spawn().map_err(|_| ErrorCode::Unavailable)?;
    let work = async {
        let mut stdin = child.stdin.take().ok_or(ErrorCode::Unavailable)?;
        let stdout = child.stdout.take().ok_or(ErrorCode::Unavailable)?;
        stdin
            .write_all(&request)
            .await
            .map_err(|_| ErrorCode::Unavailable)?;
        stdin.flush().await.map_err(|_| ErrorCode::Unavailable)?;
        let mut bytes = Zeroizing::new(Vec::new());
        stdout
            .take((magicvault_prompt::MAX_SECRET_BYTES + 2) as u64)
            .read_to_end(&mut bytes)
            .await
            .map_err(|_| ErrorCode::Unavailable)?;
        if bytes.len() > magicvault_prompt::MAX_SECRET_BYTES + 1 {
            return Err(ErrorCode::Capacity);
        }
        let status = child.wait().await.map_err(|_| ErrorCode::Unavailable)?;
        // Keep stdin open until exit; dropping it signals cancellation to the UI.
        drop(stdin);
        if !status.success() {
            return Err(ErrorCode::Unavailable);
        }
        Reply::decode(kind, &bytes).map_err(|_| ErrorCode::InvalidRequest)
    };
    let result = tokio::select! {
        _ = cancel.cancelled() => Err(ErrorCode::Cancelled),
        result = tokio::time::timeout(Duration::from_secs(CONSENT_TTL_SECS), work) => {
            result.unwrap_or(Err(ErrorCode::Expired))
        },
    };
    if result.is_err() {
        let _ = child.kill().await;
        let _ = child.wait().await;
    }
    // Cancellation wins even if a late button click races child completion.
    if cancel.is_cancelled() {
        return Err(ErrorCode::Cancelled);
    }
    result
}

#[async_trait]
impl HumanInteraction for NativeHuman {
    async fn confirm(&self, message: &str, cancel: CancellationToken) -> Result<bool, ErrorCode> {
        Ok(matches!(
            dialog(message, Kind::Confirm, cancel).await?,
            Reply::Allow
        ))
    }
    async fn confirm_use(
        &self,
        message: &str,
        cancel: CancellationToken,
    ) -> Result<UseDecision, ErrorCode> {
        match dialog(message, Kind::Use, cancel).await? {
            Reply::Allow => Ok(UseDecision::AllowOnce),
            Reply::Always => Ok(UseDecision::AlwaysAllow),
            _ => Ok(UseDecision::Deny),
        }
    }
    async fn secret(
        &self,
        message: &str,
        cancel: CancellationToken,
    ) -> Result<Zeroizing<String>, ErrorCode> {
        match dialog(message, Kind::Secret, cancel).await? {
            Reply::Secret(value) => Ok(value),
            _ => Err(ErrorCode::Denied),
        }
    }
    async fn secret_once(
        &self,
        message: &str,
        cancel: CancellationToken,
    ) -> Result<Zeroizing<String>, ErrorCode> {
        match dialog(message, Kind::SecretOnce, cancel).await? {
            Reply::Secret(value) => Ok(value),
            _ => Err(ErrorCode::Denied),
        }
    }
    async fn confirm_once(
        &self,
        message: &str,
        cancel: CancellationToken,
    ) -> Result<bool, ErrorCode> {
        Ok(matches!(
            dialog(message, Kind::ConfirmOnce, cancel).await?,
            Reply::Allow
        ))
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::{os::unix::fs::PermissionsExt, path::PathBuf, time::Duration};

    fn helper(directory: &tempfile::TempDir, body: &str) -> PathBuf {
        let path = directory.path().join("prompt-fixture");
        std::fs::write(
            &path,
            format!("#!/bin/sh\ndd bs=1 count=4 of=/dev/null 2>/dev/null\n{body}\n"),
        )
        .unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
        path
    }

    #[tokio::test]
    async fn helper_answers_are_typed_and_unsuccessful_processes_cannot_approve() {
        let directory = tempfile::tempdir().unwrap();
        let path = helper(&directory, "printf '\\002'");
        assert!(matches!(
            dialog_at(&path, "synthetic", Kind::Use, CancellationToken::new()).await,
            Ok(Reply::Always)
        ));
        assert!(matches!(
            dialog_at(
                &path,
                "synthetic",
                Kind::ConfirmOnce,
                CancellationToken::new()
            )
            .await,
            Err(ErrorCode::InvalidRequest)
        ));
        helper(&directory, "printf '\\001'; exit 1");
        assert!(matches!(
            dialog_at(&path, "synthetic", Kind::Confirm, CancellationToken::new()).await,
            Err(ErrorCode::Unavailable)
        ));
        helper(&directory, "printf '\\003synthetic value'");
        match dialog_at(
            &path,
            "synthetic",
            Kind::SecretOnce,
            CancellationToken::new(),
        )
        .await
        .unwrap()
        {
            Reply::Secret(value) => assert_eq!(value.as_str(), "synthetic value"),
            _ => panic!("wrong reply kind"),
        }
    }

    #[tokio::test]
    async fn cancellation_reaps_the_helper_and_precancel_never_starts_it() {
        let directory = tempfile::tempdir().unwrap();
        let path = helper(&directory, "printf '%s' \"$$\" > \"$0.pid\"\nexec sleep 30");
        let marker = path.with_extension("pid");
        let cancelled = CancellationToken::new();
        cancelled.cancel();
        assert!(matches!(
            dialog_at(&path, "synthetic", Kind::Confirm, cancelled).await,
            Err(ErrorCode::Cancelled)
        ));
        assert!(!marker.exists());
        let cancel = CancellationToken::new();
        let worker_cancel = cancel.clone();
        let worker = tokio::spawn(async move {
            dialog_at(&path, "synthetic", Kind::Confirm, worker_cancel).await
        });
        let pid = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let Ok(pid) = std::fs::read_to_string(&marker) {
                    if let Ok(pid) = pid.parse::<i32>() {
                        break pid;
                    }
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        cancel.cancel();
        assert!(matches!(
            tokio::time::timeout(Duration::from_secs(5), worker)
                .await
                .unwrap()
                .unwrap(),
            Err(ErrorCode::Cancelled)
        ));
        assert_eq!(unsafe { libc::kill(pid, 0) }, -1);
        assert_eq!(
            std::io::Error::last_os_error().raw_os_error(),
            Some(libc::ESRCH)
        );
    }
}
