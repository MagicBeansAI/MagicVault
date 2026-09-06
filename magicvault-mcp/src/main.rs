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
fn main() -> std::process::ExitCode {
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
    if let Err(code) = run_stdio_runtime(run(args)) {
        eprintln!("{}", serde_json::json!({"error":code}));
        return std::process::ExitCode::FAILURE;
    }
    std::process::ExitCode::SUCCESS
}

fn run_stdio_runtime(future: impl std::future::Future<Output = Result<(), ErrorCode>>) -> Result<(), ErrorCode> {
    let runtime = tokio::runtime::Builder::new_multi_thread().enable_all().build()
        .map_err(|_| ErrorCode::Unavailable)?;
    let result = runtime.block_on(future);
    // Tokio stdin/stdout use blocking tasks that cannot always be cancelled.
    // This executable is only a daemon client: it owns NO durable commits.
    // Bound teardown after the SDK stops, then main returns and the process
    // ends any blocked stdio threads. The custody daemon keeps its full drain.
    runtime.shutdown_timeout(std::time::Duration::from_millis(250));
    result
}
async fn run(args: Args) -> Result<(), ErrorCode> {
    let root = args.root.map(Ok).unwrap_or_else(storage::default_root)?;
    let client = Client::load(root, &args.profile)?;
    let transport = BoundedTransport::new(tokio::io::stdin(), tokio::io::stdout());
    let server = MagicVaultMcp::new(client).serve(transport).await.map_err(|_| ErrorCode::Unavailable)?;
    server.waiting().await.map_err(|_| ErrorCode::Unavailable)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idle_blocking_stdio_cannot_hold_runtime_teardown() {
        let (release, wait) = std::sync::mpsc::channel::<()>();
        let (started, ready) = tokio::sync::oneshot::channel();
        // Model Tokio's uninterruptible read. The timeout in run_stdio_runtime
        // must return before we release it; an ordinary runtime Drop deadlocks.
        let result = run_stdio_runtime(async move {
            tokio::task::spawn_blocking(move || { let _ = started.send(()); let _ = wait.recv(); });
            ready.await.map_err(|_| ErrorCode::Unavailable)?;
            Ok(())
        });
        drop(release);
        assert_eq!(result, Ok(()));
    }
}
