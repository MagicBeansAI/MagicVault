//! Chrome launches this binary through an installed private wrapper. stdout is
//! exclusively the native protocol, never CLI output, tokens or diagnostics.
use clap::Parser;
use magicvault_effect::bridge::{self, BridgeCommand, BridgeReply, BridgeRequest, BridgeResult};
use magicvault_service::{
    native::{self, BrowserHello, NativeGreeting, NATIVE_VERSION},
    protocol::ErrorCode,
};
use std::{path::PathBuf, time::Duration};
use zeroize::Zeroizing;

#[derive(Parser)]
struct Args {
    #[arg(long)]
    config: PathBuf,
    origin: String,
}

fn main() -> std::process::ExitCode {
    // Never let clap echo rejected invocation arguments into Chrome's logs.
    let Ok(args) = Args::try_parse() else {
        return std::process::ExitCode::FAILURE;
    };
    let Ok(runtime) = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
    else {
        return std::process::ExitCode::FAILURE;
    };
    let result = runtime.block_on(async {
        let result = run(args).await;
        #[cfg(unix)]
        if let Err(code) = result {
            // Closed codes only, including denial/revocation. Chrome's generic
            // host-exited diagnostic cannot distinguish safe retry from refusal.
            if let Ok(bytes) = serde_json::to_vec(&NativeGreeting::Error {
                version: NATIVE_VERSION,
                code,
            }) {
                let _ = tokio::time::timeout(
                    Duration::from_secs(5),
                    bridge::write_frame(&mut tokio::io::stdout(), &bytes),
                )
                .await;
            }
        }
        result
    });
    // The host owns no custody writes. Bound uncancellable stdio teardown;
    // the separate daemon remains responsible for draining durable writes.
    runtime.shutdown_timeout(Duration::from_millis(250));
    if result.is_ok() {
        std::process::ExitCode::SUCCESS
    } else {
        std::process::ExitCode::FAILURE
    }
}

#[cfg(unix)]
async fn run(args: Args) -> Result<(), ErrorCode> {
    let config = native::load_config(&args.config, &args.origin)?;
    let mut input = tokio::io::stdin();
    let mut output = tokio::io::stdout();
    let bytes = tokio::time::timeout(Duration::from_secs(5), bridge::read_frame(&mut input))
        .await
        .map_err(|_| ErrorCode::UnsupportedVersion)??;
    let browser: BrowserHello =
        serde_json::from_slice(&bytes).map_err(|_| ErrorCode::UnsupportedVersion)?;
    drop(bytes);
    if !browser.valid() {
        return Err(ErrorCode::UnsupportedVersion);
    }
    let mut hello = native::hello(&config)?;
    hello.browser = Some(browser);
    let mut socket = tokio::time::timeout(
        Duration::from_secs(3),
        tokio::net::UnixStream::connect(config.root.join("bridge.sock")),
    )
    .await
    .map_err(|_| ErrorCode::TransportUnavailable)?
    .map_err(|_| ErrorCode::TransportUnavailable)?;
    if !socket
        .peer_cred()
        .is_ok_and(|peer| peer.uid() == unsafe { libc::geteuid() })
    {
        return Err(ErrorCode::Unauthorized);
    }
    let bytes = Zeroizing::new(serde_json::to_vec(&hello).map_err(|_| ErrorCode::Unavailable)?);
    tokio::time::timeout(
        Duration::from_secs(5),
        bridge::write_frame(&mut socket, &bytes),
    )
    .await
    .map_err(|_| ErrorCode::TransportUncertain)??;
    drop(bytes);
    drop(hello);
    let ready = tokio::select! {
        ready = tokio::time::timeout(Duration::from_secs(190), bridge::read_frame(&mut socket)) => ready.map_err(|_| ErrorCode::Expired)??,
        _ = bridge::read_frame(&mut input) => return Ok(()),
    };
    let ready: NativeGreeting =
        serde_json::from_slice(&ready).map_err(|_| ErrorCode::UnsupportedVersion)?;
    match &ready {
        NativeGreeting::Ready {
            version,
            browser_handle,
        } if *version == NATIVE_VERSION && !browser_handle.is_nil() => {}
        NativeGreeting::Error { version, code } if *version == NATIVE_VERSION => return Err(*code),
        _ => return Err(ErrorCode::UnsupportedVersion),
    }
    let bytes = serde_json::to_vec(&ready).map_err(|_| ErrorCode::Unavailable)?;
    tokio::time::timeout(
        Duration::from_secs(5),
        bridge::write_frame(&mut output, &bytes),
    )
    .await
    .map_err(|_| ErrorCode::TransportUncertain)??;
    loop {
        let bytes = tokio::select! {
            command = bridge::read_frame(&mut socket) => command?,
            // Only replies are accepted. EOF or unsolicited input closes the
            // integration without retrying/reopening an uncertain effect.
            _ = bridge::read_frame(&mut input) => return Ok(()),
        };
        let command: BridgeCommand =
            serde_json::from_slice(&bytes).map_err(|_| ErrorCode::TransportUncertain)?;
        let count = match &command.request {
            BridgeRequest::Targets | BridgeRequest::FilteredTargets(_) => None,
            BridgeRequest::Fill { fields, .. } => Some(fields.len()),
        };
        let id = command.request_id;
        drop(command);
        let work = async {
            bridge::write_frame(&mut output, &bytes).await?;
            let reply = bridge::read_frame(&mut input).await?;
            let reply: BridgeReply =
                serde_json::from_slice(&reply).map_err(|_| ErrorCode::TransportUncertain)?;
            if reply.request_id != id {
                return Err(ErrorCode::TransportUncertain);
            }
            let valid = match (&reply.result, count) {
                (BridgeResult::Targets(targets), None) => {
                    targets.len() <= magicvault_service::protocol::MAX_TARGETS
                        && targets.iter().all(|t| t.valid())
                }
                (BridgeResult::Filled(outcome), Some(count)) => outcome.valid(count),
                (BridgeResult::Error(_), None) => true,
                _ => false,
            };
            if !valid {
                return Err(ErrorCode::TransportUncertain);
            }
            let reply = serde_json::to_vec(&reply).map_err(|_| ErrorCode::Unavailable)?;
            bridge::write_frame(&mut socket, &reply).await
        };
        tokio::time::timeout(Duration::from_secs(35), work)
            .await
            .map_err(|_| ErrorCode::TransportUncertain)??;
    }
}

#[cfg(not(unix))]
async fn run(_: Args) -> Result<(), ErrorCode> {
    Err(ErrorCode::Unavailable)
}
