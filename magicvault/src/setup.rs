//! Human-invoked onboarding only. npm and MCP never invoke these operations.
use magicvault_service::{
    client::Client,
    installation::{self, Installation},
    launch_agent, native,
    protocol::{valid_name, ErrorCode},
    storage,
};
use serde_json::{json, Value};
use std::{
    future::Future,
    path::{Path, PathBuf},
    time::Duration,
};

const VERSION: &str = env!("CARGO_PKG_VERSION");

async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> Result<T, ErrorCode> + Send + 'static,
) -> Result<T, ErrorCode> {
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|_| ErrorCode::Unavailable)?
}
fn paths(root: PathBuf, app: Option<PathBuf>) -> Result<(PathBuf, PathBuf), ErrorCode> {
    Ok((
        installation::canonical_leaf(&root)?,
        match app {
            Some(p) => installation::canonical_leaf(&p)?,
            None => installation::default_app_dir()?,
        },
    ))
}
fn supported() -> Result<(), ErrorCode> {
    if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        Ok(())
    } else {
        Err(ErrorCode::Unavailable)
    }
}
fn exists(path: &Path) -> Result<bool, ErrorCode> {
    match path.symlink_metadata() {
        Ok(_) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(_) => Err(ErrorCode::Unavailable),
    }
}
fn configuration(app: &Installation, root: &Path, profile: &str) -> Result<Value, ErrorCode> {
    Ok(json!({
        "version": VERSION, "app_directory": app.directory(), "vault_root": root,
        "extension_directory": app.extension(),
        "mcpServers": {"magicvault": {"command": app.executable("magicvault-mcp")?, "args": ["--root", root, "--profile", profile]}},
        "next_steps": ["Enroll credentials with magicvault enroll; values belong only in native prompts.", "For CDP, register a local debugging endpoint. For the extension, load extension_directory in Chrome and run magicvault extension install --extension-id ID.", "Register reference-only destination profiles before new process or HTTP delivery."]
    }))
}

pub async fn setup(
    root: PathBuf,
    profile: String,
    app_dir: Option<PathBuf>,
    bundle: Option<PathBuf>,
    install_only: bool,
    label: String,
) -> Result<Value, ErrorCode> {
    supported()?;
    if !valid_name(&profile)
        || label.is_empty()
        || label.len() > 120
        || label.chars().any(char::is_control)
    {
        return Err(ErrorCode::InvalidRequest);
    }
    let (root, app_path) = paths(root, app_dir)?;
    let vault = root.clone();
    let app = blocking(move || {
        let app = Installation::open(&app_path, &vault, true)?;
        match app.current()? {
            Some(current) if current.version == VERSION => {}
            Some(_) => return Err(ErrorCode::Conflict), // Explicit upgrade, never hidden replacement.
            None => app.activate(
                app.stage(
                    &bundle
                        .map(Ok)
                        .unwrap_or_else(installation::bundled_source)?,
                    VERSION,
                )?,
            )?,
        }
        Ok(app)
    })
    .await?;
    let mut output = configuration(&app, &root, &profile)?;
    if install_only {
        output["installed"] = json!(true);
        output["service_started"] = json!(false);
        output["vault_unchanged"] = json!(true);
        return Ok(output);
    }
    let vault = root.clone();
    blocking(move || {
        if exists(&vault.join("instance.json"))? {
            storage::inspect_instance(&vault)?;
        } else {
            storage::initialize(&vault)?;
        }
        Ok(())
    })
    .await?;
    let vault = root.clone();
    let executable = app.executable("magicvault")?;
    blocking(move || {
        if launch_agent::verify_executable(&vault, &executable)?.is_none() {
            launch_agent::install(&vault, &executable)?;
        }
        Ok(())
    })
    .await?;
    if !launch_agent::loaded(&root).await? {
        // Do not mistake an unrelated foreground daemon's readiness for the
        // newly installed service. Its writer lease must be free before launch.
        drop(drained(&root).await?);
        launch_agent::start(&root).await?;
    }
    ready(&root).await?;
    let client = Client::load(root.clone(), &profile)?;
    if !exists(&root.join(format!("client-{profile}.json")))? {
        Client::pair(root.clone(), &profile, label).await?;
    } else if client.status().await?.client_id.is_none() {
        // A revoked capability is not silently overwritten or re-paired.
        return Err(ErrorCode::Unauthorized);
    }
    output["installed"] = json!(true);
    output["service_started"] = json!(true);
    output["paired"] = json!(true);
    Ok(output)
}

async fn ready(root: &Path) -> Result<(), ErrorCode> {
    let client = Client::unpaired(root.to_owned())?;
    bounded_poll(|| async { Ok(client.status().await.is_ok_and(|s| s.ready)) }).await
}

async fn bounded_poll<F, Fut>(mut check: F) -> Result<(), ErrorCode>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<bool, ErrorCode>>,
{
    tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            if check().await? {
                return Ok(());
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .map_err(|_| ErrorCode::TransportUncertain)?
}

/// Return the daemon's single-writer lease, held through activation/removal.
/// An unregistered foreground daemon also prevents replacement.
async fn drained(root: &Path) -> Result<std::sync::Arc<storage::InstanceLock>, ErrorCode> {
    drained_with_timeout(root, Duration::from_secs(15)).await
}

async fn drained_with_timeout(
    root: &Path,
    timeout: Duration,
) -> Result<std::sync::Arc<storage::InstanceLock>, ErrorCode> {
    tokio::time::timeout(timeout, async {
        loop {
            let vault = root.to_owned();
            match blocking(move || storage::open(&vault)).await {
                Ok(lock) => return Ok(lock),
                Err(ErrorCode::Busy) => tokio::time::sleep(Duration::from_millis(100)).await,
                Err(e) => return Err(e),
            }
        }
    })
    .await
    .map_err(|_| ErrorCode::TransportUncertain)?
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, os::unix::fs::PermissionsExt};

    fn root() -> tempfile::TempDir {
        let root = tempfile::tempdir().unwrap();
        fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let file = root.path().join("instance.json");
        fs::write(
            &file,
            serde_json::to_vec(&storage::Instance {
                format_version: 1,
                id: uuid::Uuid::new_v4(),
            })
            .unwrap(),
        )
        .unwrap();
        fs::set_permissions(file, fs::Permissions::from_mode(0o600)).unwrap();
        root
    }

    #[tokio::test]
    async fn live_writer_blocks_lifecycle_until_lease_release() {
        let root = root();
        let held = storage::open(root.path()).unwrap();
        let path = root.path().to_owned();
        let waiter =
            tokio::spawn(async move { drained_with_timeout(&path, Duration::from_secs(3)).await });
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert!(!waiter.is_finished());
        drop(held);
        let acquired = waiter.await.unwrap().unwrap();
        assert!(matches!(storage::open(root.path()), Err(ErrorCode::Busy)));
        drop(acquired);
        assert!(storage::open(root.path()).is_ok());
    }

    #[tokio::test]
    async fn drain_timeout_and_invalid_instance_fail_closed_without_initialization() {
        let root = root();
        let _held = storage::open(root.path()).unwrap();
        assert!(matches!(
            drained_with_timeout(root.path(), Duration::from_millis(30)).await,
            Err(ErrorCode::TransportUncertain)
        ));
        let missing = root.path().join("missing");
        assert!(drained_with_timeout(&missing, Duration::from_secs(1))
            .await
            .is_err());
        assert!(!missing.exists());
    }

    #[tokio::test]
    async fn doctor_reports_missing_and_unknown_apps_without_creating_files() {
        let root = tempfile::tempdir().unwrap();
        let vault = root.path().join("vault");
        let app = root.path().join("app");
        let report = doctor(vault.clone(), "agent".into(), Some(app.clone()))
            .await
            .unwrap();
        assert_eq!(report["installation"]["installed"], false);
        assert_eq!(report["read_only"], true);
        assert!(!app.exists());
        assert!(!vault.exists());
        fs::create_dir(&app).unwrap();
        fs::set_permissions(&app, fs::Permissions::from_mode(0o700)).unwrap();
        fs::write(app.join("unknown"), "preserve").unwrap();
        let report = doctor(vault, "agent".into(), Some(app.clone()))
            .await
            .unwrap();
        assert!(report["installation"]["error"].is_string());
        assert_eq!(fs::read_dir(app).unwrap().count(), 1);
    }
}

pub async fn upgrade(
    root: PathBuf,
    app_dir: Option<PathBuf>,
    bundle: Option<PathBuf>,
) -> Result<Value, ErrorCode> {
    supported()?;
    let (root, app_path) = paths(root, app_dir)?;
    let vault = root.clone();
    let (app, staged) = blocking(move || {
        let app = Installation::open(&app_path, &vault, false)?;
        let current = app.current()?.ok_or(ErrorCode::Conflict)?;
        let parse = |v: &str| {
            v.split('.')
                .map(|s| s.parse::<u32>().map_err(|_| ErrorCode::InvalidRequest))
                .collect::<Result<Vec<_>, _>>()
        };
        if parse(&current.version)? > parse(VERSION)? {
            return Err(ErrorCode::Conflict);
        }
        let staged = app.stage(
            &bundle
                .map(Ok)
                .unwrap_or_else(installation::bundled_source)?,
            VERSION,
        )?;
        Ok((app, staged))
    })
    .await?;
    let initialized = exists(&root.join("instance.json"))?;
    let mut restart = false;
    let lease = if initialized {
        let vault = root.clone();
        let executable = app.executable("magicvault")?;
        let definition =
            blocking(move || launch_agent::verify_executable(&vault, &executable)).await?;
        if definition.is_some() && launch_agent::loaded(&root).await? {
            launch_agent::stop(&root).await?;
            restart = true;
        }
        Some(drained(&root).await?)
    } else {
        None
    };
    let app = blocking(move || {
        app.activate(staged)?;
        Ok(app)
    })
    .await?;
    drop(lease);
    if restart {
        launch_agent::start(&root).await?;
        ready(&root).await?;
    }
    Ok(
        json!({"upgraded": true, "version": VERSION, "service_restarted": restart, "app_directory": app.directory(), "vault_unchanged": true, "reconnect_mcp_and_reload_extension": true, "previous_bundles_retained": true}),
    )
}

pub fn install_extension(
    root: &Path,
    profile: &str,
    extension_id: &str,
    app_dir: Option<PathBuf>,
) -> Result<(), ErrorCode> {
    let (root, path) = paths(root.to_owned(), app_dir)?;
    let app = Installation::open(&path, &root, false)?;
    app.current()?.ok_or(ErrorCode::Unavailable)?;
    // Keep the installer lock until publication completes, so a simultaneous
    // uninstall cannot leave freshly installed definitions pointing at an archive.
    native::install(
        &root,
        profile,
        extension_id,
        &app.executable("magicvault-native-host")?,
    )
}

pub async fn uninstall(root: PathBuf, app_dir: Option<PathBuf>) -> Result<Value, ErrorCode> {
    supported()?;
    let (root, app_path) = paths(root, app_dir)?;
    let vault = root.clone();
    let app = blocking(move || {
        let app = Installation::open(&app_path, &vault, false)?;
        app.current()?.ok_or(ErrorCode::Conflict)?;
        Ok(app)
    })
    .await?;
    let initialized = exists(&root.join("instance.json"))?;
    let lease = if initialized {
        let vault = root.clone();
        let executable = app.executable("magicvault")?;
        let definition =
            blocking(move || launch_agent::verify_executable(&vault, &executable)).await?;
        if definition.is_some() && launch_agent::loaded(&root).await? {
            launch_agent::stop(&root).await?;
        }
        let lease = drained(&root).await?;
        let vault = root.clone();
        let host = app.executable("magicvault-native-host")?;
        blocking(move || {
            let config_path = native::config_path(&vault);
            if exists(&config_path)? {
                let bytes = storage::read_private(&config_path, 8192)?;
                let config: native::HostConfig =
                    serde_json::from_slice(&bytes).map_err(|_| ErrorCode::Conflict)?;
                if config.executable != host {
                    return Err(ErrorCode::Conflict);
                }
                native::remove(&vault)?;
            }
            if definition.is_some() {
                launch_agent::remove(&vault)?;
            }
            Ok(())
        })
        .await?;
        Some(lease)
    } else {
        None
    };
    let archive = blocking(move || app.retire()).await?;
    drop(lease);
    Ok(
        json!({"uninstalled": true, "application_archive": archive, "vault_preserved": true, "keychain_preserved": true, "pairings_preserved": true, "next_steps": ["Remove the unpacked extension in the browser and the MagicVault entry in your MCP client.", "Uninstall the npm launcher separately. The application archive is recoverable; vault data was not deleted."]}),
    )
}

pub async fn doctor(
    root: PathBuf,
    profile: String,
    app_dir: Option<PathBuf>,
) -> Result<Value, ErrorCode> {
    if !valid_name(&profile) {
        return Err(ErrorCode::InvalidRequest);
    }
    let (root, path) = paths(root, app_dir)?;
    let vault = root.clone();
    let app_path = path.clone();
    let installation = blocking(move || {
        if !exists(&app_path)? { return Ok(json!({"installed": false})); }
        let app = Installation::open(&app_path, &vault, false)?;
        Ok(match app.current()? {
            Some(bundle) => json!({"installed": true, "version": bundle.version, "integrity_verified": true, "publisher_signature_verified": false, "extension_directory": app.extension()}),
            None => json!({"installed": false, "incomplete": true}),
        })
    }).await;
    let installation = match installation {
        Ok(v) => v,
        Err(e) => json!({"installed": null, "error": e}),
    };
    let daemon = match Client::load(root.clone(), &profile) {
        Ok(client) => match client.status().await {
            Ok(s) => serde_json::to_value(s).map_err(|_| ErrorCode::Unavailable)?,
            Err(e) => json!({"ready": false, "error": e}),
        },
        Err(e) => json!({"ready": false, "error": e}),
    };
    Ok(
        json!({"cli_version": VERSION, "supported_platform": supported().is_ok(), "app_directory": path, "vault_root": root, "installation": installation, "daemon": daemon, "read_only": true}),
    )
}
