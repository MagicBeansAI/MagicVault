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

// Closed labels only: unknown methods, arguments, expressions and replies may
// contain fixture material. Never include them in a browser failure diagnostic.
fn command_label(method: &str) -> &'static str {
    match method {
        "Extensions.loadUnpacked" => "Extensions.loadUnpacked",
        "Target.attachToTarget" => "Target.attachToTarget",
        "Target.createTarget" => "Target.createTarget",
        "Runtime.evaluate" => "Runtime.evaluate",
        "Page.navigate" => "Page.navigate",
        "SystemInfo.getProcessInfo" => "SystemInfo.getProcessInfo",
        _ => "Other",
    }
}

#[derive(Debug, PartialEq)]
pub enum RendererProbe {
    Responsive,
    NotReady,
    Error,
    Closed,
    Invalid,
    TimedOut,
}

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
        let label = command_label(method);
        let awaits_promise = params.get("awaitPromise") == Some(&Value::Bool(true));
        self.next += 1;
        let id = self.next;
        let mut request = json!({"id":id,"method":method,"params":params});
        if let Some(session) = session {
            request["sessionId"] = json!(session);
        }
        let result = tokio::time::timeout(Duration::from_secs(10), async {
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
                        "test browser command failed ({label})"
                    );
                    return response["result"].clone();
                }
            }
            panic!("test browser peer closed ({label})");
        })
        .await;
        match result {
            Ok(result) => result,
            Err(_) => {
                // The original command has already failed. One fixed read-only
                // probe records responsiveness after failure, not throughout
                // the failed command; never replay it or resume effects.
                if method == "Runtime.evaluate" {
                    if let Some(session) = session {
                        let probe = self.renderer_probe(session).await;
                        eprintln!("test renderer timeout observation: awaits_promise={awaits_promise}; probe={probe:?}");
                    }
                }
                panic!("test browser command timed out ({label})")
            }
        }
    }
    /// Failure-only fixed read, never a retry of the command that failed.
    pub async fn renderer_probe(&mut self, session: &str) -> RendererProbe {
        self.boolean_probe(session, "true", false).await
    }
    /// Fixed value-free status message to our disposable extension options page.
    /// This does not connect, rediscover targets, change grants or deliver values.
    pub async fn extension_status_probe(&mut self, session: &str) -> RendererProbe {
        self.boolean_probe(
            session,
            "chrome.runtime.sendMessage({action:'status'}).then(s => s.connected === true)",
            true,
        )
        .await
    }
    async fn boolean_probe(
        &mut self,
        session: &str,
        expression: &str,
        await_promise: bool,
    ) -> RendererProbe {
        self.next += 1;
        let id = self.next;
        tokio::time::timeout(Duration::from_secs(2), async {
            let mut request = json!({"id":id,"method":"Runtime.evaluate","sessionId":session,
                "params":{"expression":expression,"returnByValue":true,"silent":true}});
            if await_promise {
                request["params"]["awaitPromise"] = json!(true);
            }
            if self
                .socket
                .send(Message::Text(request.to_string()))
                .await
                .is_err()
            {
                return RendererProbe::Closed;
            }
            // Bounded diagnostic only; no event/response bodies are retained or
            // printed. Late replies to the failed request are never its success.
            for _ in 0..128 {
                let Some(Ok(message)) = self.socket.next().await else {
                    return RendererProbe::Closed;
                };
                if matches!(message, Message::Close(_)) {
                    return RendererProbe::Closed;
                }
                let Message::Text(text) = message else {
                    continue;
                };
                let Ok(response) = serde_json::from_str::<Value>(&text) else {
                    return RendererProbe::Invalid;
                };
                if response["id"] != id {
                    continue;
                }
                if response["sessionId"].as_str() != Some(session) {
                    return RendererProbe::Invalid;
                }
                if response.get("error").is_some()
                    || response["result"].get("exceptionDetails").is_some()
                {
                    return RendererProbe::Error;
                }
                return match response["result"]["result"]["value"].as_bool() {
                    Some(true) => RendererProbe::Responsive,
                    Some(false) => RendererProbe::NotReady,
                    None => RendererProbe::Invalid,
                };
            }
            RendererProbe::Invalid
        })
        .await
        .unwrap_or(RendererProbe::TimedOut)
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
    /// Fixture ownership anchor for read-only resource sampling, not PID discovery.
    pub fn owned_pid(&self) -> u32 {
        self.child.id()
    }
}
impl Drop for DisposableBrowser {
    fn drop(&mut self) {
        // Test-only teardown; never enumerate or kill unrelated browser PIDs.
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::FutureExt;

    #[test]
    fn browser_command_diagnostic_labels_are_closed() {
        for method in [
            "Extensions.loadUnpacked",
            "Target.attachToTarget",
            "Target.createTarget",
            "Runtime.evaluate",
            "Page.navigate",
            "SystemInfo.getProcessInfo",
        ] {
            assert_eq!(command_label(method), method);
        }
        for unknown in [
            BROWSER_CANARY,
            "Runtime.evaluate?private",
            "",
            "unknown\nprivate",
        ] {
            assert_eq!(command_label(unknown), "Other");
        }
    }

    #[tokio::test]
    async fn browser_command_errors_withhold_parameters_and_response_payloads() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (socket, _) = listener.accept().await.unwrap();
            let mut socket = tokio_tungstenite::accept_async(socket).await.unwrap();
            let request = socket.next().await.unwrap().unwrap();
            let request: Value = serde_json::from_str(request.to_text().unwrap()).unwrap();
            socket
                .send(Message::Text(
                    json!({
                        "id":request["id"], "error":{"message":BROWSER_CANARY}
                    })
                    .to_string(),
                ))
                .await
                .unwrap();
        });
        let mut peer = CdpPeer::connect(&format!("ws://{address}")).await;
        let failure = std::panic::AssertUnwindSafe(peer.command(
            "Runtime.evaluate",
            json!({"expression":BROWSER_CANARY}),
            Some(BROWSER_CANARY),
        ))
        .catch_unwind()
        .await
        .expect_err("error response must fail the fixture");
        let message = failure.downcast_ref::<String>().unwrap();
        assert_eq!(message, "test browser command failed (Runtime.evaluate)");
        assert!(!message.contains(BROWSER_CANARY));
        server.await.unwrap();
    }

    #[tokio::test]
    async fn browser_command_timeout_keeps_the_original_bound_and_closed_label() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (socket, _) = listener.accept().await.unwrap();
            let mut socket = tokio_tungstenite::accept_async(socket).await.unwrap();
            assert!(socket.next().await.unwrap().unwrap().is_text());
            // Keep the owned connection open without a response. The command
            // must fail at its unchanged normal fixture ten-second bound.
            std::future::pending::<()>().await;
        });
        let mut peer = CdpPeer::connect(&format!("ws://{address}")).await;
        let started = tokio::time::Instant::now();
        let failure = tokio::time::timeout(
            Duration::from_secs(15),
            std::panic::AssertUnwindSafe(peer.command(
                BROWSER_CANARY,
                json!({"private":BROWSER_CANARY}),
                Some(BROWSER_CANARY),
            ))
            .catch_unwind(),
        )
        .await;
        server.abort();
        assert!(server.await.unwrap_err().is_cancelled());
        let failure = failure
            .expect("fixture must retain its bounded timeout")
            .expect_err("missing response must fail the fixture");
        assert!(started.elapsed() >= Duration::from_secs(10));
        assert_eq!(
            failure.downcast_ref::<String>().unwrap(),
            "test browser command timed out (Other)"
        );
    }

    #[tokio::test]
    async fn renderer_probe_is_read_only_correlated_and_payload_free() {
        for (body, session, expected) in [
            (
                json!({"result":{"result":{"value":true}}}),
                "owned",
                RendererProbe::Responsive,
            ),
            (
                json!({"error":{"message":BROWSER_CANARY}}),
                "owned",
                RendererProbe::Error,
            ),
            (
                json!({"result":{"result":{"value":true}}}),
                "wrong",
                RendererProbe::Invalid,
            ),
            (
                json!({"result":{"result":{"value":BROWSER_CANARY}}}),
                "owned",
                RendererProbe::Invalid,
            ),
        ] {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = listener.local_addr().unwrap();
            let server = tokio::spawn(async move {
                let (socket, _) = listener.accept().await.unwrap();
                let mut socket = tokio_tungstenite::accept_async(socket).await.unwrap();
                let request = socket.next().await.unwrap().unwrap();
                let request: Value = serde_json::from_str(request.to_text().unwrap()).unwrap();
                assert_eq!(request["method"], "Runtime.evaluate");
                assert_eq!(
                    request["params"],
                    json!({"expression":"true","returnByValue":true,"silent":true})
                );
                socket
                    .send(Message::Text(
                        json!({"id":0,"result":BROWSER_CANARY}).to_string(),
                    ))
                    .await
                    .unwrap();
                let mut reply = body;
                reply["id"] = request["id"].clone();
                reply["sessionId"] = json!(session);
                socket.send(Message::Text(reply.to_string())).await.unwrap();
            });
            let mut peer = CdpPeer::connect(&format!("ws://{address}")).await;
            assert_eq!(peer.renderer_probe("owned").await, expected);
            server.await.unwrap();
        }
    }

    #[tokio::test]
    async fn extension_probe_only_requests_boolean_connection_status() {
        for connected in [true, false] {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = listener.local_addr().unwrap();
            let server = tokio::spawn(async move {
                let (socket, _) = listener.accept().await.unwrap();
                let mut socket = tokio_tungstenite::accept_async(socket).await.unwrap();
                let request = socket.next().await.unwrap().unwrap();
                let request: Value = serde_json::from_str(request.to_text().unwrap()).unwrap();
                assert_eq!(request["method"], "Runtime.evaluate");
                assert_eq!(
                    request["params"],
                    json!({
                        "expression":"chrome.runtime.sendMessage({action:'status'}).then(s => s.connected === true)",
                        "awaitPromise":true,"returnByValue":true,"silent":true
                    })
                );
                socket
                    .send(Message::Text(
                        json!({"id":request["id"],"sessionId":request["sessionId"],
                    "result":{"result":{"value":connected}}})
                        .to_string(),
                    ))
                    .await
                    .unwrap();
            });
            let mut peer = CdpPeer::connect(&format!("ws://{address}")).await;
            assert_eq!(
                peer.extension_status_probe("owned").await,
                if connected {
                    RendererProbe::Responsive
                } else {
                    RendererProbe::NotReady
                }
            );
            server.await.unwrap();
        }
    }

    #[tokio::test]
    async fn silent_probe_obeys_its_independent_deadline() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (socket, _) = listener.accept().await.unwrap();
            let mut socket = tokio_tungstenite::accept_async(socket).await.unwrap();
            let _ = socket.next().await.unwrap().unwrap();
            std::future::pending::<()>().await;
        });
        let mut peer = CdpPeer::connect(&format!("ws://{address}")).await;
        let result =
            tokio::time::timeout(Duration::from_secs(5), peer.renderer_probe("owned")).await;
        server.abort();
        assert!(server.await.unwrap_err().is_cancelled());
        assert_eq!(result.unwrap(), RendererProbe::TimedOut);
    }

    #[tokio::test]
    async fn unrelated_probe_frames_cannot_extend_its_budget() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (socket, _) = listener.accept().await.unwrap();
            let mut socket = tokio_tungstenite::accept_async(socket).await.unwrap();
            let _ = socket.next().await.unwrap().unwrap();
            for _ in 0..128 {
                socket
                    .send(Message::Text(
                        json!({"id":0,"result":BROWSER_CANARY}).to_string(),
                    ))
                    .await
                    .unwrap();
            }
            std::future::pending::<()>().await;
        });
        let mut peer = CdpPeer::connect(&format!("ws://{address}")).await;
        let result =
            tokio::time::timeout(Duration::from_secs(5), peer.renderer_probe("owned")).await;
        server.abort();
        assert!(server.await.unwrap_err().is_cancelled());
        assert_eq!(result.unwrap(), RendererProbe::Invalid);
    }

    #[tokio::test]
    async fn responsive_probe_and_late_reply_never_rescue_a_timed_out_command() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (socket, _) = listener.accept().await.unwrap();
            let mut socket = tokio_tungstenite::accept_async(socket).await.unwrap();
            let first = socket.next().await.unwrap().unwrap();
            let first: Value = serde_json::from_str(first.to_text().unwrap()).unwrap();
            let probe = socket.next().await.unwrap().unwrap();
            let probe: Value = serde_json::from_str(probe.to_text().unwrap()).unwrap();
            assert_eq!(probe["params"]["expression"], "true");
            assert_ne!(probe["id"], first["id"]);
            for request in [first, probe] {
                socket
                    .send(Message::Text(
                        json!({"id":request["id"],"sessionId":request["sessionId"],
                    "result":{"result":{"value":true}}})
                        .to_string(),
                    ))
                    .await
                    .unwrap();
            }
        });
        let mut peer = CdpPeer::connect(&format!("ws://{address}")).await;
        let failure = std::panic::AssertUnwindSafe(peer.command(
            "Runtime.evaluate",
            json!({"expression":BROWSER_CANARY,"awaitPromise":true}),
            Some("owned"),
        ))
        .catch_unwind()
        .await
        .expect_err("diagnosis must preserve timeout failure");
        assert_eq!(
            failure.downcast_ref::<String>().unwrap(),
            "test browser command timed out (Runtime.evaluate)"
        );
        server.await.unwrap();
    }
}
