//! Explicit test-only Chrome owner. Always uses a new profile and loopback site.
//! No production custody or fake-approval switch is exposed by this helper.
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::{
    path::PathBuf,
    process::{Child, Command, Stdio},
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
};
use tokio_tungstenite::{tungstenite::Message, MaybeTlsStream, WebSocketStream};

pub const BROWSER_CANARY: &str = "SYNTHETIC-CHROMIUM-FILL-CANARY";
const HTML: &str = include_str!("../browser-fixtures/index.html");
const JS: &str = include_str!("../browser-fixtures/fixture.js");

pub struct CdpPeer {
    socket: WebSocketStream<MaybeTlsStream<TcpStream>>,
    next: u64,
}
impl CdpPeer {
    pub async fn connect(endpoint: &str) -> Self {
        let (socket, _) = tokio::time::timeout(
            Duration::from_secs(5),
            tokio_tungstenite::connect_async(endpoint),
        )
        .await
        .unwrap()
        .unwrap();
        Self { socket, next: 0 }
    }
    pub async fn command(&mut self, method: &str, params: Value, session: Option<&str>) -> Value {
        self.next += 1;
        let id = self.next;
        let mut request = json!({"id":id,"method":method,"params":params});
        if let Some(session) = session {
            request["sessionId"] = json!(session);
        }
        tokio::time::timeout(Duration::from_secs(10), async {
            self.socket
                .send(Message::Text(request.to_string()))
                .await
                .unwrap();
            while let Some(message) = self.socket.next().await {
                let Message::Text(text) = message.unwrap() else {
                    continue;
                };
                let response: Value = serde_json::from_str(&text).unwrap();
                if response["id"] == id {
                    assert!(
                        response.get("error").is_none(),
                        "test browser command failed"
                    );
                    return response["result"].clone();
                }
            }
            panic!("test browser peer closed");
        })
        .await
        .expect("test browser command timed out")
    }
    pub async fn attach(&mut self, tab: &str) -> String {
        self.command(
            "Target.attachToTarget",
            json!({"targetId":tab,"flatten":true}),
            None,
        )
        .await["sessionId"]
            .as_str()
            .unwrap()
            .into()
    }
    pub async fn evaluate_bool(&mut self, session: &str, expression: &str) -> bool {
        let result = self
            .command(
                "Runtime.evaluate",
                json!({"expression":expression,"returnByValue":true}),
                Some(session),
            )
            .await;
        assert!(
            result.get("exceptionDetails").is_none(),
            "test expression failed"
        );
        result["result"]["value"]
            .as_bool()
            .expect("test expression must return only a boolean")
    }
    pub async fn wait_ready(&mut self, session: &str) {
        tokio::time::timeout(Duration::from_secs(15), async {
            while !self
                .evaluate_bool(session, "document.readyState === 'complete'")
                .await
            {
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
        })
        .await
        .expect("test page did not finish loading");
    }
    pub async fn navigate(&mut self, session: &str, url: &str) {
        let response = self
            .command("Page.navigate", json!({"url":url}), Some(session))
            .await;
        assert!(
            response.get("errorText").is_none(),
            "test navigation failed"
        );
        let expression = format!(
            "location.href === {} && document.readyState === 'complete'",
            serde_json::to_string(url).unwrap()
        );
        tokio::time::timeout(Duration::from_secs(20), async {
            while !self.evaluate_bool(session, &expression).await {
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
        })
        .await
        .expect("test navigation did not settle");
    }
    pub async fn open_page(&mut self, url: &str) -> (String, String) {
        let tab = self
            .command("Target.createTarget", json!({"url":"about:blank"}), None)
            .await["targetId"]
            .as_str()
            .unwrap()
            .to_owned();
        let session = self.attach(&tab).await;
        self.navigate(&session, url).await;
        (tab, session)
    }
}

struct Site(tokio::task::JoinHandle<()>);
impl Drop for Site {
    fn drop(&mut self) {
        self.0.abort();
    }
}
async fn site() -> (String, Site) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
    let task = tokio::spawn(async move {
        let mut tasks = tokio::task::JoinSet::new();
        loop {
            tokio::select! {
                _ = tasks.join_next(), if !tasks.is_empty() => {},
                accepted = listener.accept() => {
                    let Ok((mut socket, _)) = accepted else { break; };
                    if tasks.len() >= 16 { continue; }
                    tasks.spawn(async move {
                        let _ = tokio::time::timeout(Duration::from_secs(5), async {
                            let mut header = Vec::new();
                            while header.len() < 4096 && !header.ends_with(b"\r\n\r\n") {
                                let byte = socket.read_u8().await?;
                                header.push(byte);
                            }
                            let line = std::str::from_utf8(&header).unwrap_or("").lines().next().unwrap_or("");
                            let (status, mime, body) = match line.split_whitespace().collect::<Vec<_>>().as_slice() {
                                ["GET", "/fixture.js", "HTTP/1.1"] => (200, "text/javascript", JS),
                                ["GET", "/" | "/login" | "/controls" | "/frames" | "/frame" | "/next", "HTTP/1.1"] => (200, "text/html", HTML),
                                ["GET", "/favicon.ico", "HTTP/1.1"] => (204, "text/plain", ""),
                                _ => (404, "text/plain", ""),
                            };
                            let response = format!("HTTP/1.1 {status} Fixture\r\nContent-Type: {mime}; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nContent-Security-Policy: default-src 'none'; script-src 'self'; style-src 'unsafe-inline'; frame-src 'self'; connect-src 'none'; form-action 'none'; base-uri 'none'\r\nConnection: close\r\n\r\n{body}",body.len());
                            socket.write_all(response.as_bytes()).await
                        }).await;
                    });
                }
            }
        }
    });
    (origin, Site(task))
}

pub struct DisposableBrowser {
    pub endpoint: String,
    pub origin: String,
    child: Child,
    _profile: tempfile::TempDir,
    _site: Site,
}
impl DisposableBrowser {
    pub async fn start(headless: bool) -> Self {
        Self::start_configured(headless, None).await
    }
    /// Test-only native messaging definitions in a fresh user-data directory.
    /// Never invokes an OS-wide installer or reuses a personal Chrome profile.
    pub async fn with_native_host(headless: bool, manifest: &[u8]) -> Self {
        Self::start_configured(headless, Some(manifest)).await
    }
    async fn start_configured(headless: bool, native_manifest: Option<&[u8]>) -> Self {
        let executable = std::env::var_os("MAGICVAULT_CHROME")
            .map(PathBuf::from)
            .expect("explicit MAGICVAULT_CHROME executable required");
        assert!(executable.is_absolute() && executable.is_file());
        let profile = match std::env::var_os("MAGICVAULT_BROWSER_TMPDIR") {
            Some(parent) => {
                let parent = PathBuf::from(parent);
                assert!(parent.is_absolute() && parent.is_dir());
                tempfile::Builder::new()
                    .prefix("mv-browser-")
                    .tempdir_in(parent)
                    .unwrap()
            }
            None => tempfile::tempdir().unwrap(),
        };
        if let Some(manifest) = native_manifest {
            let value: Value = serde_json::from_slice(manifest).unwrap();
            assert_eq!(value["name"], "ai.magicbeans.magicvault");
            let directory = profile.path().join("NativeMessagingHosts");
            std::fs::create_dir(&directory).unwrap();
            std::fs::write(directory.join("ai.magicbeans.magicvault.json"), manifest).unwrap();
        }
        let (origin, server) = site().await;
        let mut command = Command::new(executable);
        command
            .arg(format!("--user-data-dir={}", profile.path().display()))
            .args([
                "--remote-debugging-address=127.0.0.1",
                "--remote-debugging-port=0",
                "--no-first-run",
                "--no-default-browser-check",
                "--disable-background-networking",
                "--disable-sync",
                "--disable-component-update",
            ]);
        if headless {
            command.arg("--headless=new");
        }
        if native_manifest.is_some() {
            // Only opt-in test browsers expose the unpacked-extension loader.
            command.arg("--enable-unsafe-extension-debugging");
        }
        let child = command
            .arg(if native_manifest.is_some() {
                "about:blank".into()
            } else {
                format!("{origin}/login")
            })
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        // Construct the owner before any await so cancellation/panic also kills
        // and reaps this exact child before removing its disposable profile.
        let mut result = Self {
            endpoint: String::new(),
            origin,
            child,
            _profile: profile,
            _site: server,
        };
        result.endpoint = tokio::time::timeout(Duration::from_secs(20), async {
            loop {
                if let Ok(text) =
                    std::fs::read_to_string(result._profile.path().join("DevToolsActivePort"))
                {
                    let mut lines = text.lines();
                    if let (Some(port), Some(path)) = (lines.next(), lines.next()) {
                        if port.parse::<u16>().is_ok_and(|port| port != 0)
                            && path.starts_with("/devtools/browser/")
                        {
                            break format!("ws://127.0.0.1:{port}{path}");
                        }
                    }
                }
                assert!(
                    result.child.try_wait().unwrap().is_none(),
                    "disposable browser exited during startup"
                );
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        })
        .await
        .expect("disposable browser startup timed out");
        result
    }
    pub async fn peer(&self) -> CdpPeer {
        CdpPeer::connect(&self.endpoint).await
    }
    pub fn running(&mut self) -> bool {
        self.child.try_wait().unwrap().is_none()
    }
}
impl Drop for DisposableBrowser {
    fn drop(&mut self) {
        // Test-only teardown; never enumerate or kill unrelated browser PIDs.
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
