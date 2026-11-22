//! Model Context Protocol (MCP) server module for PolecatWorks Data-Engine.
//!
//! Follows the unified PolecatWorks MCP Server Pattern:
//! - [`McpConfig`] & [`McpError`] for fail-fast domain configuration and typed error handling
//! - [`server::DataEngineMcpServer`] using `rmcp` `#[tool]` / `#[tool_router]` + `schemars`
//! - [`rpc::handle_http_rpc`] & [`rpc::handle_json_rpc`] for JSON-RPC 2.0 over HTTP with tracing

pub mod rpc;
pub mod server;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::analytics::AnalyticsError;
use crate::visuals::VisualsError;

#[derive(Debug, Error)]
pub enum McpError {
    #[error("mcp.server_name must not be empty")]
    EmptyServerName,

    #[error("mcp.instructions must not be empty")]
    EmptyInstructions,

    #[error(transparent)]
    Analytics(#[from] AnalyticsError),

    #[error(transparent)]
    Visuals(#[from] VisualsError),

    #[error("invalid MCP tool arguments or payload: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Unknown MCP tool '{0}'")]
    UnknownTool(String),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct McpConfig {
    pub enabled: bool,
    pub server_name: String,
    pub instructions: String,
}

impl Default for McpConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            server_name: "data-engine-mcp".to_string(),
            instructions: "PolecatWorks Data-Engine Model Context Protocol Server for deterministic statistical analytics, data visualization recommendation and rendering, and semantic topology exploration.".to_string(),
        }
    }
}

impl McpConfig {
    pub fn validate(&self) -> Result<(), McpError> {
        if self.enabled {
            if self.server_name.trim().is_empty() {
                return Err(McpError::EmptyServerName);
            }
            if self.instructions.trim().is_empty() {
                return Err(McpError::EmptyInstructions);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_mcp_config() {
        let cfg = McpConfig::default();
        assert!(cfg.validate().is_ok());
        assert!(cfg.enabled);
        assert_eq!(cfg.server_name, "data-engine-mcp");
        assert!(!cfg.instructions.trim().is_empty());
    }

    #[test]
    fn test_empty_server_name_fails_validation() {
        let mut cfg = McpConfig::default();
        cfg.server_name = "   ".to_string();
        assert!(matches!(cfg.validate(), Err(McpError::EmptyServerName)));
    }

    #[test]
    fn test_empty_instructions_fails_validation() {
        let mut cfg = McpConfig::default();
        cfg.instructions = "   ".to_string();
        assert!(matches!(cfg.validate(), Err(McpError::EmptyInstructions)));
    }

    #[test]
    fn test_disabled_mcp_skips_validation() {
        let mut cfg = McpConfig::default();
        cfg.enabled = false;
        cfg.server_name = "".to_string();
        cfg.instructions = "".to_string();
        assert!(cfg.validate().is_ok());
    }
}
