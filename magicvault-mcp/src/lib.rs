//! The SDK owns MCP protocol/lifecycle. Only implemented metadata/consent tools
//! are routed to the daemon. Native administration is deliberately not a tool.
use std::sync::Arc;
use magicvault_service::{client::Client, protocol::{Request, Response}};
use rmcp::{ServerHandler, ErrorData as McpError, model::*, service::{RequestContext, RoleServer}};
use serde_json::{json, Map, Value};
pub mod transport;

pub struct MagicVaultMcp { client: Arc<Client>, slots: tokio::sync::Semaphore }
impl MagicVaultMcp {
    pub fn new(client: Client) -> Self { Self { client: Arc::new(client), slots: tokio::sync::Semaphore::new(8) } }

    pub async fn invoke(&self, name: &str, arguments: Map<String, Value>) -> Result<Response, magicvault_service::protocol::ErrorCode> {
        let _permit = self.slots.try_acquire().map_err(|_| magicvault_service::protocol::ErrorCode::Busy)?;
        let invalid = magicvault_service::protocol::ErrorCode::InvalidRequest;
        let request = match name {
            "vault_status" if arguments.is_empty() => return self.client.status().await.map(Response::Status),
            "list_credentials" if arguments.is_empty() => Request::ListCredentials,
            "request_approval" => Request::RequestAccess(serde_json::from_value(Value::Object(arguments)).map_err(|_| invalid)?),
            "approval_status" => Request::ApprovalStatus(serde_json::from_value(Value::Object(arguments)).map_err(|_| invalid)?),
            _ => return Err(invalid),
        };
        let response = self.client.call(request).await?;
        // Enforce the response boundary even if a daemon is upgraded separately.
        match response {
            Response::Credentials(_) | Response::Approval(_) => Ok(response),
            _ => Err(magicvault_service::protocol::ErrorCode::TransportUncertain),
        }
    }
}

pub fn catalog() -> Vec<Tool> {
    let empty = json!({"type":"object","properties":{},"additionalProperties":false});
    let reference = json!({"type":"object","properties":{"credential_ref":{"type":"string","maxLength":41}},"required":["credential_ref"],"additionalProperties":false});
    let approval = json!({"type":"object","properties":{"approval_id":{"type":"string","format":"uuid","maxLength":36}},"required":["approval_id"],"additionalProperties":false});
    [
        ("vault_status", "Inspect standalone MagicVault readiness and pairing. No browser/HTTP/process effects are implemented in this foundation.", empty.clone()),
        ("list_credentials", "List only credential references, labels and field names permitted for this paired client. Never returns values.", empty),
        ("request_approval", "Ask the human to allow metadata discovery for one credential reference. Returns a pending metadata-connection approval, not permission to reveal values or perform future effects. Do not ask for raw credentials on denial.", reference),
        ("approval_status", "Inspect your own metadata-connection request. Poll after the human responds in MagicVault's native dialog. Unknown after restart means no effect/use authority; do not infer approval.", approval),
    ].into_iter().map(|(name, description, schema)| Tool::new(name, description, schema.as_object().expect("static object schema").clone())).collect()
}

impl ServerHandler for MagicVaultMcp {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("magicvault-mcp", env!("CARGO_PKG_VERSION")))
            .with_instructions("Reference-only foundation. Enrollment/pairing are human CLI/native flows. No material-read, human-grant, secure_fill, HTTP or process tool is available.")
    }
    fn get_tool(&self, name: &str) -> Option<Tool> { catalog().into_iter().find(|tool| tool.name == name) }
    async fn list_tools(&self, request: Option<PaginatedRequestParams>, _: RequestContext<RoleServer>) -> Result<ListToolsResult, McpError> {
        if request.and_then(|r| r.cursor).is_some() { return Err(McpError::invalid_params("invalid cursor", None)); }
        Ok(ListToolsResult::with_all_items(catalog()))
    }
    async fn call_tool(&self, request: CallToolRequestParams, _: RequestContext<RoleServer>) -> Result<CallToolResponse, McpError> {
        if request.input_responses.is_some() || request.request_state.is_some() {
            return Err(McpError::invalid_params("input continuations are unsupported", None));
        }
        let result = self.invoke(&request.name, request.arguments.unwrap_or_default()).await;
        let (value, error) = match result {
            Ok(response) => match serde_json::to_value(response) {
                Ok(value) => (value, false),
                Err(_) => (json!({"error":"unavailable"}), true),
            },
            Err(code) => (json!({"error":code}), true),
        };
        let content = vec![ContentBlock::text(value.to_string())];
        Ok(CallToolResponse::Complete(if error { CallToolResult::error(content) } else { CallToolResult::success(content) }))
    }
}
