#![cfg(unix)]
use async_trait::async_trait;
use magicvault_core::{store::SecretStore, InMemoryKeyProvider};
use magicvault_mcp::catalog;
use magicvault_service::{
    broker::Broker,
    client::Client,
    human::HumanInteraction,
    ipc,
    protocol::{EnrollRequest, ErrorCode, Request, Response},
    storage,
};
use rmcp::{
    model::{CallToolRequestParams, CallToolResponse},
    ServiceExt,
};
use std::{fs, os::unix::fs::PermissionsExt, sync::Arc, time::Duration};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;
use zeroize::Zeroizing;

#[test]
fn dependency_payload_logging_is_compiled_out_of_shipped_mcp() {
    assert_eq!(log::STATIC_MAX_LEVEL, log::LevelFilter::Off);
}

struct FixtureHuman;
#[async_trait]
impl HumanInteraction for FixtureHuman {
    async fn confirm(&self, _: &str, _: CancellationToken) -> Result<bool, ErrorCode> {
        Ok(true)
    }
    async fn secret(&self, _: &str, _: CancellationToken) -> Result<Zeroizing<String>, ErrorCode> {
        Ok(Zeroizing::new("SYNTHETIC-MCP-TRANSPORT-CANARY".into()))
    }
}
struct FixtureClient;
impl rmcp::ClientHandler for FixtureClient {}

#[test]
fn catalog_is_closed_and_has_no_administration_or_unimplemented_effects() {
    let tools = catalog();
    assert_eq!(
        tools.iter().map(|t| t.name.as_ref()).collect::<Vec<_>>(),
        [
            "vault_status",
            "list_credentials",
            "request_approval",
            "approval_status",
            "list_browsers",
            "browser_targets",
            "secure_fill",
            "fill_status",
            "cancel_fill"
        ]
    );
    for tool in tools {
        assert_eq!(tool.input_schema["additionalProperties"], false);
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn sdk_mcp_to_shared_client_to_real_ipc_to_core_returns_only_metadata() {
    let root = tempfile::Builder::new()
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir()
        .unwrap();
    let instance = storage::Instance {
        format_version: 1,
        id: Uuid::new_v4(),
    };
    fs::write(
        root.path().join("instance.json"),
        serde_json::to_vec(&instance).unwrap(),
    )
    .unwrap();
    fs::set_permissions(
        root.path().join("instance.json"),
        fs::Permissions::from_mode(0o600),
    )
    .unwrap();
    let lease = storage::open(root.path()).unwrap();
    let store = SecretStore::new_empty(
        Box::new(InMemoryKeyProvider::new()),
        root.path().join("vault"),
    );
    let broker = Broker::with_components(lease, store, Arc::new(FixtureHuman)).unwrap();
    let daemon = tokio::spawn(ipc::serve(Arc::clone(&broker)));
    tokio::time::timeout(Duration::from_secs(2), async {
        while !broker.socket().exists() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    Client::pair(root.path().to_owned(), "mcp", "MCP fixture".into())
        .await
        .unwrap();
    let client = Client::load(root.path().to_owned(), "mcp").unwrap();
    let Response::Enrolled(credential) = client
        .call(Request::Enroll(EnrollRequest {
            label: "Fixture".into(),
            field_names: vec!["password".into()],
        }))
        .await
        .unwrap()
    else {
        panic!("enroll");
    };

    // Exercise the shipped stdio binary, not only a handler constructed here.
    let mut server = tokio::process::Command::new(env!("CARGO_BIN_EXE_magicvault-mcp"))
        .arg("--root")
        .arg(root.path())
        .args(["--profile", "mcp"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let read = server.stdout.take().unwrap();
    let write = server.stdin.take().unwrap();
    let peer = FixtureClient.serve((read, write)).await.unwrap();
    assert_eq!(peer.list_tools(None).await.unwrap().tools.len(), 9);
    // Inspect a single response rather than letting the SDK drive continuation
    // rounds: this foundation must return Complete and never request more input.
    let response = peer
        .call_tool_once(CallToolRequestParams::new("list_credentials"))
        .await
        .unwrap();
    let CallToolResponse::Complete(result) = response else {
        panic!("complete metadata result");
    };
    let wire = serde_json::to_string(&result).unwrap();
    assert!(wire.contains("password"));
    assert!(!wire.contains("SYNTHETIC-MCP-TRANSPORT-CANARY"));
    let rejected = peer.call_tool_once(CallToolRequestParams::new("request_approval").with_arguments(
        serde_json::json!({"credential_ref":"cred_00000000-0000-0000-0000-000000000000","approved":true}).as_object().unwrap().clone()
    )).await;
    match rejected {
        Err(_) => {}
        Ok(CallToolResponse::Complete(result)) => assert_eq!(result.is_error, Some(true)),
        Ok(_) => panic!("no continuation or authority on invalid input"),
    }
    // The actual SDK subprocess routes the first implemented effect through
    // authenticated IPC, effect-bound human approval and the production CDP
    // transport. Only the remote browser peer/human input are synthetic.
    use magicvault_service::protocol::{
        BrowserQuery, BrowserRule, FillField, FillQuery, FillState, RegisterCdp, SecureFill,
    };
    client
        .call(Request::ConfigureBrowserCredential(BrowserRule {
            credential_ref: credential.credential_ref.clone(),
            origins: vec!["https://example.com".into()],
            field_names: vec!["password".into()],
        }))
        .await
        .unwrap();
    let cdp = magicvault_test_support::CdpFixture::start().await;
    let Response::Browser(browser) = client
        .call(Request::RegisterCdp(RegisterCdp {
            label: "Fixture".into(),
            endpoint: cdp.endpoint.clone(),
        }))
        .await
        .unwrap()
    else {
        panic!("browser");
    };
    let Response::BrowserTargets(targets) = client
        .call(Request::BrowserTargets(BrowserQuery {
            browser_handle: browser.browser_handle,
        }))
        .await
        .unwrap()
    else {
        panic!("targets");
    };
    let fill = SecureFill {
        operation_id: Uuid::new_v4(),
        browser_handle: browser.browser_handle,
        target_handle: targets[0].target_handle,
        fields: vec![FillField {
            css: "#password".into(),
            credential_ref: credential.credential_ref,
            credential_field: "password".into(),
        }],
    };
    let result = peer
        .call_tool_once(
            CallToolRequestParams::new("secure_fill").with_arguments(
                serde_json::to_value(&fill)
                    .unwrap()
                    .as_object()
                    .unwrap()
                    .clone(),
            ),
        )
        .await
        .unwrap();
    let CallToolResponse::Complete(result) = result else {
        panic!("complete fill request");
    };
    assert_ne!(result.is_error, Some(true));
    assert!(!serde_json::to_string(&result)
        .unwrap()
        .contains("SYNTHETIC-MCP-TRANSPORT-CANARY"));
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let result = peer
                .call_tool_once(
                    CallToolRequestParams::new("fill_status").with_arguments(
                        serde_json::to_value(FillQuery {
                            operation_id: fill.operation_id,
                        })
                        .unwrap()
                        .as_object()
                        .unwrap()
                        .clone(),
                    ),
                )
                .await
                .unwrap();
            let CallToolResponse::Complete(result) = result else {
                panic!("complete fill status");
            };
            let wire = serde_json::to_value(result).unwrap();
            let reply: Response =
                serde_json::from_str(wire["content"][0]["text"].as_str().unwrap()).unwrap();
            let Response::Fill(status) = reply else {
                panic!("fill status");
            };
            if !matches!(status.state, FillState::Pending | FillState::Filling) {
                assert_eq!(status.state, FillState::Filled);
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert_eq!(
        cdp.state.lock().unwrap().delivered,
        [vec!["SYNTHETIC-MCP-TRANSPORT-CANARY".to_owned()]]
    );
    peer.cancel().await.unwrap();
    assert!(tokio::time::timeout(Duration::from_secs(10), server.wait())
        .await
        .unwrap()
        .unwrap()
        .success());
    broker.shutdown.cancel();
    daemon.await.unwrap().unwrap();
}

#[tokio::test]
async fn mcp_rejected_arguments_never_echo_input() {
    let output = tokio::process::Command::new(env!("CARGO_BIN_EXE_magicvault-mcp"))
        .args(["--token", "SYNTHETIC-REJECTED-CANARY"])
        .output()
        .await
        .unwrap();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert_eq!(
        String::from_utf8(output.stderr).unwrap(),
        "{\"error\":\"invalid_request\"}\n"
    );
}
