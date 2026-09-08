//! Opt-in, private stack evidence for a slow fixture-owned native host. Never
//! enabled in the normal timing lane; sampling can perturb the measurement.
use std::{fs, os::unix::fs::PermissionsExt, path::Path, process::Stdio, time::Duration};
use tokio_util::sync::CancellationToken;

pub(super) struct Observation {
    stop: CancellationToken,
    worker: Option<tokio::task::JoinHandle<()>>,
}

impl Observation {
    pub(super) fn start(marker: &Path) -> Option<Self> {
        let parent = std::env::var_os("MAGICVAULT_TEST_STARTUP_DIAGNOSTICS")?;
        let parent = Path::new(&parent);
        assert!(parent.is_absolute());
        let metadata = fs::symlink_metadata(parent).unwrap();
        assert!(metadata.is_dir() && metadata.permissions().mode() & 0o077 == 0);
        let directory = tempfile::Builder::new()
            .prefix("native-startup-")
            .permissions(fs::Permissions::from_mode(0o700))
            .tempdir_in(parent)
            .unwrap();
        let expected =
            fs::canonicalize(std::env::var_os("MAGICVAULT_TEST_NATIVE_HOST").unwrap()).unwrap();
        let marker = marker.to_owned();
        let previous = fs::metadata(&marker).map(|m| m.len()).unwrap_or(0);
        let stop = CancellationToken::new();
        let cancelled = stop.clone();
        let worker = tokio::spawn(async move {
            tokio::select! {
                _ = cancelled.cancelled() => return,
                _ = tokio::time::sleep(Duration::from_secs(2)) => {},
            }
            // A new wrapper launch is required. The previous profile's recorded
            // PID is not a target, even if the next browser is slow to launch.
            while fs::metadata(&marker).map(|m| m.len()).unwrap_or(0) <= previous {
                tokio::select! {
                    _ = cancelled.cancelled() => return,
                    _ = tokio::time::sleep(Duration::from_millis(30)) => {},
                }
            }
            let Some(pid) = fs::read_to_string(marker.with_extension("pid"))
                .ok()
                .and_then(|value| value.parse::<u32>().ok())
                .filter(|pid| *pid > 1)
            else {
                return;
            };
            // Resolve the process image, not its arguments or environment.
            // Refuse a stale/reused PID unless it is this exact synthetic host.
            let mut inspect = tokio::process::Command::new("/bin/ps");
            inspect
                .args(["-p", &pid.to_string(), "-o", "comm="])
                .stdin(Stdio::null())
                .stderr(Stdio::null())
                .kill_on_drop(true);
            let Ok(Ok(output)) =
                tokio::time::timeout(Duration::from_secs(2), inspect.output()).await
            else {
                return;
            };
            let Ok(image) = std::str::from_utf8(&output.stdout) else {
                return;
            };
            if !output.status.success()
                || fs::canonicalize(image.trim()).ok().as_ref() != Some(&expected)
            {
                return;
            }
            // Keep evidence private even if sampling or the acceptance case
            // fails. Never send its raw stack/path metadata to public CI logs.
            let directory = directory.keep();
            let mut sample = tokio::process::Command::new("/usr/bin/sample");
            sample
                .arg(pid.to_string())
                .args(["1", "10", "-file"])
                .arg(directory.join("host.sample.txt"))
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .kill_on_drop(true);
            let captured = matches!(tokio::time::timeout(Duration::from_secs(5), sample.status()).await, Ok(Ok(status)) if status.success());
            eprintln!("private fixture-host startup sample captured: {captured}");
        });
        Some(Self {
            stop,
            worker: Some(worker),
        })
    }

    pub(super) async fn finish(mut self) {
        self.stop.cancel();
        // If sampling already began, let its bounded private capture finish.
        // Retain the handle in self while awaiting so cancellation of finish()
        // still reaches Drop's abort and the sample child's kill-on-drop guard.
        self.worker.as_mut().unwrap().await.unwrap();
        self.worker.take();
    }
}

impl Drop for Observation {
    fn drop(&mut self) {
        self.stop.cancel();
        if let Some(worker) = &self.worker {
            worker.abort();
        }
    }
}
