use std::sync::Arc;

use rmcp::transport::streamable_http_server::{
    session::local::LocalSessionManager,
    tower::{StreamableHttpServerConfig, StreamableHttpService},
};

use tower_http::limit::RequestBodyLimitLayer;

use super::MemexMcp;
use crate::api::state::AppState;

/// Mounts the MCP server at `/mcp` (Streamable HTTP, stateless, JSON responses).
///
/// Max JSON-RPC request body on `/mcp`; larger requests get 413.
const MCP_BODY_LIMIT: usize = 16 * 1024 * 1024;

/// Never matches a real origin: any browser request (which carries `Origin`) is rejected,
/// while non-browser clients such as the gateway (no `Origin`) pass.
const NO_BROWSER_ORIGIN: &str = "http://memex.invalid";

/// The `Host` header is checked against `Config.mcp_allowed_hosts` (DNS-rebinding guard).
pub fn mcp_router<S>(state: AppState) -> axum::Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    // `StreamableHttpServerConfig` is `#[non_exhaustive]`: built via its `with_*` setters.
    let cfg = StreamableHttpServerConfig::default()
        .with_stateful_mode(false)
        .with_json_response(true)
        .with_allowed_hosts(state.config.mcp_allowed_hosts.clone())
        .with_allowed_origins([NO_BROWSER_ORIGIN]);
    let service = StreamableHttpService::new(
        move || Ok(MemexMcp::new(state.clone())),
        Arc::new(LocalSessionManager::default()),
        cfg,
    );
    axum::Router::new()
        .nest_service("/mcp", service)
        .layer(RequestBodyLimitLayer::new(MCP_BODY_LIMIT))
}
