//! Length-prefixed local frames, peer UID authentication and bounded admission.
use crate::broker::Broker;
use magicvault_protocol::*;
use std::sync::Arc;
use zeroize::Zeroizing;

#[cfg(unix)]
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{UnixListener, UnixStream},
};

#[cfg(unix)]
pub async fn read_frame(
    stream: &mut UnixStream,
    maximum: usize,
) -> Result<Zeroizing<Vec<u8>>, ErrorCode> {
    let length = stream
        .read_u32_le()
        .await
        .map_err(|_| ErrorCode::TransportUnavailable)? as usize;
    if length == 0 || length > maximum {
        return Err(ErrorCode::InvalidRequest);
    }
    let mut bytes = Zeroizing::new(vec![0; length]);
    stream
        .read_exact(&mut bytes)
        .await
        .map_err(|_| ErrorCode::TransportUnavailable)?;
    Ok(bytes)
}

#[cfg(unix)]
pub async fn write_frame(
    stream: &mut UnixStream,
    bytes: &[u8],
    maximum: usize,
) -> Result<(), ErrorCode> {
    if bytes.is_empty() || bytes.len() > maximum {
        return Err(ErrorCode::Capacity);
    }
    stream
        .write_u32_le(bytes.len() as u32)
        .await
        .map_err(|_| ErrorCode::TransportUncertain)?;
    stream
        .write_all(bytes)
        .await
        .map_err(|_| ErrorCode::TransportUncertain)?;
    stream
        .flush()
        .await
        .map_err(|_| ErrorCode::TransportUncertain)
}

#[cfg(unix)]
fn same_user(stream: &UnixStream) -> bool {
    stream
        .peer_cred()
        .is_ok_and(|peer| peer.uid() == unsafe { libc::geteuid() })
}

#[cfg(unix)]
async fn connection(mut stream: UnixStream, broker: Arc<Broker>) {
    use std::time::Duration;
    if !same_user(&stream) {
        return;
    }
    let request = tokio::time::timeout(Duration::from_secs(5), async {
        let bytes = read_frame(&mut stream, MAX_FRAME_BYTES).await?;
        serde_json::from_slice::<Envelope>(&bytes).map_err(|_| ErrorCode::InvalidRequest)
    })
    .await
    .unwrap_or(Err(ErrorCode::InvalidRequest));
    let reply = match request {
        Ok(request) => broker.execute(request).await,
        Err(error) => Reply::Error(error),
    };
    if let Ok(bytes) = serde_json::to_vec(&reply) {
        let bytes = Zeroizing::new(bytes);
        let _ = tokio::time::timeout(
            Duration::from_secs(5),
            write_frame(&mut stream, &bytes, MAX_REPLY_BYTES),
        )
        .await;
    }
}

#[cfg(unix)]
pub async fn serve(broker: Arc<Broker>) -> Result<(), ErrorCode> {
    use std::{
        fs,
        os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt},
        time::Duration,
    };
    use tokio::{sync::Semaphore, task::JoinSet};
    let socket = broker.socket();
    // The Broker holds the exclusive instance lock before stale-socket cleanup.
    match fs::symlink_metadata(&socket) {
        Ok(metadata)
            if metadata.file_type().is_socket() && metadata.uid() == unsafe { libc::geteuid() } =>
        {
            fs::remove_file(&socket).map_err(|_| ErrorCode::Unavailable)?;
        }
        Ok(_) => return Err(ErrorCode::Conflict),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return Err(ErrorCode::Unavailable),
    }
    let listener = UnixListener::bind(&socket).map_err(|_| ErrorCode::Unavailable)?;
    fs::set_permissions(&socket, fs::Permissions::from_mode(0o600))
        .map_err(|_| ErrorCode::Unavailable)?;
    let bound_inode = fs::symlink_metadata(&socket)
        .map_err(|_| ErrorCode::Unavailable)?
        .ino();
    let bridge_socket = socket.with_file_name("bridge.sock");
    match fs::symlink_metadata(&bridge_socket) {
        Ok(metadata)
            if metadata.file_type().is_socket() && metadata.uid() == unsafe { libc::geteuid() } =>
        {
            fs::remove_file(&bridge_socket).map_err(|_| ErrorCode::Unavailable)?;
        }
        Ok(_) => return Err(ErrorCode::Conflict),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return Err(ErrorCode::Unavailable),
    }
    let bridge_listener = UnixListener::bind(&bridge_socket).map_err(|_| ErrorCode::Unavailable)?;
    fs::set_permissions(&bridge_socket, fs::Permissions::from_mode(0o600))
        .map_err(|_| ErrorCode::Unavailable)?;
    let bridge_inode = fs::symlink_metadata(&bridge_socket)
        .map_err(|_| ErrorCode::Unavailable)?
        .ino();
    let slots = Arc::new(Semaphore::new(16));
    let mut tasks = JoinSet::new();
    let stop = broker.shutdown.clone();
    let mut outcome = Ok(());
    loop {
        tokio::select! {
            _ = stop.cancelled() => break,
            _ = tasks.join_next(), if !tasks.is_empty() => {},
            incoming = listener.accept() => {
                let (stream, _) = match incoming {
                    Ok(incoming) => incoming,
                    Err(_) => { outcome = Err(ErrorCode::Unavailable); stop.cancel(); break; },
                };
                let Ok(permit) = Arc::clone(&slots).try_acquire_owned() else { drop(stream); continue; };
                let broker = Arc::clone(&broker);
                tasks.spawn(async move { let _permit = permit; connection(stream, broker).await; });
            },
            incoming = bridge_listener.accept() => {
                let (stream,_) = match incoming { Ok(incoming)=>incoming, Err(_)=>{outcome=Err(ErrorCode::Unavailable);stop.cancel();break;} };
                let Ok(permit) = Arc::clone(&slots).try_acquire_owned() else {drop(stream);continue;};
                let broker = Arc::clone(&broker);
                tasks.spawn(async move {let _permit=permit;let _=broker.accept_native(stream).await;});
            },
        }
    }
    drop(listener);
    drop(bridge_listener);
    // The shutdown token cancels native prompts. In-progress blocking commits
    // retain an Arc to the broker/instance lease until their actual completion.
    let drain = async { while tasks.join_next().await.is_some() {} };
    if tokio::time::timeout(Duration::from_secs(10), drain)
        .await
        .is_err()
    {
        tasks.abort_all();
        while tasks.join_next().await.is_some() {}
    }
    broker.quiesce().await;
    if fs::symlink_metadata(&socket)
        .is_ok_and(|m| m.file_type().is_socket() && m.ino() == bound_inode)
    {
        fs::remove_file(socket).map_err(|_| ErrorCode::Unavailable)?;
    }
    if fs::symlink_metadata(&bridge_socket)
        .is_ok_and(|m| m.file_type().is_socket() && m.ino() == bridge_inode)
    {
        fs::remove_file(bridge_socket).map_err(|_| ErrorCode::Unavailable)?;
    }
    outcome
}

#[cfg(not(unix))]
pub async fn serve(_: Arc<Broker>) -> Result<(), ErrorCode> {
    Err(ErrorCode::Unavailable)
}

#[cfg(unix)]
pub async fn exchange(socket: &std::path::Path, envelope: &Envelope) -> Result<Reply, ErrorCode> {
    use std::time::Duration;
    let mut stream = tokio::time::timeout(Duration::from_secs(3), UnixStream::connect(socket))
        .await
        .map_err(|_| ErrorCode::TransportUnavailable)?
        .map_err(|_| ErrorCode::TransportUnavailable)?;
    if !same_user(&stream) {
        return Err(ErrorCode::Unauthorized);
    }
    let bytes =
        Zeroizing::new(serde_json::to_vec(envelope).map_err(|_| ErrorCode::InvalidRequest)?);
    let work = async {
        write_frame(&mut stream, &bytes, MAX_FRAME_BYTES).await?;
        let bytes = read_frame(&mut stream, MAX_REPLY_BYTES)
            .await
            .map_err(|_| ErrorCode::TransportUncertain)?;
        serde_json::from_slice(&bytes).map_err(|_| ErrorCode::TransportUncertain)
    };
    // Never retry a mutation: a lost response may follow a completed write.
    tokio::time::timeout(Duration::from_secs(CONSENT_TTL_SECS + 10), work)
        .await
        .unwrap_or(Err(ErrorCode::TransportUncertain))
}
#[cfg(not(unix))]
pub async fn exchange(_: &std::path::Path, _: &Envelope) -> Result<Reply, ErrorCode> {
    Err(ErrorCode::Unavailable)
}
