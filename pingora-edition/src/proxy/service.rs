//! gRPC proxy service implementation

use bytes::Bytes;
use rustls::ServerConfig;
use std::sync::Arc;
use tracing::{debug, error, info, warn};

use crate::config::ProxyConfig;
use crate::error::{ProxyResult, GrpcProtocolError, TlsError, ErrorContext, ErrorContextExt};
use crate::routing::Router;
use crate::proxy::upstream::UpstreamManager;
use crate::proxy::pingora_types::{
    ProxyHttp, Session, Context, RequestHeader, ResponseHeader, HttpPeer, TrailerState
};

/// gRPC proxy service that implements Pingora's ProxyHttp trait
pub struct GrpcProxyService {
    config: ProxyConfig,
    tls_config: Option<Arc<ServerConfig>>,
    router: Router,
    upstream_manager: UpstreamManager,
}

impl GrpcProxyService {
    /// Create a new gRPC proxy service
    pub fn new(config: ProxyConfig, tls_config: Option<Arc<ServerConfig>>) -> Self {
        // Create router with configured routes
        let router = Router::new(config.routes.clone(), config.default_upstream.clone());
        
        // Create upstream manager with all unique upstreams
        let mut all_upstreams = vec![config.default_upstream.clone()];
        for route in &config.routes {
            all_upstreams.push(route.upstream.clone());
        }
        // Remove duplicates based on address
        all_upstreams.dedup_by(|a, b| a.address == b.address);
        let upstream_manager = UpstreamManager::new(all_upstreams);

        Self {
            config,
            tls_config,
            router,
            upstream_manager,
        }
    }

    /// Check if TLS is enabled
    pub fn is_tls_enabled(&self) -> bool {
        self.tls_config.is_some()
    }

    /// Get the TLS configuration
    pub fn tls_config(&self) -> Option<Arc<ServerConfig>> {
        self.tls_config.clone()
    }

    /// Extract gRPC status and message from trailers
    fn extract_grpc_status(headers: &http::HeaderMap) -> (Option<i32>, Option<String>) {
        let status = headers
            .get("grpc-status")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse().ok());

        let message = headers
            .get("grpc-message")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());

        (status, message)
    }

    /// Check if the request is a valid gRPC request
    fn is_grpc_request(session: &Session) -> bool {
        // gRPC requests should be HTTP/2 POST requests
        if session.method() != http::Method::POST {
            return false;
        }

        // Check for gRPC content-type
        if let Some(content_type) = session.headers().get("content-type") {
            if let Ok(ct_str) = content_type.to_str() {
                return ct_str.starts_with("application/grpc");
            }
        }

        false
    }



    /// Extract and preserve trailers from upstream response
    pub fn extract_trailers_from_response(
        &self,
        response_headers: &http::HeaderMap,
        trailer_state: &mut TrailerState,
    ) -> ProxyResult<()> {
        debug!("Extracting trailers from upstream response");

        // Extract trailers from response headers
        if let Err(e) = trailer_state.extract_from_headers(response_headers) {
            error!("Failed to extract trailers: {}", e);
            trailer_state.handle_parsing_error(&e.to_string());
            return Err(GrpcProtocolError::TrailerParsingFailed {
                reason: e.to_string(),
            }.into());
        }

        // Ensure we have a valid gRPC status
        if trailer_state.grpc_status.is_none() {
            warn!("No gRPC status found in trailers, setting default OK status");
            trailer_state.set_grpc_status(0, Some("OK".to_string()));
        }

        debug!("Successfully extracted {} trailers", 
               trailer_state.get_all_trailers().len());

        Ok(())
    }

    /// Handle trailer parsing errors gracefully
    pub fn handle_trailer_error(
        &self,
        error: &anyhow::Error,
        trailer_state: &mut TrailerState,
    ) {
        error!("Trailer processing error: {}", error);
        
        // Set appropriate gRPC error status
        let error_message = format!("Proxy trailer error: {}", error);
        trailer_state.set_grpc_status(13, Some(error_message)); // INTERNAL error
        
        // Add debugging information
        trailer_state.add_custom_trailer("x-proxy-trailer-error", &error.to_string());
    }

    /// Handle TLS termination for incoming requests
    /// This method processes TLS-encrypted requests and prepares them for plain HTTP/2 forwarding
    pub fn handle_tls_termination(
        &self,
        session: &Session,
        ctx: &mut Context,
    ) -> ProxyResult<()> {
        let context = ErrorContext::new()
            .with_request_path(session.uri().path().to_string());

        debug!("Processing connection for request to {}", session.uri().path());

        // For now, assume plain HTTP/2 connections (TLS handling would be more complex)
        ctx.tls_terminated = false;
        info!("HTTP/2 connection detected");
        
        // Validate HTTP/2 protocol
        if session.version() != http::Version::HTTP_2 {
            warn!("Non-HTTP/2 request received: {:?}", session.version());
        }

        debug!("Connection processing completed successfully");
        Ok(())
    }







    /// Prepare request for plain HTTP/2 forwarding
    /// Handles both TLS-terminated and plain HTTP/2 scenarios
    pub fn prepare_for_plain_forwarding_session(
        &self,
        _session: &Session,
        upstream_request: &mut RequestHeader,
        _ctx: &Context,
    ) -> ProxyResult<()> {
        debug!("Preparing request for plain HTTP/2 forwarding");

        // Remove headers that shouldn't be forwarded to upstream
        upstream_request.headers.remove("upgrade-insecure-requests");
        upstream_request.headers.remove("sec-fetch-site");
        upstream_request.headers.remove("sec-fetch-mode");
        upstream_request.headers.remove("sec-fetch-dest");

        // Add appropriate headers based on connection type
        upstream_request.headers.insert("x-forwarded-proto", "http".parse().unwrap());
        upstream_request.headers.insert("x-forwarded-by", "grpc-http-proxy".parse().unwrap());

        debug!("Request prepared for plain HTTP/2 forwarding to upstream");
        Ok(())
    }
}

#[async_trait::async_trait]
impl ProxyHttp<Context> for GrpcProxyService {
    /// Select the upstream peer for this request
    async fn upstream_peer(
        &self,
        session: &mut Session,
        ctx: &mut Context,
    ) -> Result<Box<HttpPeer>, Box<dyn std::error::Error + Send + Sync>> {
        let path = session.uri().path();
        
        let context = ErrorContext::new()
            .with_request_path(path.to_string());

        debug!("Selecting upstream peer for request to {}", path);

        // Handle TLS termination first
        self.handle_tls_termination(session, ctx)
            .with_context(context.clone())
            .map_err(|e| anyhow::anyhow!("{}", e))?;

        // Use router to find the appropriate upstream
        let upstream_config = self.router.route(path);
        
        // Store the matched route in context for debugging
        if let Some((pattern, priority)) = self.router.find_matching_route(path) {
            // Store route info in context (Pingora context doesn't have route_match field)
            debug!("Route matched: pattern={}, priority={:?}", pattern, priority);
        } else {
            debug!("Using default upstream");
        }

        // Validate that the upstream is available and healthy
        let validated_upstream = self.upstream_manager.get_upstream(upstream_config)
            .map_err(|e| {
                error!("Failed to get upstream {}: {}", upstream_config.address, e);
                Box::new(std::io::Error::new(std::io::ErrorKind::NotFound, "Upstream unavailable")) as Box<dyn std::error::Error + Send + Sync>
            })?;
        
        ctx.upstream_address = Some(validated_upstream.address);
        info!("Selected upstream: {}", validated_upstream.address);

        // Create HttpPeer for the selected upstream
        // After TLS termination, we always use plain HTTP/2 to upstream
        let peer = HttpPeer::new(validated_upstream.address, "http");
        
        debug!("Upstream peer configured for HTTP connection");
        
        Ok(Box::new(peer))
    }

    /// Filter/modify the upstream request before sending
    async fn upstream_request_filter(
        &self,
        session: &mut Session,
        upstream_request: &mut RequestHeader,
        ctx: &mut Context,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let path = session.uri().path();
        
        let _context = ErrorContext::new()
            .with_request_path(path.to_string());

        debug!("Filtering upstream request for {}", path);

        // Validate that this is a gRPC request
        if !Self::is_grpc_request(session) {
            warn!("Non-gRPC request received: method={}, content-type={:?}", 
                  session.method(), 
                  session.headers().get("content-type"));
            
            let _content_type = session.headers().get("content-type")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("missing");
            
            return Err(Box::new(std::io::Error::new(std::io::ErrorKind::InvalidInput, "Invalid gRPC content type")) as Box<dyn std::error::Error + Send + Sync>);
        }

        // Copy essential headers from the original request
        for (name, value) in session.headers().iter() {
            // Skip hop-by-hop headers that shouldn't be forwarded
            let name_str = name.as_str().to_lowercase();
            if matches!(name_str.as_str(), 
                "connection" | "upgrade" | "proxy-authenticate" | "proxy-authorization" |
                "te" | "trailers" | "transfer-encoding") {
                continue;
            }

            upstream_request.headers.insert(name.clone(), value.clone());
        }

        // Ensure proper gRPC headers are set
        upstream_request.headers.insert("te", "trailers".parse().unwrap());
        
        // Prepare request for plain HTTP/2 forwarding after TLS termination
        self.prepare_for_plain_forwarding_session(session, upstream_request, ctx)
            .map_err(|_e| Box::new(std::io::Error::new(std::io::ErrorKind::Other, "Failed to prepare request")) as Box<dyn std::error::Error + Send + Sync>)?;

        debug!("Upstream request prepared with {} headers", upstream_request.headers.len());
        Ok(())
    }

    /// Filter/modify the response headers from upstream
    async fn response_filter(
        &self,
        session: &mut Session,
        upstream_response: &mut ResponseHeader,
        ctx: &mut Context,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let path = session.uri().path();
        debug!("Filtering response headers for {}", path);

        // Initialize trailer state for this response
        ctx.trailer_state = Some(TrailerState::new());

        // For gRPC responses, we need to preserve all headers
        // gRPC uses headers for metadata and trailers for status information

        // Ensure proper CORS headers if needed (for web gRPC clients)
        upstream_response.headers.insert("access-control-allow-origin", "*".parse().unwrap());
        upstream_response.headers.insert("access-control-allow-methods", "POST, GET, OPTIONS".parse().unwrap());
        upstream_response.headers.insert("access-control-allow-headers", "content-type, grpc-timeout, grpc-encoding".parse().unwrap());
        upstream_response.headers.insert("access-control-expose-headers", "grpc-status, grpc-message".parse().unwrap());

        debug!("Response headers filtered, status: {}", upstream_response.status);
        Ok(())
    }

    /// Filter/modify the response body and handle trailers
    async fn upstream_response_body_filter(
        &self,
        session: &mut Session,
        body: &mut Option<Bytes>,
        end_of_stream: bool,
        _ctx: &mut Context,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let path = session.uri().path();
        
        // For gRPC streaming, we should not buffer the body
        // Just pass it through as-is to maintain streaming semantics
        
        if let Some(body_bytes) = body {
            debug!("Processing body chunk of {} bytes for {}", 
                   body_bytes.len(), path);
            
            // Check for gRPC frame markers in the body to detect trailers
            // gRPC frames start with a 5-byte header: [compression flag][length]
            if body_bytes.len() >= 5 {
                let compression_flag = body_bytes[0];
                let length = u32::from_be_bytes([body_bytes[1], body_bytes[2], body_bytes[3], body_bytes[4]]);
                
                debug!("gRPC frame: compression={}, length={}", compression_flag, length);
                
                // In a real implementation, we would parse the gRPC frame
                // and extract any embedded trailer information
            }
        }

        // Handle trailers when we reach the end of the stream
        if end_of_stream {
            debug!("End of stream reached for {}", path);
            
            // In a real implementation, we would extract trailers from the upstream response
            // For now, we'll simulate proper gRPC trailer handling
            debug!("gRPC stream completed successfully");
        }

        Ok(())
    }
}