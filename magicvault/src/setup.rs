//! Human-invoked onboarding only. npm and MCP never invoke these operations.
use magicvault_service::{
    client::Client,
    installation::{self, Installation},
    launch_agent, native,
    protocol::{valid_name, BrowserBackend, ErrorCode, Request, Response},
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
        "extension_directory": app.extension(), "extension_id": native::EXTENSION_ID,
        "mcpServers": {"magicvault": {"command": app.executable("magicvault-mcp")?, "args": ["--root", root, "--profile", profile]}},
        "next_steps": ["Enroll credentials with magicvault enroll; values belong only in native prompts.", "For CDP, register a local debugging endpoint. For the extension, load extension_directory in Chrome; normal setup installs the bridge and the extension connects automatically. Approve the first native browser-profile dialog.", "Register reference-only destination profiles before new process or HTTP delivery."]
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
    let vault = root.clone();
    let executable = app.executable("magicvault-native-host")?;
    let host_profile = profile.clone();
    blocking(move || native::install(&vault, &host_profile, native::EXTENSION_ID, &executable))
        .await?;
    output["native_host_installed"] = json!(true);
    // Advisory only: a closed/paused browser must not fail setup or gate HTTP,
    // process or CDP use. Never wait for a human prompt or trigger connection.
    output["extension"] = extension_status(&root, &profile).await;
    Ok(output)
}

async fn extension_status(root: &Path, profile: &str) -> Value {
    let vault = root.to_owned();
    let registration = blocking(move || native::inspect_registration(&vault)).await;
    let browsers = tokio::time::timeout(Duration::from_secs(2), async {
        Client::load(root.to_owned(), profile)?
            .call(Request::ListBrowsers)
            .await
    })
    .await
    .unwrap_or(Err(ErrorCode::TransportUnavailable));
    extension_report(profile, registration, browsers)
}

fn extension_report(
    profile: &str,
    registration: Result<Option<native::RegistrationInspection>, ErrorCode>,
    browsers: Result<Response, ErrorCode>,
) -> Value {
    let mut next_steps = Vec::new();
    let native_host = match registration {
        Ok(Some(info)) => {
            if !info.missing_files.is_empty() {
                next_steps.push("Native-host files are missing. Rerun normal setup for the bundled extension, or extension install for your custom identity; do not delete the vault.");
            }
            if info.client_profile != profile {
                next_steps.push("The native host uses a different CLI/MCP profile. Run doctor with its client_profile to check that profile's connections; registrations are OS-user-wide.");
            }
            json!({"state": if info.missing_files.is_empty() {"verified"} else {"incomplete"},
                "extension_id": info.extension_id, "client_profile": info.client_profile,
                "matches_selected_profile": info.client_profile == profile, "missing_files": info.missing_files})
        }
        Ok(None) => {
            next_steps.push("Native-host registration is not configured for this vault. Run normal setup, or extension install for a source/custom build.");
            json!({"state": "not_configured"})
        }
        Err(e) => {
            next_steps.push("Native-host registration could not be verified. Inspect the closed error; foreign or modified definitions require deliberate recovery, not deletion or silent replacement.");
            json!({"state": "unavailable", "error": e})
        }
    };
    let connections = match browsers {
        Ok(Response::Browsers(rows)) => Ok(rows
            .iter()
            .filter(|b| b.backend == BrowserBackend::Extension)
            .count()),
        Ok(_) => Err(ErrorCode::TransportUncertain),
        Err(e) => Err(e),
    };
    let connection = match connections {
        Ok(count) => {
            json!({"state": if count > 0 {"connected"} else {"not_connected"}, "connected_profiles": count})
        }
        Err(e) => json!({"state": "unavailable", "connected_profiles": null, "error": e}),
    };
    if !matches!(connections, Ok(1..)) {
        next_steps.push("Extension installation is unconfirmed, not necessarily absent. Open Chrome/Chromium and check the extension is installed and enabled; inspect its setup page for Pause, approval or retry status. Automatic retries may take several minutes. Then rerun doctor with the native host's client profile.");
    }
    json!({
        "bundled_extension_id": native::EXTENSION_ID,
        "client_profile": profile,
        "native_host": native_host,
        "connection": connection,
        "browser_installation": if matches!(connections, Ok(1..)) {"confirmed_connected"} else {"unconfirmed"},
        "scope": "Snapshot of extension connections visible to this paired CLI/MCP profile, not all browser profiles. Connection confirms presence, not website permission or credential-fill authorization.",
        "next_steps": next_steps,
    })
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
        assert_eq!(report["extension"]["browser_installation"], "unconfirmed");
        assert_eq!(report["extension"]["connection"]["state"], "unavailable");
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

    #[test]
    fn extension_report_does_not_confuse_registration_or_cdp_with_installed_extension() {
        use magicvault_service::protocol::BrowserInfo;
        let registration = || {
            Some(native::RegistrationInspection {
                extension_id: native::EXTENSION_ID.into(),
                client_profile: "native-client".into(),
                missing_files: vec![],
            })
        };
        let report = extension_report(
            "agent",
            Ok(registration()),
            Ok(Response::Browsers(vec![BrowserInfo {
                browser_handle: uuid::Uuid::new_v4(),
                label: "CDP".into(),
                backend: BrowserBackend::Cdp,
            }])),
        );
        assert_eq!(report["native_host"]["state"], "verified");
        assert_eq!(report["native_host"]["matches_selected_profile"], false);
        assert_eq!(report["browser_installation"], "unconfirmed");
        assert_eq!(report["connection"]["connected_profiles"], 0);
        assert!(report["next_steps"].as_array().unwrap().len() >= 2);
        let mut incomplete = registration().unwrap();
        incomplete.missing_files.push("/synthetic/missing".into());
        let report = extension_report(
            "native-client",
            Ok(Some(incomplete)),
            Err(ErrorCode::Unauthorized),
        );
        assert_eq!(report["native_host"]["state"], "incomplete");
        assert_eq!(report["browser_installation"], "unconfirmed");
        assert!(report["connection"]["connected_profiles"].is_null());
        assert_eq!(report["connection"]["error"], "unauthorized");
        let report = extension_report("agent", Err(ErrorCode::Conflict), Ok(Response::Revoked));
        assert_eq!(report["native_host"]["error"], "conflict");
        assert_eq!(report["connection"]["error"], "transport_uncertain");
    }

    #[tokio::test]
    async fn doctor_reads_authenticated_connection_metadata_over_ipc_without_new_authority() {
        use magicvault_service::{ipc, protocol::*};
        let root = root();
        let token = "b".repeat(64);
        let id = uuid::Uuid::new_v4();
        let pairing_file = root.path().join("client-agent.json");
        fs::write(
            &pairing_file,
            serde_json::to_vec(&Pairing {
                client_id: id,
                token: token.clone(),
            })
            .unwrap(),
        )
        .unwrap();
        fs::set_permissions(&pairing_file, fs::Permissions::from_mode(0o600)).unwrap();
        let listener = tokio::net::UnixListener::bind(root.path().join("rpc.sock")).unwrap();
        fs::set_permissions(
            root.path().join("rpc.sock"),
            fs::Permissions::from_mode(0o600),
        )
        .unwrap();
        let server = tokio::spawn(async move {
            let epoch = uuid::Uuid::new_v4();
            for n in 0..3 {
                let (mut stream, _) = listener.accept().await.unwrap();
                let bytes = ipc::read_frame(&mut stream, MAX_FRAME_BYTES).await.unwrap();
                let envelope: Envelope = serde_json::from_slice(&bytes).unwrap();
                assert_eq!(envelope.token.as_deref(), Some(token.as_str()));
                let response = match envelope.request {
                    Request::Status if n < 2 => Response::Status(ServiceStatus {
                        epoch,
                        client_id: Some(id),
                        ready: true,
                        effects: vec![],
                    }),
                    Request::ListBrowsers if n == 2 => {
                        assert_eq!(envelope.epoch, Some(epoch));
                        Response::Browsers(
                            [
                                BrowserBackend::Cdp,
                                BrowserBackend::Extension,
                                BrowserBackend::Extension,
                            ]
                            .into_iter()
                            .map(|backend| BrowserInfo {
                                browser_handle: uuid::Uuid::new_v4(),
                                label: "Synthetic browser".into(),
                                backend,
                            })
                            .collect(),
                        )
                    }
                    _ => panic!("diagnostic must only read status and browser metadata"),
                };
                ipc::write_frame(
                    &mut stream,
                    &serde_json::to_vec(&Reply::Ok(response)).unwrap(),
                    MAX_REPLY_BYTES,
                )
                .await
                .unwrap();
            }
        });
        let report = tokio::time::timeout(
            Duration::from_secs(5),
            doctor(
                root.path().to_owned(),
                "agent".into(),
                Some(root.path().join("missing-app")),
            ),
        )
        .await
        .unwrap()
        .unwrap();
        server.await.unwrap();
        assert_eq!(report["extension"]["connection"]["connected_profiles"], 2);
        assert_eq!(
            report["extension"]["browser_installation"],
            "confirmed_connected"
        );
        assert!(!report.to_string().contains(&"b".repeat(64)));
        assert!(!root.path().join("missing-app").exists());
        assert!(!root.path().join("native-host.json").exists());
        assert!(!root.path().join("native-install.lock").exists());
    }

    #[tokio::test]
    async fn extension_probe_times_out_without_claiming_browser_is_absent() {
        let root = root();
        let _listener = tokio::net::UnixListener::bind(root.path().join("rpc.sock")).unwrap();
        fs::set_permissions(
            root.path().join("rpc.sock"),
            fs::Permissions::from_mode(0o600),
        )
        .unwrap();
        let report = tokio::time::timeout(
            Duration::from_secs(4),
            extension_status(root.path(), "agent"),
        )
        .await
        .unwrap();
        assert_eq!(report["browser_installation"], "unconfirmed");
        assert_eq!(report["connection"]["state"], "unavailable");
        assert!(report["connection"]["connected_profiles"].is_null());
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
    let vault = root.clone();
    let app = blocking(move || {
        app.activate(staged)?;
        if initialized && exists(&native::config_path(&vault))? {
            let bytes = storage::read_private(&native::config_path(&vault), 8192)?;
            let config: native::HostConfig =
                serde_json::from_slice(&bytes).map_err(|_| ErrorCode::Conflict)?;
            // Preserve an application builder's explicit custom extension ID.
            // Setup selects the bundled identity; upgrade repairs only this
            // already managed registration and never changes the selected ID.
            native::install(
                &vault,
                &config.profile,
                &config.extension_id,
                &app.executable("magicvault-native-host")?,
            )?;
        }
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
    let daemon = match tokio::time::timeout(Duration::from_secs(2), async {
        Client::load(root.clone(), &profile)?.status().await
    })
    .await
    .unwrap_or(Err(ErrorCode::TransportUnavailable))
    {
        Ok(s) => serde_json::to_value(s).map_err(|_| ErrorCode::Unavailable)?,
        Err(e) => json!({"ready": false, "error": e}),
    };
    let extension = extension_status(&root, &profile).await;
    Ok(
        json!({"cli_version": VERSION, "supported_platform": supported().is_ok(), "app_directory": path, "vault_root": root, "installation": installation, "daemon": daemon, "extension": extension, "read_only": true}),
    )
}
