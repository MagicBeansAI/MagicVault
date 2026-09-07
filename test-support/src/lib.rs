//! Test-only synthetic peers and opt-in disposable real browsers. No production
//! dependency may use this crate. Real browsers require an explicit executable.
pub mod browser;
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use tokio_tungstenite::tungstenite::Message;

pub const CANARY: &str = "SYNTHETIC-PHASE3-NOT-A-REAL-CREDENTIAL";

#[derive(Default)]
pub struct FixtureState {
    pub delivered: Vec<Vec<String>>,
    pub navigated: bool,
    pub lose_fill_reply: bool,
    pub malformed_fill_reply: bool,
    pub partial_fill: bool,
    pub omit_unique_context: bool,
    pub calls: Vec<String>,
}
pub struct CdpFixture {
    pub endpoint: String,
    pub state: Arc<Mutex<FixtureState>>,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for CdpFixture {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl CdpFixture {
    pub async fn start() -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let state = Arc::new(Mutex::new(FixtureState::default()));
        let capture = Arc::clone(&state);
        let task = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
            while let Some(Ok(Message::Text(text))) = socket.next().await {
                let message: Value = serde_json::from_str(&text).unwrap();
                let method = message["method"].as_str().unwrap();
                let (result, event, close) = {
                    let mut state = capture.lock().unwrap();
                    state.calls.push(method.into());
                    let mut event = None;
                    let mut close = false;
                    let result = match method {
                        "Target.getTargets" => {
                            json!({"targetInfos":[{"targetId":"tab-fixture","type":"page","url":"https://example.com/login?token=SYNTHETIC-URL-CANARY"}]})
                        }
                        "Target.attachToTarget" => json!({"sessionId":"session-fixture"}),
                        "Target.detachFromTarget" => json!({}),
                        "Page.getFrameTree" => {
                            json!({"frameTree":{"frame":{"id":"frame-fixture","loaderId":if state.navigated{"doc-new"}else{"doc-fixture"},"url":"https://example.com/login?token=SYNTHETIC-URL-CANARY","securityOrigin":"https://example.com"}}})
                        }
                        "Runtime.enable" => json!({}),
                        "Page.createIsolatedWorld" => {
                            if !state.omit_unique_context {
                                event = Some(
                                    json!({"method":"Runtime.executionContextCreated","sessionId":"session-fixture","params":{"context":{"id":42,"uniqueId":"unique-fixture","name":"magicvault-fixture"}}}),
                                );
                            }
                            json!({"executionContextId":42})
                        }
                        "Runtime.callFunctionOn" => {
                            assert_eq!(message["params"]["uniqueContextId"], "unique-fixture");
                            assert!(message["params"].get("executionContextId").is_none());
                            let fields = message["params"]["arguments"][1]["value"]
                                .as_array()
                                .unwrap();
                            state.delivered.push(
                                fields
                                    .iter()
                                    .map(|f| f["value"].as_str().unwrap().to_owned())
                                    .collect(),
                            );
                            close = state.lose_fill_reply;
                            if state.malformed_fill_reply {
                                json!({"result":"SYNTHETIC-PROVIDER-ERROR-CANARY"})
                            } else if state.partial_fill {
                                json!({"result":{"value":{"fields":fields.iter().enumerate().map(|(i,_)|if i==0{"filled"}else{"not_filled"}).collect::<Vec<_>>(),"error":"stale_target"}}})
                            } else {
                                json!({"result":{"value":{"fields":vec!["filled";fields.len()],"error":null}}})
                            }
                        }
                        _ => panic!("unexpected fixture command"),
                    };
                    (result, event, close)
                };
                if let Some(event) = event {
                    if socket.send(Message::Text(event.to_string())).await.is_err() {
                        break;
                    }
                }
                if close {
                    let _ = socket.close(None).await;
                    break;
                }
                let mut reply = json!({"id":message["id"],"result":result});
                if let Some(session) = message.get("sessionId") {
                    reply["sessionId"] = session.clone();
                }
                if socket.send(Message::Text(reply.to_string())).await.is_err() {
                    break;
                }
            }
        });
        Self {
            endpoint: format!("ws://127.0.0.1:{port}/devtools/browser/fixture"),
            state,
            task,
        }
    }
}
