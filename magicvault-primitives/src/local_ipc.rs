//! Same-user local streams: Unix sockets or ACL-restricted Windows named pipes.
#[cfg(unix)]
mod platform {
    use std::{
        io,
        os::fd::{AsFd, AsRawFd, OwnedFd},
        path::Path,
    };
    pub type Stream = tokio::net::UnixStream;
    pub async fn connect(path: &Path) -> io::Result<Stream> {
        Stream::connect(path).await
    }
    pub fn same_user(stream: &Stream) -> bool {
        stream
            .peer_cred()
            .is_ok_and(|p| p.uid() == unsafe { libc::geteuid() })
    }
    pub struct Monitor(OwnedFd);
    impl Monitor {
        pub fn new(stream: &Stream) -> io::Result<Self> {
            Ok(Self(stream.as_fd().try_clone_to_owned()?))
        }
        pub fn disconnect(&self) {
            unsafe {
                libc::shutdown(self.0.as_raw_fd(), libc::SHUT_RDWR);
            }
        }
        pub fn connected(&self) -> bool {
            let mut byte = 0u8;
            let result = unsafe {
                libc::recv(
                    self.0.as_raw_fd(),
                    (&mut byte as *mut u8).cast(),
                    1,
                    libc::MSG_PEEK | libc::MSG_DONTWAIT,
                )
            };
            result > 0
                || (result < 0
                    && matches!(
                        io::Error::last_os_error().kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                    ))
        }
    }
}
#[cfg(windows)]
mod platform {
    use std::{
        io,
        os::windows::io::{AsRawHandle, OwnedHandle},
        path::Path,
        pin::Pin,
        task::{Context, Poll},
    };
    use tokio::{
        io::{AsyncRead, AsyncWrite, ReadBuf},
        net::windows::named_pipe::{
            ClientOptions, NamedPipeClient, NamedPipeServer, ServerOptions,
        },
    };
    use windows_sys::Win32::{Foundation::HANDLE, System::Pipes::*};
    pub enum Stream {
        Client(NamedPipeClient),
        Server(NamedPipeServer),
    }
    impl AsRawHandle for Stream {
        fn as_raw_handle(&self) -> HANDLE {
            match self {
                Self::Client(s) => s.as_raw_handle(),
                Self::Server(s) => s.as_raw_handle(),
            }
        }
    }
    impl AsyncRead for Stream {
        fn poll_read(
            self: Pin<&mut Self>,
            cx: &mut Context<'_>,
            buf: &mut ReadBuf<'_>,
        ) -> Poll<io::Result<()>> {
            match self.get_mut() {
                Self::Client(s) => Pin::new(s).poll_read(cx, buf),
                Self::Server(s) => Pin::new(s).poll_read(cx, buf),
            }
        }
    }
    impl AsyncWrite for Stream {
        fn poll_write(
            self: Pin<&mut Self>,
            cx: &mut Context<'_>,
            buf: &[u8],
        ) -> Poll<io::Result<usize>> {
            match self.get_mut() {
                Self::Client(s) => Pin::new(s).poll_write(cx, buf),
                Self::Server(s) => Pin::new(s).poll_write(cx, buf),
            }
        }
        fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
            match self.get_mut() {
                Self::Client(s) => Pin::new(s).poll_flush(cx),
                Self::Server(s) => Pin::new(s).poll_flush(cx),
            }
        }
        fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
            match self.get_mut() {
                Self::Client(s) => Pin::new(s).poll_shutdown(cx),
                Self::Server(s) => Pin::new(s).poll_shutdown(cx),
            }
        }
    }
    fn name(path: &Path) -> io::Result<String> {
        use std::os::windows::ffi::OsStrExt;
        if !path.is_absolute() {
            return Err(io::ErrorKind::InvalidInput.into());
        }
        // Identity includes canonical parent and SID; rpc/bridge remain distinct.
        let parent = path
            .parent()
            .ok_or(io::ErrorKind::InvalidInput)?
            .canonicalize()?;
        let leaf = path.file_name().ok_or(io::ErrorKind::InvalidInput)?;
        let mut hash = blake3::Hasher::new();
        for unit in parent.join(leaf).as_os_str().encode_wide() {
            hash.update(&unit.to_le_bytes());
        }
        hash.update(&crate::windows::user_sid(unsafe {
            windows_sys::Win32::System::Threading::GetCurrentProcess()
        })?);
        Ok(format!(r"\\.\pipe\MagicVault-{}", hash.finalize().to_hex()))
    }
    pub async fn connect(path: &Path) -> io::Result<Stream> {
        let name = name(path)?;
        // Only connection admission retries PIPE_BUSY; no request bytes have
        // been sent. The caller owns the timeout. Never retry a dispatched use.
        loop {
            match ClientOptions::new().open(&name) {
                Ok(stream) => return Ok(Stream::Client(stream)),
                Err(e) if e.raw_os_error() == Some(231) => {
                    tokio::time::sleep(std::time::Duration::from_millis(20)).await
                }
                Err(e) => return Err(e),
            }
        }
    }
    pub fn same_user(stream: &Stream) -> bool {
        let mut pid = 0;
        let result = unsafe {
            match stream {
                Stream::Client(s) => GetNamedPipeServerProcessId(s.as_raw_handle(), &mut pid),
                Stream::Server(s) => GetNamedPipeClientProcessId(s.as_raw_handle(), &mut pid),
            }
        };
        result != 0 && crate::windows::same_process_user(pid)
    }
    pub struct Listener {
        name: String,
        next: NamedPipeServer,
    }
    impl Listener {
        fn create(name: &str, first: bool) -> io::Result<NamedPipeServer> {
            let security = crate::windows::PrivateSecurity::new()?;
            let mut attributes = security.attributes();
            unsafe {
                ServerOptions::new()
                    .first_pipe_instance(first)
                    .reject_remote_clients(true)
                    .create_with_security_attributes_raw(
                        name,
                        (&mut attributes as *mut windows_sys::Win32::Security::SECURITY_ATTRIBUTES)
                            .cast(),
                    )
            }
        }
        pub fn bind(path: &Path) -> io::Result<Self> {
            let name = name(path)?;
            Ok(Self {
                next: Self::create(&name, true)?,
                name,
            })
        }
        pub async fn accept(&mut self) -> io::Result<Stream> {
            self.next.connect().await?;
            let next = Self::create(&self.name, false)?;
            Ok(Stream::Server(std::mem::replace(&mut self.next, next)))
        }
    }
    pub struct Monitor(OwnedHandle);
    impl Monitor {
        pub fn new(stream: &Stream) -> io::Result<Self> {
            use std::os::windows::io::BorrowedHandle;
            let handle = unsafe { BorrowedHandle::borrow_raw(stream.as_raw_handle()) };
            Ok(Self(handle.try_clone_to_owned()?))
        }
        pub fn disconnect(&self) {
            unsafe {
                DisconnectNamedPipe(self.0.as_raw_handle());
            }
        }
        pub fn connected(&self) -> bool {
            unsafe {
                PeekNamedPipe(
                    self.0.as_raw_handle(),
                    std::ptr::null_mut(),
                    0,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                ) != 0
            }
        }
    }
}
pub use platform::*;

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[tokio::test]
    async fn named_pipe_is_exclusive_same_user_and_disconnects_without_consuming_data() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().canonicalize().unwrap().join("rpc.sock");
        let mut listener = Listener::bind(&path).unwrap();
        assert!(Listener::bind(&path).is_err());
        let (client, server) = tokio::time::timeout(std::time::Duration::from_secs(5), async {
            tokio::join!(connect(&path), listener.accept())
        })
        .await
        .unwrap();
        let (mut client, mut server) = (client.unwrap(), server.unwrap());
        assert!(same_user(&client) && same_user(&server));
        let monitor = Monitor::new(&server).unwrap();
        assert!(monitor.connected());
        client.write_all(b"synthetic").await.unwrap();
        assert!(monitor.connected());
        let mut bytes = [0; 9];
        server.read_exact(&mut bytes).await.unwrap();
        assert_eq!(&bytes, b"synthetic");
        monitor.disconnect();
        let result =
            tokio::time::timeout(std::time::Duration::from_secs(5), client.read(&mut bytes))
                .await
                .unwrap();
        assert!(matches!(result, Ok(0) | Err(_)));
        assert!(!monitor.connected());
    }
}
