use std::path::PathBuf;
use clap::Parser;
use magicvault_service::{client::Client, protocol::ErrorCode, storage};
use magicvault_mcp::{MagicVaultMcp, transport::BoundedTransport};
use rmcp::ServiceExt;

#[derive(Parser)]
#[command(version, about = "MagicVault reference-only stdio MCP surface")]
struct Args {
    #[arg(long)] root: Option<PathBuf>,
    #[arg(long, default_value = "default")] profile: String,
}
#[tokio::main]
async fn main() -> std::process::ExitCode {
    let args = match Args::try_parse() {
        Ok(args) => args,
        Err(error) if matches!(error.kind(), clap::error::ErrorKind::DisplayHelp | clap::error::ErrorKind::DisplayVersion) => {
            let _ = error.print();
            return std::process::ExitCode::SUCCESS;
        },
        Err(_) => {
            eprintln!("{{\"error\":\"invalid_request\"}}");
            return std::process::ExitCode::FAILURE;
        },
    };
    if let Err(code) = run(args).await {
        eprintln!("{}", serde_json::json!({"error":code}));
        return std::process::ExitCode::FAILURE;
    }
    std::process::ExitCode::SUCCESS
}
async fn run(args: Args) -> Result<(), ErrorCode> {
    let root = args.root.map(Ok).unwrap_or_else(storage::default_root)?;
    let client = Client::load(root, &args.profile)?;
    let transport = BoundedTransport::new(tokio::io::stdin(), tokio::io::stdout());
    let server = MagicVaultMcp::new(client).serve(transport).await.map_err(|_| ErrorCode::Unavailable)?;
    server.waiting().await.map_err(|_| ErrorCode::Unavailable)?;
    Ok(())
}
