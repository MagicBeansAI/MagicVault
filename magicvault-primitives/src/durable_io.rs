//! Durable local-filesystem write primitives extracted from
//! `magician_v2::artifact_v2::io` — every filesystem-backed store in the
//! workspace shares these; the error type is deliberately `std::io::Error`
//! so no store takes a dependency on another module's error enum.

use std::{future::Future, path::Path, time::Duration};

use uuid::Uuid;
/// the helpers losing that on consolidation was a review finding.
pub fn warn_cleanup_failed(tmp_path: &Path, error: &std::io::Error) {
    // NotFound is the write failing before it created the temp — there is
    // nothing to clean and nothing to report.
    if error.kind() == std::io::ErrorKind::NotFound {
        return;
    }
    tracing::warn!(
        tmp_path = %tmp_path.display(),
        %error,
        "failed to remove staging temp after a failed durable write; \
         the file will not be reused or swept"
    );
}

const TRANSIENT_IO_RETRY_ATTEMPTS: usize = 8;
const TRANSIENT_IO_RETRY_BASE_DELAY_MS: u64 = 5;

pub fn is_transient_fd_exhaustion(error: &std::io::Error) -> bool {
    matches!(error.raw_os_error(), Some(23 | 24))
}

pub async fn retry_transient_io<T, F, Fut>(mut operation: F) -> Result<T, std::io::Error>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T, std::io::Error>>,
{
    for attempt in 0..TRANSIENT_IO_RETRY_ATTEMPTS {
        match operation().await {
            Ok(value) => return Ok(value),
            Err(error)
                if is_transient_fd_exhaustion(&error)
                    && attempt + 1 < TRANSIENT_IO_RETRY_ATTEMPTS =>
            {
                tokio::time::sleep(retry_delay(attempt)).await;
            },
            Err(error) => return Err(error),
        }
    }
    unreachable!("transient I/O retry loop always returns before exhausting attempts")
}

pub fn retry_transient_io_blocking<T, F>(mut operation: F) -> Result<T, std::io::Error>
where
    F: FnMut() -> Result<T, std::io::Error>,
{
    for attempt in 0..TRANSIENT_IO_RETRY_ATTEMPTS {
        match operation() {
            Ok(value) => return Ok(value),
            Err(error)
                if is_transient_fd_exhaustion(&error)
                    && attempt + 1 < TRANSIENT_IO_RETRY_ATTEMPTS =>
            {
                std::thread::sleep(retry_delay(attempt));
            },
            Err(error) => return Err(error),
        }
    }
    unreachable!("transient blocking I/O retry loop always returns before exhausting attempts")
}

fn retry_delay(attempt: usize) -> Duration {
    let multiplier = 1_u64 << attempt.min(6);
    Duration::from_millis(TRANSIENT_IO_RETRY_BASE_DELAY_MS.saturating_mul(multiplier))
}
/// Write `value` to `path` durably: unique temp, `sync_all`, rename, then a
/// parent-directory sync so the rename itself survives power loss.
///
/// **The error is `std::io::Error` on purpose.** This is the primitive every
/// filesystem-backed store in the crate needs, and the ones that do not already
/// speak `ArtifactV2Error` were writing their own copy rather than take a
/// dependency on another module's error enum — `secrets/store.rs` did exactly
/// that, and its copy is the reason this split exists. A primitive nobody can
/// call is a primitive everybody reimplements, and the reimplementations are
/// where the missing `sync_all` and the shared `.tmp` name come from.
///
/// The temp name carries a UUID rather than a fixed suffix. A fixed
/// `<file>.tmp` is shared by every concurrent writer of that path, so two of
/// them interleaving rename a half-written file over the store — twice-shipped,
/// once in the VibeDev project store and once in the secret vault.
///
/// The temp is removed on every failure path; with a unique name, skipping that
/// would leak a file per failure rather than reusing one.
pub fn write_bytes_durably_sync(path: &Path, value: &[u8]) -> std::io::Result<()> {
    write_bytes_durably_with_mode_sync(path, value, None)
}

/// Synchronous [`write_bytes_durably_with_mode`]. The staging file receives
/// `mode` before publication, then the destination directory is synced after
/// the atomic rename.
pub fn write_bytes_durably_with_mode_sync(
    path: &Path,
    value: &[u8],
    mode: Option<u32>,
) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        retry_transient_io_blocking(|| std::fs::create_dir_all(parent))?;
    }
    let tmp_path = path.with_file_name(format!(".artifact-write-{}.tmp", Uuid::new_v4().simple()));
    let written = (|| {
        use std::io::Write;
        let mut file = retry_transient_io_blocking(|| std::fs::File::create(&tmp_path))?;
        file.write_all(value)?;
        file.flush()?;
        file.sync_all()?;
        drop(file);
        #[cfg(unix)]
        if let Some(mode) = mode {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&tmp_path, std::fs::Permissions::from_mode(mode))?;
        }
        #[cfg(not(unix))]
        let _ = mode;
        retry_transient_io_blocking(|| std::fs::rename(&tmp_path, path))?;
        sync_parent_dir_blocking(path)
    })();
    if written.is_err() {
        if let Err(cleanup) = std::fs::remove_file(&tmp_path) {
            warn_cleanup_failed(&tmp_path, &cleanup);
        }
    }
    written
}

/// Publish a caller-produced staging file through the same durable rename
/// boundary as the byte writers.
///
/// Use this when an external storage engine must create the staged bytes (for
/// example SQLCipher export) and therefore a byte-oriented helper cannot
/// produce them. The staging file is synced again immediately before rename,
/// and the destination parent is synced after publication.
pub fn publish_staged_file_durably_sync(
    staged_path: &Path,
    destination_path: &Path,
) -> std::io::Result<()> {
    retry_transient_io_blocking(|| std::fs::File::open(staged_path)?.sync_all())?;
    retry_transient_io_blocking(|| std::fs::rename(staged_path, destination_path))?;
    sync_parent_dir_blocking(destination_path)
}
/// `fsync` the directory holding `path`, so a rename into it is durable.
///
/// A rename is atomic with respect to *ordering*, not durability: without this
/// the rename can survive a power cut while the contents do not. Exposed with
/// an `io::Error` for the same reason as [`write_bytes_durably_sync`] — the
/// append-only writers need it on its own, without a temp-and-rename.
pub fn sync_parent_dir_blocking(path: &Path) -> std::io::Result<()> {
    let Some(parent) = path.parent() else {
        return Ok(());
    };
    retry_transient_io_blocking(|| {
        let dir = std::fs::File::open(parent)?;
        dir.sync_all()
    })
}
