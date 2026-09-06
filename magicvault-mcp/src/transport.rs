//! Official SDK message codec with bounded stdio and payload-free failures.
use std::{future::Future, io::{self, Write}, marker::PhantomData, sync::Arc, time::Duration};
use bytes::BytesMut;
use futures_util::{SinkExt, StreamExt};
use rmcp::{service::{RoleServer, RxJsonRpcMessage, TxJsonRpcMessage}, transport::{Transport, async_rw::JsonRpcMessageCodec}};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_util::codec::{Encoder, FramedRead, FramedWrite};

pub struct BoundedTransport<R, W> {
    reader: FramedRead<R, JsonRpcMessageCodec<RxJsonRpcMessage<RoleServer>>>,
    writer: Arc<tokio::sync::Mutex<Option<FramedWrite<W, BoundedEncoder<TxJsonRpcMessage<RoleServer>>>>>>,
}
impl<R: AsyncRead, W: AsyncWrite> BoundedTransport<R, W> {
    pub fn new(reader: R, writer: W) -> Self {
        Self {
            reader: FramedRead::new(reader, JsonRpcMessageCodec::new_with_max_length(32 * 1024)),
            writer: Arc::new(tokio::sync::Mutex::new(Some(FramedWrite::new(writer, BoundedEncoder(PhantomData))))),
        }
    }
}
struct BoundedEncoder<T>(PhantomData<fn() -> T>);
struct LimitedWriter<'a> { bytes: &'a mut BytesMut, start: usize }
impl Write for LimitedWriter<'_> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if (self.bytes.len() - self.start).saturating_add(bytes.len()) > 1024 * 1024 {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "MCP reply limit"));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> { Ok(()) }
}
impl<T: serde::Serialize> Encoder<T> for BoundedEncoder<T> {
    type Error = io::Error;
    fn encode(&mut self, item: T, destination: &mut BytesMut) -> io::Result<()> {
        let start = destination.len();
        let mut writer = LimitedWriter { bytes: destination, start };
        if serde_json::to_writer(&mut writer, &item).is_err() {
            writer.bytes.truncate(start);
            return Err(io::Error::new(io::ErrorKind::InvalidData, "MCP reply unavailable"));
        }
        writer.bytes.extend_from_slice(b"\n");
        Ok(())
    }
}
impl<R, W> Transport<RoleServer> for BoundedTransport<R, W>
where R: AsyncRead + Send + Unpin, W: AsyncWrite + Send + Unpin + 'static {
    type Error = io::Error;
    fn send(&mut self, item: TxJsonRpcMessage<RoleServer>) -> impl Future<Output = io::Result<()>> + Send + 'static {
        let writer = Arc::clone(&self.writer);
        async move {
            let mut writer = writer.lock().await;
            let output = writer.as_mut().ok_or_else(|| io::Error::new(io::ErrorKind::NotConnected, "MCP closed"))?;
            let result = tokio::time::timeout(Duration::from_secs(5), output.send(item)).await
                .unwrap_or_else(|_| Err(io::Error::new(io::ErrorKind::TimedOut, "MCP write timeout")));
            // A partial frame may have escaped. Drop the writer, never retry it.
            if result.is_err() { writer.take(); }
            result
        }
    }
    async fn receive(&mut self) -> Option<RxJsonRpcMessage<RoleServer>> {
        match self.reader.next().await { Some(Ok(message)) => Some(message), _ => None }
    }
    async fn close(&mut self) -> io::Result<()> {
        if let Some(mut writer) = self.writer.lock().await.take() {
            tokio::time::timeout(Duration::from_secs(5), writer.close()).await
                .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "MCP close timeout"))??;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounded_encoder_rolls_back_an_oversized_frame() {
        let mut encoder = BoundedEncoder::<serde_json::Value>(PhantomData);
        let mut bytes = BytesMut::from(&b"previous\n"[..]);
        assert!(encoder.encode(serde_json::json!({"text":"x".repeat(1024 * 1024)}),&mut bytes).is_err());
        assert_eq!(&bytes[..],b"previous\n");
        encoder.encode(serde_json::json!({"ok":true}),&mut bytes).unwrap();
        assert_eq!(&bytes[..],b"previous\n{\"ok\":true}\n");
    }
}
