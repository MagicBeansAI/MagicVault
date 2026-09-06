use std::{path::PathBuf, sync::Arc};
use clap::{Parser, Subcommand};
use magicvault_service::{broker::Broker, client::Client, human::NativeHuman, ipc, protocol::*, storage};
use uuid::Uuid;

#[derive(Parser)]
#[command(version, about = "Reference-only credential foundation; browser/HTTP/process effects are not implemented yet")]
struct Cli {
    #[arg(long, global = true)] root: Option<PathBuf>,
    #[arg(long, global = true, default_value = "default")] profile: String,
    #[command(subcommand)] command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Create a new standalone instance/keychain identity. Never adopts Magician data.
    Init,
    /// Run the standalone daemon in this user session.
    Serve,
    /// Pair this CLI/MCP profile after native human consent; never prints its token.
    Pair { #[arg(long)] label: String },
    Status,
    /// Request human enrollment. Values are entered only in hidden native prompts.
    Enroll { #[arg(long)] label: String, #[arg(long = "field", required = true)] fields: Vec<String> },
    ListCredentials,
    /// Request human permission to discover a selected credential's metadata.
    RequestAccess { #[arg(long)] credential_ref: String },
    ApprovalStatus { #[arg(long)] approval_id: Uuid },
    RevokeClient { #[arg(long)] client_id: Uuid },
    /// Ask the human to stop the daemon. Does not delete any data or key.
    Stop,
    /// Manage the macOS user-session LaunchAgent; never removes vault data.
    Service { #[command(subcommand)] action: ServiceAction },
}

#[derive(Subcommand)]
enum ServiceAction { Install, Start, Stop, Remove }

#[tokio::main]
async fn main() -> std::process::ExitCode {
    // No tracing subscriber: protocol/stdout and OS diagnostics never mix.
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(error) if matches!(error.kind(), clap::error::ErrorKind::DisplayHelp | clap::error::ErrorKind::DisplayVersion) => {
            let _ = error.print();
            return std::process::ExitCode::SUCCESS;
        },
        Err(_) => {
            eprintln!("{{\"error\":\"invalid_request\"}}");
            return std::process::ExitCode::FAILURE;
        },
    };
    match run(cli).await {
        Ok(value) => { println!("{value}"); std::process::ExitCode::SUCCESS },
        Err(error) => {
            let value = serde_json::json!({"error": error});
            eprintln!("{value}");
            std::process::ExitCode::FAILURE
        },
    }
}

async fn run(cli: Cli) -> Result<serde_json::Value, ErrorCode> {
    let root = match cli.root { Some(root) => root, None => storage::default_root()? };
    match cli.command {
        Command::Service { action } => {
            use magicvault_service::launch_agent;
            match action {
                ServiceAction::Install => {
                    let executable = std::env::current_exe().map_err(|_| ErrorCode::Unavailable)?;
                    let path = tokio::task::spawn_blocking(move || launch_agent::install(&root, &executable)).await.map_err(|_| ErrorCode::Unavailable)??;
                    Ok(serde_json::json!({"installed": true, "definition": path, "started": false}))
                },
                ServiceAction::Start => { launch_agent::start(&root).await?; Ok(serde_json::json!({"loaded":true,"readiness":"query status"})) },
                ServiceAction::Stop => { launch_agent::stop(&root).await?; Ok(serde_json::json!({"unloaded":true})) },
                ServiceAction::Remove => {
                    tokio::task::spawn_blocking(move || launch_agent::remove(&root)).await.map_err(|_| ErrorCode::Unavailable)??;
                    Ok(serde_json::json!({"definition_removed":true,"running_service_unchanged":true,"vault_unchanged":true}))
                },
            }
        },
        Command::Init => {
            let instance = tokio::task::spawn_blocking(move || storage::initialize(&root)).await.map_err(|_| ErrorCode::Unavailable)??;
            Ok(serde_json::json!({"initialized": true, "instance_id": instance.id}))
        },
        Command::Serve => {
            let broker = tokio::task::spawn_blocking(move || Broker::open(storage::open(&root)?, Arc::new(NativeHuman))).await.map_err(|_| ErrorCode::Unavailable)??;
            let stop = broker.shutdown.clone();
            tokio::spawn(async move {
                #[cfg(unix)]
                {
                    if let Ok(mut terminate) = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
                        tokio::select! { _ = tokio::signal::ctrl_c() => {}, _ = terminate.recv() => {} }
                    } else { let _ = tokio::signal::ctrl_c().await; }
                }
                #[cfg(not(unix))]
                { let _ = tokio::signal::ctrl_c().await; }
                stop.cancel();
            });
            ipc::serve(broker).await?;
            Ok(serde_json::json!({"stopped": true}))
        },
        Command::Pair { label } => {
            let id = Client::pair(root, &cli.profile, label).await?;
            Ok(serde_json::json!({"paired": true, "client_id": id}))
        },
        command => {
            let client = Client::load(root, &cli.profile)?;
            let request = match command {
                Command::Status => Request::Status,
                Command::Enroll { label, fields } => Request::Enroll(EnrollRequest { label, field_names: fields }),
                Command::ListCredentials => Request::ListCredentials,
                Command::RequestAccess { credential_ref } => Request::RequestAccess(AccessRequest { credential_ref }),
                Command::ApprovalStatus { approval_id } => Request::ApprovalStatus(ApprovalQuery { approval_id }),
                Command::RevokeClient { client_id } => Request::RevokeClient(RevokeRequest { client_id }),
                Command::Stop => Request::Shutdown,
                _ => return Err(ErrorCode::InvalidRequest),
            };
            let response = if matches!(request, Request::Status) {
                Response::Status(client.status().await?)
            } else { client.call(request).await? };
            // Pairing capabilities must never enter the generic output path.
            if matches!(response, Response::Paired(_)) { return Err(ErrorCode::TransportUncertain); }
            serde_json::to_value(response).map_err(|_| ErrorCode::Unavailable)
        },
    }
}
