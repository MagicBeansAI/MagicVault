//! The SDK owns MCP protocol/lifecycle. Only implemented metadata/consent tools
//! and browser effects are routed to the daemon. Native administration is
//! deliberately not a tool. No material-bearing integration message is routed.
use magicvault_service::{
    client::Client,
    protocol::{Request, Response},
};
use rmcp::{
    model::*,
    service::{RequestContext, RoleServer},
    ErrorData as McpError, ServerHandler,
};
use serde_json::{json, Map, Value};
use std::sync::Arc;
pub mod transport;

pub struct MagicVaultMcp {
    client: Arc<Client>,
    slots: tokio::sync::Semaphore,
}
impl MagicVaultMcp {
    pub fn new(client: Client) -> Self {
        Self {
            client: Arc::new(client),
            slots: tokio::sync::Semaphore::new(8),
        }
    }

    pub async fn invoke(
        &self,
        name: &str,
        arguments: Map<String, Value>,
    ) -> Result<Response, magicvault_service::protocol::ErrorCode> {
        let _permit = self
            .slots
            .try_acquire()
            .map_err(|_| magicvault_service::protocol::ErrorCode::Busy)?;
        let invalid = magicvault_service::protocol::ErrorCode::InvalidRequest;
        let request = match name {
            "vault_status" if arguments.is_empty() => {
                return self.client.status().await.map(Response::Status)
            }
            "list_credentials" if arguments.is_empty() => Request::ListCredentials,
            "request_approval" => Request::RequestAccess(
                serde_json::from_value(Value::Object(arguments)).map_err(|_| invalid)?,
            ),
            "approval_status" => Request::ApprovalStatus(
                serde_json::from_value(Value::Object(arguments)).map_err(|_| invalid)?,
            ),
            "list_browsers" if arguments.is_empty() => Request::ListBrowsers,
            "browser_targets" => Request::BrowserTargets(
                serde_json::from_value(Value::Object(arguments)).map_err(|_| invalid)?,
            ),
            "secure_fill" => Request::SecureFill(
                serde_json::from_value(Value::Object(arguments)).map_err(|_| invalid)?,
            ),
            "fill_status" => Request::FillStatus(
                serde_json::from_value(Value::Object(arguments)).map_err(|_| invalid)?,
            ),
            "cancel_fill" => Request::CancelFill(
                serde_json::from_value(Value::Object(arguments)).map_err(|_| invalid)?,
            ),
            _ => return Err(invalid),
        };
        let response = self.client.call(request).await?;
        // Enforce the response boundary even if a daemon is upgraded separately.
        match (name, &response) {
            ("list_credentials", Response::Credentials(_))
            | ("request_approval" | "approval_status", Response::Approval(_))
            | ("list_browsers", Response::Browsers(_))
            | ("browser_targets", Response::BrowserTargets(_))
            | ("secure_fill" | "fill_status" | "cancel_fill", Response::Fill(_)) => Ok(response),
            _ => Err(magicvault_service::protocol::ErrorCode::TransportUncertain),
        }
    }
}

pub fn catalog() -> Vec<Tool> {
    let empty = json!({"type":"object","properties":{},"additionalProperties":false});
    let reference = json!({"type":"object","properties":{"credential_ref":{"type":"string","maxLength":41}},"required":["credential_ref"],"additionalProperties":false});
    let approval = json!({"type":"object","properties":{"approval_id":{"type":"string","format":"uuid","maxLength":36}},"required":["approval_id"],"additionalProperties":false});
    let browser = json!({"type":"object","properties":{"browser_handle":{"type":"string","format":"uuid","maxLength":36}},"required":["browser_handle"],"additionalProperties":false});
    let operation = json!({"type":"object","properties":{"operation_id":{"type":"string","format":"uuid","maxLength":36}},"required":["operation_id"],"additionalProperties":false});
    let fill = json!({"type":"object","additionalProperties":false,"required":["operation_id","browser_handle","target_handle","fields"],"properties":{
        "operation_id":{"type":"string","format":"uuid","maxLength":36},
        "browser_handle":{"type":"string","format":"uuid","maxLength":36},
        "target_handle":{"type":"string","format":"uuid","maxLength":36},
        "fields":{"type":"array","minItems":1,"maxItems":8,"items":{"type":"object","additionalProperties":false,"required":["css","credential_ref","credential_field"],"properties":{
            "css":{"type":"string","minLength":1,"maxLength":512},"credential_ref":{"type":"string","maxLength":41},"credential_field":{"type":"string","minLength":1,"maxLength":64}
        }}}
    }});
    [
        ("vault_status", "Inspect standalone MagicVault readiness, pairing and implemented effects. Browser delivery uses secure_fill; HTTP/process effects are not implemented.", empty.clone()),
        ("list_credentials", "List only credential references, labels and field names permitted for this paired client. Never returns values.", empty.clone()),
        ("request_approval", "Ask the human to allow metadata discovery for one credential reference. Returns a pending metadata-connection approval, not permission to reveal values or perform future effects. Do not ask for raw credentials on denial.", reference),
        ("approval_status", "Inspect your own metadata-connection request. Poll after the human responds in MagicVault's native dialog. Unknown after restart means no effect/use authority; do not infer approval.", approval),
        ("list_browsers", "List browsers connected for your paired profile. If none, request human CDP/extension setup. Never ask for credentials or debugging capabilities in chat.", empty),
        ("browser_targets", "Discover value-free, short-lived document/frame handles in a registered browser. Rediscovery replaces unused handles. Inspect origin and top_origin; no page dump or field values are returned.", browser),
        ("secure_fill", "Fill credential fields using stored references instead of ordinary typing. Requires browser permission and native human consent for this exact operation. Supply strict CSS locators, never values or another tool's snapshot refs. Choose a fresh operation UUID once, retain it, and poll fill_status. The target handle is single-use. Does not submit or claim login success. Never retry with a new ID after uncertainty; never fall back to retrieving/pasting secrets on denial. Other browser tools' later observations are not filtered by this tool.", fill),
        ("fill_status", "Inspect your browser fill operation. Pending requires human action; filling requires waiting. Filled means delivery only, not login. Partial/uncertain means some writes may have happened; do not automatically repeat. Missing after restart is not success or safe-to-retry evidence.", operation.clone()),
        ("cancel_fill", "Request cancellation of your pending or running fill. Poll fill_status for the actual outcome. Cancellation cannot recall values already delivered to the browser.", operation),
    ].into_iter().map(|(name, description, schema)| Tool::new(name, description, schema.as_object().expect("static object schema").clone())).collect()
}

impl ServerHandler for MagicVaultMcp {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("magicvault-mcp", env!("CARGO_PKG_VERSION")))
            .with_instructions("Reference-only credential requests. Browser secret fields use secure_fill through an explicitly connected CDP browser or extension. Navigation and submission stay with the existing browser tool. Enrollment, pairing, browser setup and browser policy use human CLI/native flows. No material-read, human-grant, arbitrary JavaScript, HTTP or process tool is exposed. Browser output/capture filtering outside MagicVault remains out of scope.")
    }
    fn get_tool(&self, name: &str) -> Option<Tool> {
        catalog().into_iter().find(|tool| tool.name == name)
    }
    async fn list_tools(
        &self,
        request: Option<PaginatedRequestParams>,
        _: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        if request.and_then(|r| r.cursor).is_some() {
            return Err(McpError::invalid_params("invalid cursor", None));
        }
        Ok(ListToolsResult::with_all_items(catalog()))
    }
    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, McpError> {
        if request.input_responses.is_some() || request.request_state.is_some() {
            return Err(McpError::invalid_params(
                "input continuations are unsupported",
                None,
            ));
        }
        let result = self
            .invoke(&request.name, request.arguments.unwrap_or_default())
            .await;
        let (value, error) = match result {
            Ok(response) => match serde_json::to_value(response) {
                Ok(value) => (value, false),
                Err(_) => (json!({"error":"unavailable"}), true),
            },
            Err(code) => (json!({"error":code}), true),
        };
        let content = vec![ContentBlock::text(value.to_string())];
        Ok(CallToolResponse::Complete(if error {
            CallToolResult::error(content)
        } else {
            CallToolResult::success(content)
        }))
    }
}
