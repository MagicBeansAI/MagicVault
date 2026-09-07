use clap::{Parser, Subcommand};
use magicvault_service::{
    broker::Broker, client::Client, human::NativeHuman, ipc, protocol::*, storage,
};
use std::{path::PathBuf, sync::Arc};
use uuid::Uuid;
mod setup;

#[derive(Parser)]
#[command(
    version,
    about = "Reference-only credential delivery: browser fills, new processes and HTTP requests"
)]
struct Cli {
    #[arg(long, global = true)]
    root: Option<PathBuf>,
    #[arg(long, global = true, default_value = "default")]
    profile: String,
    /// Private application directory, separate from the credential vault.
    #[arg(long, global = true)]
    app_dir: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Install a stable prebuilt bundle, initialize/start the service, and pair.
    Setup {
        #[arg(long)]
        bundle: Option<PathBuf>,
        /// Copy application files only: no vault, keychain, service or pairing.
        #[arg(long)]
        install_only: bool,
        #[arg(long, default_value = "MagicVault CLI and MCP")]
        label: String,
    },
    /// Read-only installation, daemon and extension-connection diagnostics.
    Doctor,
    /// Drain the owned service before activating a new immutable bundle.
    Upgrade {
        #[arg(long)]
        bundle: Option<PathBuf>,
    },
    /// Unload/remove owned integrations and archive app files; preserve the vault.
    Uninstall,
    /// Create a new standalone instance/keychain identity. Never adopts Magician data.
    Init,
    /// Run the standalone daemon in this user session.
    Serve,
    /// Pair this CLI/MCP profile after native human consent; never prints its token.
    Pair {
        #[arg(long)]
        label: String,
    },
    Status,
    /// Request human enrollment. Values are entered only in hidden native prompts.
    Enroll {
        #[arg(long)]
        label: String,
        #[arg(long = "field", required = true)]
        fields: Vec<String>,
    },
    ListCredentials,
    /// Request human permission to discover a selected credential's metadata.
    RequestAccess {
        #[arg(long)]
        credential_ref: String,
    },
    ApprovalStatus {
        #[arg(long)]
        approval_id: Uuid,
    },
    /// Attach an explicitly configured local CDP browser after human consent.
    RegisterCdp {
        #[arg(long)]
        label: String,
        #[arg(long)]
        endpoint: String,
    },
    ListBrowsers,
    BrowserTargets {
        #[arg(long)]
        browser_handle: Uuid,
    },
    DisconnectBrowser {
        #[arg(long)]
        browser_handle: Uuid,
    },
    /// Configure this client's selected fields and exact page/frame origins.
    /// Omitting --origin removes browser permission. Native consent is required.
    ConfigureBrowserCredential {
        #[arg(long)]
        credential_ref: String,
        #[arg(long = "field", required = true)]
        fields: Vec<String>,
        #[arg(long = "origin")]
        origins: Vec<String>,
    },
    /// Reference-only JSON request from a file. Never put values in this file.
    /// Returns a pending operation; use fill-status. Does not submit the form.
    SecureFill {
        #[arg(long)]
        request_file: PathBuf,
    },
    FillStatus {
        #[arg(long)]
        operation_id: Uuid,
    },
    CancelFill {
        #[arg(long)]
        operation_id: Uuid,
    },
    /// Human-authored, reference-only destination JSON; native approval required.
    RegisterDeliveryProfile {
        #[arg(long)]
        request_file: PathBuf,
    },
    ListDeliveryProfiles,
    /// Remove this client's profile and cancel its pending/running deliveries.
    RemoveDeliveryProfile {
        #[arg(long)]
        profile_id: Uuid,
    },
    /// Run a registered exact command once. Never returns stdout/stderr or secrets.
    SecureNewProcess {
        #[arg(long)]
        profile_id: Uuid,
        #[arg(long)]
        operation_id: Uuid,
    },
    /// Send a registered HTTP request once. Never returns response bodies/headers.
    SecureNewHttp {
        #[arg(long)]
        profile_id: Uuid,
        #[arg(long)]
        operation_id: Uuid,
    },
    DeliveryStatus {
        #[arg(long)]
        operation_id: Uuid,
    },
    CancelDelivery {
        #[arg(long)]
        operation_id: Uuid,
    },
    /// Install/remove only this instance's Chromium native messaging bridge.
    Extension {
        #[command(subcommand)]
        action: ExtensionAction,
    },
    RevokeClient {
        #[arg(long)]
        client_id: Uuid,
    },
    /// Ask the human to stop the daemon. Does not delete any data or key.
    Stop,
    /// Manage the macOS user-session LaunchAgent; never removes vault data.
    Service {
        #[command(subcommand)]
        action: ServiceAction,
    },
}

#[derive(Subcommand)]
enum ServiceAction {
    Install,
    Start,
    Stop,
    Remove,
}

#[derive(Subcommand)]
enum ExtensionAction {
    Install {
        #[arg(long, default_value = magicvault_service::native::EXTENSION_ID)]
        extension_id: String,
        #[arg(long)]
        host_executable: Option<PathBuf>,
    },
    Remove,
}

#[tokio::main]
async fn main() -> std::process::ExitCode {
    // No tracing subscriber: protocol/stdout and OS diagnostics never mix.
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(error)
            if matches!(
                error.kind(),
                clap::error::ErrorKind::DisplayHelp | clap::error::ErrorKind::DisplayVersion
            ) =>
        {
            let _ = error.print();
            return std::process::ExitCode::SUCCESS;
        }
        Err(_) => {
            eprintln!("{{\"error\":\"invalid_request\"}}");
            return std::process::ExitCode::FAILURE;
        }
    };
    match run(cli).await {
        Ok(value) => {
            println!("{value}");
            std::process::ExitCode::SUCCESS
        }
        Err(error) => {
            let value = serde_json::json!({"error": error});
            eprintln!("{value}");
            std::process::ExitCode::FAILURE
        }
    }
}

async fn run(cli: Cli) -> Result<serde_json::Value, ErrorCode> {
    let root = match cli.root {
        Some(root) => root,
        None => storage::default_root()?,
    };
    match cli.command {
        Command::Setup {
            bundle,
            install_only,
            label,
        } => setup::setup(root, cli.profile, cli.app_dir, bundle, install_only, label).await,
        Command::Doctor => setup::doctor(root, cli.profile, cli.app_dir).await,
        Command::Upgrade { bundle } => setup::upgrade(root, cli.app_dir, bundle).await,
        Command::Uninstall => setup::uninstall(root, cli.app_dir).await,
        Command::Extension { action } => {
            let profile = cli.profile;
            let app_dir = cli.app_dir;
            tokio::task::spawn_blocking(move || match action {
                ExtensionAction::Install {
                    extension_id,
                    host_executable,
                } => match host_executable {
                    Some(path) => {
                        magicvault_service::native::install(&root, &profile, &extension_id, &path)
                    }
                    None => setup::install_extension(&root, &profile, &extension_id, app_dir),
                },
                ExtensionAction::Remove => magicvault_service::native::remove(&root),
            })
            .await
            .map_err(|_| ErrorCode::Unavailable)??;
            Ok(
                serde_json::json!({"definition_updated":true,"vault_unchanged":true,"browser_restart_or_reconnect_required":true}),
            )
        }
        Command::Service { action } => {
            use magicvault_service::launch_agent;
            match action {
                ServiceAction::Install => {
                    let executable = std::env::current_exe().map_err(|_| ErrorCode::Unavailable)?;
                    let path = tokio::task::spawn_blocking(move || {
                        launch_agent::install(&root, &executable)
                    })
                    .await
                    .map_err(|_| ErrorCode::Unavailable)??;
                    Ok(serde_json::json!({"installed": true, "definition": path, "started": false}))
                }
                ServiceAction::Start => {
                    launch_agent::start(&root).await?;
                    Ok(serde_json::json!({"loaded":true,"readiness":"query status"}))
                }
                ServiceAction::Stop => {
                    launch_agent::stop(&root).await?;
                    Ok(serde_json::json!({"unloaded":true}))
                }
                ServiceAction::Remove => {
                    tokio::task::spawn_blocking(move || launch_agent::remove(&root))
                        .await
                        .map_err(|_| ErrorCode::Unavailable)??;
                    Ok(
                        serde_json::json!({"definition_removed":true,"running_service_unchanged":true,"vault_unchanged":true}),
                    )
                }
            }
        }
        Command::Init => {
            let instance = tokio::task::spawn_blocking(move || storage::initialize(&root))
                .await
                .map_err(|_| ErrorCode::Unavailable)??;
            Ok(serde_json::json!({"initialized": true, "instance_id": instance.id}))
        }
        Command::Serve => {
            let broker = tokio::task::spawn_blocking(move || {
                Broker::open(storage::open(&root)?, Arc::new(NativeHuman))
            })
            .await
            .map_err(|_| ErrorCode::Unavailable)??;
            let stop = broker.shutdown.clone();
            tokio::spawn(async move {
                #[cfg(unix)]
                {
                    if let Ok(mut terminate) =
                        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                    {
                        tokio::select! { _ = tokio::signal::ctrl_c() => {}, _ = terminate.recv() => {} }
                    } else {
                        let _ = tokio::signal::ctrl_c().await;
                    }
                }
                #[cfg(not(unix))]
                {
                    let _ = tokio::signal::ctrl_c().await;
                }
                stop.cancel();
            });
            ipc::serve(broker).await?;
            Ok(serde_json::json!({"stopped": true}))
        }
        Command::Pair { label } => {
            let id = Client::pair(root, &cli.profile, label).await?;
            Ok(serde_json::json!({"paired": true, "client_id": id}))
        }
        command => {
            let client = Client::load(root, &cli.profile)?;
            let request = match command {
                Command::Status => Request::Status,
                Command::Enroll { label, fields } => Request::Enroll(EnrollRequest {
                    label,
                    field_names: fields,
                }),
                Command::ListCredentials => Request::ListCredentials,
                Command::RequestAccess { credential_ref } => {
                    Request::RequestAccess(AccessRequest { credential_ref })
                }
                Command::ApprovalStatus { approval_id } => {
                    Request::ApprovalStatus(ApprovalQuery { approval_id })
                }
                Command::RegisterCdp { label, endpoint } => {
                    Request::RegisterCdp(RegisterCdp { label, endpoint })
                }
                Command::ListBrowsers => Request::ListBrowsers,
                Command::BrowserTargets { browser_handle } => {
                    Request::BrowserTargets(BrowserQuery { browser_handle })
                }
                Command::DisconnectBrowser { browser_handle } => {
                    Request::DisconnectBrowser(BrowserQuery { browser_handle })
                }
                Command::ConfigureBrowserCredential {
                    credential_ref,
                    fields,
                    origins,
                } => Request::ConfigureBrowserCredential(BrowserRule {
                    credential_ref,
                    field_names: fields,
                    origins,
                }),
                Command::SecureFill { request_file } => {
                    let request = tokio::task::spawn_blocking(move || {
                        use std::io::Read;
                        let mut options = std::fs::OpenOptions::new();
                        options.read(true);
                        #[cfg(unix)]
                        {
                            use std::os::unix::fs::OpenOptionsExt;
                            options.custom_flags(
                                libc::O_NONBLOCK | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                            );
                        }
                        let file = options
                            .open(request_file)
                            .map_err(|_| ErrorCode::InvalidRequest)?;
                        if !file
                            .metadata()
                            .map_err(|_| ErrorCode::InvalidRequest)?
                            .is_file()
                        {
                            return Err(ErrorCode::InvalidRequest);
                        }
                        let mut bytes = zeroize::Zeroizing::new(Vec::new());
                        file.take((MAX_FRAME_BYTES + 1) as u64)
                            .read_to_end(&mut bytes)
                            .map_err(|_| ErrorCode::InvalidRequest)?;
                        if bytes.len() > MAX_FRAME_BYTES {
                            return Err(ErrorCode::Capacity);
                        }
                        serde_json::from_slice::<SecureFill>(&bytes)
                            .map_err(|_| ErrorCode::InvalidRequest)
                    })
                    .await
                    .map_err(|_| ErrorCode::Unavailable)??;
                    Request::SecureFill(request)
                }
                Command::FillStatus { operation_id } => {
                    Request::FillStatus(FillQuery { operation_id })
                }
                Command::CancelFill { operation_id } => {
                    Request::CancelFill(FillQuery { operation_id })
                }
                Command::RegisterDeliveryProfile { request_file } => {
                    let bytes = read_delivery_profile(request_file).await?;
                    Request::RegisterDeliveryProfile(
                        serde_json::from_slice(&bytes).map_err(|_| ErrorCode::InvalidRequest)?,
                    )
                }
                Command::ListDeliveryProfiles => Request::ListDeliveryProfiles,
                Command::RemoveDeliveryProfile { profile_id } => {
                    Request::RemoveDeliveryProfile(ProfileQuery { profile_id })
                }
                Command::SecureNewProcess {
                    profile_id,
                    operation_id,
                } => Request::SecureNewProcess(SecureDelivery {
                    profile_id,
                    operation_id,
                }),
                Command::SecureNewHttp {
                    profile_id,
                    operation_id,
                } => Request::SecureNewHttp(SecureDelivery {
                    profile_id,
                    operation_id,
                }),
                Command::DeliveryStatus { operation_id } => {
                    Request::DeliveryStatus(FillQuery { operation_id })
                }
                Command::CancelDelivery { operation_id } => {
                    Request::CancelDelivery(FillQuery { operation_id })
                }
                Command::RevokeClient { client_id } => {
                    Request::RevokeClient(RevokeRequest { client_id })
                }
                Command::Stop => Request::Shutdown,
                _ => return Err(ErrorCode::InvalidRequest),
            };
            let response = if matches!(request, Request::Status) {
                Response::Status(client.status().await?)
            } else {
                client.call(request).await?
            };
            // Pairing capabilities must never enter the generic output path.
            if matches!(response, Response::Paired(_)) {
                return Err(ErrorCode::TransportUncertain);
            }
            serde_json::to_value(response).map_err(|_| ErrorCode::Unavailable)
        }
    }
}

async fn read_delivery_profile(path: PathBuf) -> Result<zeroize::Zeroizing<Vec<u8>>, ErrorCode> {
    tokio::task::spawn_blocking(move || {
        use std::io::Read;
        let mut options = std::fs::OpenOptions::new();
        options.read(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.custom_flags(libc::O_NONBLOCK | libc::O_NOFOLLOW | libc::O_CLOEXEC);
        }
        let file = options.open(path).map_err(|_| ErrorCode::InvalidRequest)?;
        if !file
            .metadata()
            .map_err(|_| ErrorCode::InvalidRequest)?
            .is_file()
        {
            return Err(ErrorCode::InvalidRequest);
        }
        let mut bytes = zeroize::Zeroizing::new(Vec::new());
        file.take((MAX_FRAME_BYTES + 1) as u64)
            .read_to_end(&mut bytes)
            .map_err(|_| ErrorCode::InvalidRequest)?;
        if bytes.len() > MAX_FRAME_BYTES {
            return Err(ErrorCode::Capacity);
        }
        Ok(bytes)
    })
    .await
    .map_err(|_| ErrorCode::Unavailable)?
}
