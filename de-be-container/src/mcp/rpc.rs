//! JSON-RPC 2.0 over HTTP handler for the Data-Engine MCP server.
//!
//! Matches the unified PolecatWorks MCP JSON-RPC 2.0 pattern with structured tracing,
//! duration instrumentation, and standard JSON-RPC error codes (`-32600`, `-32601`, `-32602`).

use axum::{Json, extract::State, http::StatusCode, response::IntoResponse};
use serde_json::{Value, json};
use tracing::{debug, error, instrument, trace};

use crate::web::AppState;

pub async fn handle_http_rpc(
    State(state): State<AppState>,
    Json(payload): Json<Value>,
) -> impl IntoResponse {
    match handle_json_rpc(&state, payload).await {
        Some(resp) => (StatusCode::OK, Json(resp)).into_response(),
        None => StatusCode::NO_CONTENT.into_response(),
    }
}

#[instrument(
    skip(state, raw_request),
    fields(method = tracing::field::Empty, req_id = tracing::field::Empty)
)]
pub async fn handle_json_rpc(state: &AppState, raw_request: Value) -> Option<Value> {
    let id = raw_request.get("id").cloned();
    let current_span = tracing::Span::current();
    if let Some(ref id_val) = id {
        current_span.record("req_id", tracing::field::display(id_val));
    }

    let method = match raw_request.get("method").and_then(|m| m.as_str()) {
        Some(m) => {
            current_span.record("method", m);
            m
        }
        None => {
            debug!("MCP JSON-RPC request missing method");
            return id.map(|id_val| {
                json!({
                    "jsonrpc": "2.0",
                    "id": id_val,
                    "error": {
                        "code": -32600,
                        "message": "Invalid Request: missing method"
                    }
                })
            });
        }
    };

    trace!("Processing MCP JSON-RPC method '{}'", method);
    let params = raw_request.get("params").cloned().unwrap_or(Value::Null);

    let result = match method {
        "initialize" => {
            debug!("MCP client initialize received");
            json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {
                    "tools": {}
                },
                "serverInfo": {
                    "name": state.config.mcp.server_name,
                    "version": env!("CARGO_PKG_VERSION")
                },
                "instructions": state.config.mcp.instructions
            })
        }
        "notifications/initialized" => {
            debug!("MCP client initialized notification received");
            if id.is_none() {
                return None;
            }
            json!({})
        }
        "ping" => {
            trace!("MCP ping received");
            json!({})
        }
        "tools/list" => {
            let tools = state.mcp_server.list_tools();
            debug!("MCP tools/list returning {} tools", tools.len());
            json!({
                "tools": tools
            })
        }
        "tools/call" => {
            let tool_name = match params.get("name").and_then(|n| n.as_str()) {
                Some(n) => n,
                None => {
                    debug!("MCP tools/call missing 'name' parameter");
                    return id.map(|id_val| {
                        json!({
                            "jsonrpc": "2.0",
                            "id": id_val,
                            "error": {
                                "code": -32602,
                                "message": "Invalid params: 'name' is required"
                            }
                        })
                    });
                }
            };

            let arguments = params
                .get("arguments")
                .cloned()
                .unwrap_or_else(|| json!({}));
            debug!(tool = tool_name, args = %arguments, "MCP tools/call invoked");

            let start = std::time::Instant::now();
            let res = state.mcp_server.call_tool(state, tool_name, arguments).await;
            let elapsed = start.elapsed();

            match res {
                Ok(payload) => {
                    let text =
                        serde_json::to_string_pretty(&payload).unwrap_or_else(|_| "{}".to_string());
                    debug!(
                        tool = tool_name,
                        duration_ms = elapsed.as_millis(),
                        "MCP tool execution succeeded"
                    );
                    json!({
                        "content": [
                            {
                                "type": "text",
                                "text": text
                            }
                        ],
                        "isError": false
                    })
                }
                Err(err) => {
                    error!(
                        tool = tool_name,
                        duration_ms = elapsed.as_millis(),
                        error = %err,
                        "MCP tool execution failed"
                    );
                    json!({
                        "content": [
                            {
                                "type": "text",
                                "text": err.to_string()
                            }
                        ],
                        "isError": true
                    })
                }
            }
        }
        _ => {
            debug!("MCP JSON-RPC method '{}' not found", method);
            return id.map(|id_val| {
                json!({
                    "jsonrpc": "2.0",
                    "id": id_val,
                    "error": {
                        "code": -32601,
                        "message": format!("Method '{}' not found", method)
                    }
                })
            });
        }
    };

    Some(match id {
        Some(id_val) => json!({
            "jsonrpc": "2.0",
            "id": id_val,
            "result": result
        }),
        None => result,
    })
}
