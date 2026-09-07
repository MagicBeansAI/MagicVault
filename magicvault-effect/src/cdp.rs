//! A dedicated, bounded CDP connection; never a proxy or arbitrary JS endpoint.
use crate::{
    canonical_origin, matches_target_filter, origin_from_url, valid_target_filter, BrowserAdapter,
    MaterialField, Outcome, Target, TargetFilter, FILL_FUNCTION,
};
use async_trait::async_trait;
use futures_util::{SinkExt, StreamExt};
use magicvault_protocol::{ErrorCode, FILL_TIMEOUT_SECS, MAX_FIELDS, MAX_TARGETS};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};
use tokio::{net::TcpStream, sync::Mutex};
use tokio_tungstenite::{
    tungstenite::{protocol::WebSocketConfig, Message},
    MaybeTlsStream, WebSocketStream,
};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;
use zeroize::Zeroize;

const MAX_CDP_BYTES: usize = 512 * 1024;
type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;

pub fn validate_endpoint(endpoint: &str) -> Result<(), ErrorCode> {
    let url = url::Url::parse(endpoint).map_err(|_| ErrorCode::InvalidRequest)?;
    let local = match url.host() {
        Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
        Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
        _ => false,
    };
    if endpoint.len() > 512
        || url.scheme() != "ws"
        || !local
        || url.port().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || !url
            .path()
            .strip_prefix("/devtools/browser/")
            .is_some_and(|id| {
                !id.is_empty() && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
            })
    {
        return Err(ErrorCode::InvalidRequest);
    }
    Ok(())
}

struct Connection {
    socket: Socket,
    next_id: u64,
    contexts: HashMap<(String, i64), String>,
}
impl Connection {
    async fn command(
        &mut self,
        session: Option<&str>,
        method: &str,
        mut params: Value,
    ) -> Result<Value, ErrorCode> {
        self.next_id = self.next_id.checked_add(1).ok_or(ErrorCode::Capacity)?;
        let id = self.next_id;
        let mut request = json!({"id": id, "method": method, "params": params.take()});
        if let Some(session) = session {
            request["sessionId"] = json!(session);
        }
        let encoded = serde_json::to_string(&request).map_err(|_| ErrorCode::Unavailable);
        scrub(&mut request);
        let encoded = encoded?;
        if encoded.len() > MAX_CDP_BYTES {
            let mut encoded = encoded;
            encoded.zeroize();
            return Err(ErrorCode::Capacity);
        }
        let work = async {
            self.socket
                .send(Message::Text(encoded))
                .await
                .map_err(|_| ErrorCode::TransportUncertain)?;
            for _ in 0..1024 {
                let message = self
                    .socket
                    .next()
                    .await
                    .ok_or(ErrorCode::TransportUncertain)?
                    .map_err(|_| ErrorCode::TransportUncertain)?;
                let Message::Text(mut text) = message else {
                    if matches!(message, Message::Close(_)) {
                        return Err(ErrorCode::TransportUncertain);
                    }
                    continue;
                };
                let parsed =
                    serde_json::from_str::<Value>(&text).map_err(|_| ErrorCode::TransportUncertain);
                text.zeroize();
                let mut message = parsed?;
                if message.get("id").and_then(Value::as_u64) == Some(id) {
                    if message.get("sessionId").and_then(Value::as_str) != session {
                        scrub(&mut message);
                        return Err(ErrorCode::TransportUncertain);
                    }
                    if message.get("error").is_some() {
                        scrub(&mut message);
                        return Err(ErrorCode::UnsupportedTarget);
                    }
                    let result = message
                        .get_mut("result")
                        .map(Value::take)
                        .ok_or(ErrorCode::TransportUncertain);
                    scrub(&mut message);
                    return result;
                }
                if message.get("method").and_then(Value::as_str)
                    == Some("Runtime.executionContextCreated")
                {
                    let context = &message["params"]["context"];
                    if let (Some(session), Some(id), Some(unique)) = (
                        message["sessionId"].as_str(),
                        context["id"].as_i64(),
                        context["uniqueId"].as_str(),
                    ) {
                        if self.contexts.len() >= 512
                            || id < 0
                            || session.len() > 256
                            || unique.len() > 256
                            || unique.is_empty()
                            || session.chars().any(char::is_control)
                            || unique.chars().any(char::is_control)
                        {
                            scrub(&mut message);
                            return Err(ErrorCode::Capacity);
                        }
                        self.contexts
                            .insert((session.to_owned(), id), unique.to_owned());
                    }
                }
                // Unsolicited browser diagnostics may themselves hold secrets.
                // They are never logged or forwarded to the agent.
                scrub(&mut message);
            }
            Err(ErrorCode::Capacity)
        };
        tokio::time::timeout(Duration::from_secs(5), work)
            .await
            .map_err(|_| ErrorCode::TransportUncertain)?
    }

    async fn attach(&mut self, tab: &str) -> Result<String, ErrorCode> {
        let reply = self
            .command(
                None,
                "Target.attachToTarget",
                json!({"targetId":tab,"flatten":true}),
            )
            .await?;
        bounded_string(&reply["sessionId"])
    }

    async fn detach(&mut self, session: &str) -> Result<(), ErrorCode> {
        self.command(
            None,
            "Target.detachFromTarget",
            json!({"sessionId":session}),
        )
        .await?;
        self.contexts.retain(|(s, _), _| s != session);
        Ok(())
    }

    async fn frame_targets(&mut self, tab: &str, session: &str) -> Result<Vec<Target>, ErrorCode> {
        let mut reply = self
            .command(Some(session), "Page.getFrameTree", json!({}))
            .await?;
        let result = parse_frames(tab, &reply["frameTree"]);
        scrub(&mut reply);
        result
    }
}

pub struct CdpBrowser {
    connection: Mutex<Option<Connection>>,
    stop: CancellationToken,
    alive: AtomicBool,
    world_name: String,
}
impl CdpBrowser {
    pub async fn connect(endpoint: &str) -> Result<Arc<Self>, ErrorCode> {
        validate_endpoint(endpoint)?;
        let config = WebSocketConfig {
            max_message_size: Some(MAX_CDP_BYTES),
            max_frame_size: Some(MAX_CDP_BYTES),
            write_buffer_size: 0,
            max_write_buffer_size: MAX_CDP_BYTES * 2,
            ..WebSocketConfig::default()
        };
        // No redirect following, DNS resolution, proxy, cookies or ambient auth.
        let (socket, _) = tokio::time::timeout(
            Duration::from_secs(5),
            tokio_tungstenite::connect_async_with_config(endpoint, Some(config), true),
        )
        .await
        .map_err(|_| ErrorCode::TransportUnavailable)?
        .map_err(|_| ErrorCode::TransportUnavailable)?;
        Ok(Arc::new(Self {
            connection: Mutex::new(Some(Connection {
                socket,
                next_id: 0,
                contexts: HashMap::new(),
            })),
            stop: CancellationToken::new(),
            alive: AtomicBool::new(true),
            world_name: format!("magicvault-{}", Uuid::new_v4()),
        }))
    }

    async fn perform_fill(
        connection: &mut Connection,
        target: &Target,
        fields: &[MaterialField],
        world_name: &str,
    ) -> Result<Outcome, ErrorCode> {
        let session = connection.attach(&target.tab).await?;
        let work =
            async {
                if !connection
                    .frame_targets(&target.tab, &session)
                    .await?
                    .contains(target)
                {
                    return Err(ErrorCode::StaleTarget);
                }
                connection
                    .command(Some(&session), "Runtime.enable", json!({}))
                    .await?;
                let world = connection.command(Some(&session), "Page.createIsolatedWorld", json!({
                "frameId":target.frame,"worldName":world_name,"grantUniveralAccess":false
            })).await?;
                let numeric = world["executionContextId"]
                    .as_i64()
                    .ok_or(ErrorCode::UnsupportedTarget)?;
                // Never deliver material using a numeric context ID that could be
                // recycled across process navigation. No fallback on older browsers.
                let unique = connection
                    .contexts
                    .get(&(session.clone(), numeric))
                    .cloned()
                    .ok_or(ErrorCode::UnsupportedTarget)?;
                if !connection
                    .frame_targets(&target.tab, &session)
                    .await?
                    .contains(target)
                {
                    return Err(ErrorCode::StaleTarget);
                }
                let mut reply = connection.command(Some(&session), "Runtime.callFunctionOn", json!({
                "functionDeclaration":FILL_FUNCTION,"uniqueContextId":unique,
                "arguments":[{"value":target.origin},{"value":fields}],
                "returnByValue":true,"silent":true,"awaitPromise":false,"userGesture":false
            })).await?;
                let outcome = if reply.get("exceptionDetails").is_some() {
                    Err(ErrorCode::TransportUncertain)
                } else {
                    reply
                        .get_mut("result")
                        .and_then(|value| value.get_mut("value"))
                        .map(Value::take)
                        .ok_or(ErrorCode::TransportUncertain)
                        .and_then(|value| {
                            serde_json::from_value::<Outcome>(value)
                                .map_err(|_| ErrorCode::TransportUncertain)
                        })
                };
                scrub(&mut reply);
                let outcome = outcome?;
                if !outcome.valid(fields.len()) {
                    return Err(ErrorCode::TransportUncertain);
                }
                Ok(outcome)
            }
            .await;
        // Failed cleanup invalidates this connection instead of leaking sessions.
        connection.detach(&session).await?;
        work
    }
}

#[async_trait]
impl BrowserAdapter for CdpBrowser {
    async fn targets(&self, cancel: CancellationToken) -> Result<Vec<Target>, ErrorCode> {
        self.targets_filtered(&TargetFilter::default(), cancel)
            .await
    }

    async fn targets_filtered(
        &self,
        filter: &TargetFilter,
        cancel: CancellationToken,
    ) -> Result<Vec<Target>, ErrorCode> {
        if !valid_target_filter(filter) {
            return Err(ErrorCode::InvalidRequest);
        }
        if !self.connected() {
            return Err(ErrorCode::Unavailable);
        }
        let mut guard = self.connection.try_lock().map_err(|_| ErrorCode::Busy)?;
        let mut connection = guard.take().ok_or(ErrorCode::Unavailable)?;
        // Only a locally computed discovery overflow at a clean protocol
        // boundary permits reuse. Transport/frame failures still disconnect.
        let mut bounded_overflow = false;
        let work = async {
            let mut reply = connection
                .command(
                    None,
                    "Target.getTargets",
                    json!({"filter":[{"type":"page","exclude":false},{"exclude":true}]}),
                )
                .await?;
            let infos = reply["targetInfos"]
                .as_array()
                .ok_or(ErrorCode::TransportUncertain)?;
            let tabs = infos
                .iter()
                .filter(|v| v["type"] == "page")
                .filter(|v| {
                    filter
                        .tab_id
                        .as_ref()
                        .is_none_or(|tab| v["targetId"].as_str() == Some(tab.as_str()))
                })
                .filter(|v| {
                    v["url"]
                        .as_str()
                        .and_then(|url| origin_from_url(url).ok())
                        .is_some_and(|origin| {
                            filter
                                .top_origin
                                .as_ref()
                                .is_none_or(|expected| expected == &origin)
                        })
                })
                .map(|v| bounded_string(&v["targetId"]))
                .take(MAX_TARGETS + 1)
                .collect::<Result<Vec<_>, _>>()?;
            scrub(&mut reply);
            if tabs.len() > MAX_TARGETS {
                bounded_overflow = true;
                return Err(ErrorCode::Capacity);
            }
            let mut targets = Vec::new();
            for tab in tabs {
                let session = connection.attach(&tab).await?;
                let discovered = connection.frame_targets(&tab, &session).await;
                connection.detach(&session).await?;
                match discovered {
                    // A page may navigate after Target.getTargets. Reapply
                    // exact narrowing to the actual discovered document.
                    Ok(found) => targets.extend(
                        found
                            .into_iter()
                            .filter(|t| matches_target_filter(t, filter)),
                    ),
                    Err(ErrorCode::UnsupportedTarget) => {}
                    Err(error) => return Err(error),
                }
                if targets.len() > MAX_TARGETS {
                    bounded_overflow = true;
                    return Err(ErrorCode::Capacity);
                }
            }
            Ok(targets)
        };
        let result = tokio::select! {
            _ = self.stop.cancelled() => Err(ErrorCode::Unavailable),
            _ = cancel.cancelled() => Err(ErrorCode::Cancelled),
            result = tokio::time::timeout(Duration::from_secs(FILL_TIMEOUT_SECS), work) => result.unwrap_or(Err(ErrorCode::TransportUncertain)),
        };
        if result.is_ok() || (bounded_overflow && matches!(result, Err(ErrorCode::Capacity))) {
            *guard = Some(connection);
        } else {
            self.alive.store(false, Ordering::Release);
        }
        result
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
        if !self.connected() || cancel.is_cancelled() {
            return Outcome::failed(count, ErrorCode::Cancelled);
        }
        let Ok(mut guard) = self.connection.try_lock() else {
            return Outcome::failed(count, ErrorCode::Busy);
        };
        let Some(mut connection) = guard.take() else {
            return Outcome::failed(count, ErrorCode::Unavailable);
        };
        let result = tokio::select! {
            _ = self.stop.cancelled() => Err(ErrorCode::TransportUncertain),
            _ = cancel.cancelled() => Err(ErrorCode::TransportUncertain),
            result = tokio::time::timeout(Duration::from_secs(FILL_TIMEOUT_SECS), Self::perform_fill(&mut connection,target,&fields,&self.world_name)) => result.unwrap_or(Err(ErrorCode::TransportUncertain)),
        };
        match result {
            Ok(outcome) => {
                *guard = Some(connection);
                outcome
            }
            Err(error) => {
                self.alive.store(false, Ordering::Release);
                // A protocol failure can follow an already-applied write. Only
                // pre-delivery stale checks are known not to have filled.
                if error == ErrorCode::StaleTarget {
                    Outcome::failed(count, error)
                } else {
                    Outcome::uncertain(count)
                }
            }
        }
    }

    fn disconnect(&self) {
        self.alive.store(false, Ordering::Release);
        self.stop.cancel();
        if let Ok(mut guard) = self.connection.try_lock() {
            guard.take();
        }
    }
    fn connected(&self) -> bool {
        self.alive.load(Ordering::Acquire) && !self.stop.is_cancelled()
    }
}

fn bounded_string(value: &Value) -> Result<String, ErrorCode> {
    value
        .as_str()
        .filter(|s| !s.is_empty() && s.len() <= 256 && !s.chars().any(char::is_control))
        .map(str::to_owned)
        .ok_or(ErrorCode::UnsupportedTarget)
}

fn parse_frames(tab: &str, tree: &Value) -> Result<Vec<Target>, ErrorCode> {
    let root = &tree["frame"];
    let top_origin = canonical_origin(
        root["securityOrigin"]
            .as_str()
            .ok_or(ErrorCode::UnsupportedTarget)?,
    )
    .map_err(|_| ErrorCode::UnsupportedTarget)?;
    if origin_from_url(root["url"].as_str().ok_or(ErrorCode::UnsupportedTarget)?)? != top_origin {
        return Err(ErrorCode::UnsupportedTarget);
    }
    let top_document = bounded_string(&root["loaderId"])?;
    let main_id = bounded_string(&root["id"])?;
    let mut pending = vec![tree];
    let mut targets = Vec::new();
    let mut visited = 0;
    while let Some(tree) = pending.pop() {
        visited += 1;
        if visited > MAX_TARGETS {
            return Err(ErrorCode::Capacity);
        }
        let frame = &tree["frame"];
        if let (Some(origin), Some(url), Ok(id), Ok(document)) = (
            frame["securityOrigin"].as_str(),
            frame["url"].as_str(),
            bounded_string(&frame["id"]),
            bounded_string(&frame["loaderId"]),
        ) {
            if let (Ok(origin), Ok(url_origin)) = (canonical_origin(origin), origin_from_url(url)) {
                if origin == url_origin {
                    targets.push(Target {
                        tab: tab.to_owned(),
                        is_main_frame: id == main_id,
                        frame: id,
                        document,
                        top_document: top_document.clone(),
                        origin,
                        top_origin: top_origin.clone(),
                    });
                }
            }
        }
        if let Some(children) = tree["childFrames"].as_array() {
            if pending.len() + children.len() > MAX_TARGETS {
                return Err(ErrorCode::Capacity);
            }
            pending.extend(children);
        }
    }
    Ok(targets)
}

/// Best-effort erasure of temporary JSON strings; transport/browser heaps are
/// trusted recipients and are not claimed to be cryptographically erased.
pub(crate) fn scrub(value: &mut Value) {
    match value {
        Value::String(text) => text.zeroize(),
        Value::Array(values) => values.iter_mut().for_each(scrub),
        Value::Object(values) => values.values_mut().for_each(scrub),
        _ => {}
    }
}
