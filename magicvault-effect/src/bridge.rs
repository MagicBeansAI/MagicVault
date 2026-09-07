//! Trusted native integration wire. This protocol is not exposed on MCP/rpc.sock.
//! The service authenticates the host before constructing a bridge adapter.
use crate::{BrowserAdapter, MaterialField, Outcome, Target};
use async_trait::async_trait;
use magicvault_protocol::{ErrorCode, FILL_TIMEOUT_SECS, MAX_FIELDS, MAX_TARGETS};
use serde::{Deserialize, Serialize};
use std::{sync::Arc, time::Duration};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;
use zeroize::Zeroizing;

pub const BRIDGE_VERSION: u32 = 1;
pub const MAX_BRIDGE_BYTES: usize = 256 * 1024;
pub const HOST_NAME: &str = "ai.magicbeans.magicvault";

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BridgeCommand {
    pub request_id: Uuid,
    pub request: BridgeRequest,
}

#[derive(Serialize, Deserialize)]
#[serde(
    tag = "method",
    content = "params",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum BridgeRequest {
    Targets,
    Fill {
        target: Target,
        fields: Vec<MaterialField>,
    },
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BridgeReply {
    pub request_id: Uuid,
    pub result: BridgeResult,
}

#[derive(Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "data",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum BridgeResult {
    Targets(Vec<Target>),
    Filled(Outcome),
    Error(ErrorCode),
}

pub fn valid_extension_id(id: &str) -> bool {
    id.len() == 32 && id.bytes().all(|b| (b'a'..=b'p').contains(&b))
}

#[cfg(unix)]
pub async fn read_frame<R: tokio::io::AsyncRead + Unpin>(
    input: &mut R,
) -> Result<Zeroizing<Vec<u8>>, ErrorCode> {
    use tokio::io::AsyncReadExt;
    let length = input
        .read_u32_le()
        .await
        .map_err(|_| ErrorCode::TransportUnavailable)? as usize;
    if length == 0 || length > MAX_BRIDGE_BYTES {
        return Err(ErrorCode::Capacity);
    }
    let mut bytes = Zeroizing::new(vec![0; length]);
    input
        .read_exact(&mut bytes)
        .await
        .map_err(|_| ErrorCode::TransportUncertain)?;
    Ok(bytes)
}

#[cfg(unix)]
pub async fn write_frame<W: tokio::io::AsyncWrite + Unpin>(
    output: &mut W,
    bytes: &[u8],
) -> Result<(), ErrorCode> {
    use tokio::io::AsyncWriteExt;
    if bytes.is_empty() || bytes.len() > MAX_BRIDGE_BYTES {
        return Err(ErrorCode::Capacity);
    }
    output
        .write_u32_le(bytes.len() as u32)
        .await
        .map_err(|_| ErrorCode::TransportUncertain)?;
    output
        .write_all(bytes)
        .await
        .map_err(|_| ErrorCode::TransportUncertain)?;
    output
        .flush()
        .await
        .map_err(|_| ErrorCode::TransportUncertain)
}

#[cfg(unix)]
pub struct NativeBridge {
    socket: Mutex<Option<tokio::net::UnixStream>>,
    stop: CancellationToken,
    initialized: std::sync::atomic::AtomicBool,
}

#[cfg(unix)]
impl NativeBridge {
    /// Must only be called after peer UID, capability, extension identity and
    /// daemon-owned registration consent have all been authenticated.
    pub fn authenticated(socket: tokio::net::UnixStream) -> Arc<Self> {
        Arc::new(Self {
            socket: Mutex::new(Some(socket)),
            stop: CancellationToken::new(),
            initialized: std::sync::atomic::AtomicBool::new(false),
        })
    }

    pub async fn initialize(&self, greeting: &[u8]) -> Result<(), ErrorCode> {
        let mut guard = self.socket.try_lock().map_err(|_| ErrorCode::Busy)?;
        let socket = guard.as_mut().ok_or(ErrorCode::Unavailable)?;
        match tokio::time::timeout(Duration::from_secs(5), write_frame(socket, greeting)).await {
            Ok(Ok(())) => {
                self.initialized
                    .store(true, std::sync::atomic::Ordering::Release);
                Ok(())
            }
            _ => {
                guard.take();
                self.stop.cancel();
                Err(ErrorCode::TransportUncertain)
            }
        }
    }

    async fn exchange(
        &self,
        request: BridgeRequest,
        cancel: CancellationToken,
    ) -> Result<BridgeResult, ErrorCode> {
        if self.stop.is_cancelled() || cancel.is_cancelled() {
            return Err(ErrorCode::Cancelled);
        }
        if !self.initialized.load(std::sync::atomic::Ordering::Acquire) {
            return Err(ErrorCode::Busy);
        }
        let mut guard = self.socket.try_lock().map_err(|_| ErrorCode::Busy)?;
        let mut socket = guard.take().ok_or(ErrorCode::Unavailable)?;
        let command = BridgeCommand {
            request_id: Uuid::new_v4(),
            request,
        };
        let bytes =
            Zeroizing::new(serde_json::to_vec(&command).map_err(|_| ErrorCode::Unavailable)?);
        let work = async {
            write_frame(&mut socket, &bytes).await?;
            let bytes = read_frame(&mut socket).await?;
            let reply: BridgeReply =
                serde_json::from_slice(&bytes).map_err(|_| ErrorCode::TransportUncertain)?;
            if reply.request_id != command.request_id {
                return Err(ErrorCode::TransportUncertain);
            }
            Ok(reply.result)
        };
        let result = tokio::select! {
            _ = self.stop.cancelled() => Err(ErrorCode::TransportUncertain),
            _ = cancel.cancelled() => Err(ErrorCode::TransportUncertain),
            result = tokio::time::timeout(Duration::from_secs(FILL_TIMEOUT_SECS), work) => result.unwrap_or(Err(ErrorCode::TransportUncertain)),
        };
        if result.is_ok() {
            *guard = Some(socket);
        } else {
            self.stop.cancel();
        }
        result
    }
}

#[cfg(unix)]
#[async_trait]
impl BrowserAdapter for NativeBridge {
    async fn targets(&self, cancel: CancellationToken) -> Result<Vec<Target>, ErrorCode> {
        match self.exchange(BridgeRequest::Targets, cancel).await? {
            BridgeResult::Targets(targets)
                if targets.len() <= MAX_TARGETS && targets.iter().all(Target::valid) =>
            {
                Ok(targets)
            }
            BridgeResult::Error(error) => Err(error),
            _ => {
                self.disconnect();
                Err(ErrorCode::TransportUncertain)
            }
        }
    }

    async fn fill(
        &self,
        target: &Target,
        fields: Vec<MaterialField>,
        cancel: CancellationToken,
    ) -> Outcome {
        let count = fields.len();
        if !target.valid()
            || count == 0
            || count > MAX_FIELDS
            || fields.iter().any(|f| {
                !magicvault_protocol::valid_css(&f.css)
                    || f.value.is_empty()
                    || f.value.len() > 4096
            })
        {
            return Outcome::failed(count.min(MAX_FIELDS), ErrorCode::InvalidRequest);
        }
        match self
            .exchange(
                BridgeRequest::Fill {
                    target: target.clone(),
                    fields,
                },
                cancel,
            )
            .await
        {
            Ok(BridgeResult::Filled(outcome)) if outcome.valid(count) => outcome,
            Err(ErrorCode::Busy) => Outcome::failed(count, ErrorCode::Busy),
            Err(ErrorCode::Cancelled) => Outcome::failed(count, ErrorCode::Cancelled),
            // Unstructured or wrong-kind replies never become optimistic
            // failures: the remote field may already have been written.
            _ => {
                self.disconnect();
                Outcome::uncertain(count)
            }
        }
    }

    fn disconnect(&self) {
        self.stop.cancel();
        if let Ok(mut guard) = self.socket.try_lock() {
            guard.take();
        }
    }
    fn connected(&self) -> bool {
        !self.stop.is_cancelled()
    }
}
