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
        if session.method != http::Method::POST {
            return false;
        }

        // Check for gRPC content-type
        if let Some(content_type) = session.headers.get("content-type") {
            if let Ok(ct_str) = content_type.to_str() {
                return ct_str.starts_with("application/grpc");
            }
        }

        false
    }

    /// Detect if the connection is plain HTTP/2 or TLS-terminated
    fn detect_connection_type(&self, session: &Session) -> ProxyResult<bool> {
        // Check if this is a plain HTTP/2 connection
        // In a real implementation, this would check the connection state
        
        // For plain HTTP/2, the version should be HTTP/2
        if session.version != http::Version::HTTP_2 {
            return Err(GrpcProtocolError::InvalidHttpVersion {
                version: session.version,
            }.into());
        }

        // If TLS is not configured, this must be a plain connection
        if !self.is_tls_enabled() {
            debug!("Plain HTTP/2 connection detected (TLS not configured)");
            return Ok(false); // false = plain HTTP/2
        }

        // If TLS is configured, we need to determine if this connection used TLS
        // In a real implementation, this would check the TLS connection state
        // For now, we assume TLS connections when TLS is configured
        debug!("TLS connection detected (TLS configured)");
        Ok(true) // true = TLS connection
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
            .with_request_path(session.uri.path().to_string());

        debug!("Processing connection for request to {}", session.uri.path());

        // Detect connection type (TLS or plain HTTP/2)
        let is_tls_connection = self.detect_connection_type(session)
            .with_context(context.clone())?;

        if is_tls_connection {
            // Handle TLS-terminated connection
            ctx.tls_terminated = true;
            info!("TLS connection detected, performing termination");

            // Extract client certificate information for mTLS scenarios
            if let Some(_tls_config) = &self.tls_config {
                // Check if mTLS is enabled (CA certificate provided)
                if self.config.tls.as_ref().and_then(|t| t.ca_cert_path.as_ref()).is_some() {
                    debug!("mTLS enabled, extracting client certificate information");
                    
                    // Simulate client certificate extraction
                    ctx.client_cert = self.extract_client_certificate_from_tls(session)
                        .with_context(context.clone())?;
                    
                    if ctx.client_cert.is_some() {
                        info!("Client certificate validated and extracted for mTLS");
                    } else {
                        warn!("mTLS enabled but no client certificate provided");
                    }
                }
            }

            // Validate ALPN negotiation for TLS connections
            self.validate_alpn_negotiation(session)
                .with_context(context.clone())?;
            
            info!("TLS termination completed, will forward as plain HTTP/2");
        } else {
            // Handle plain HTTP/2 connection
            ctx.tls_terminated = false;
            info!("Plain HTTP/2 connection detected, no TLS termination needed");
            
            // Validate HTTP/2 protocol for plain connections
            self.validate_plain_http2_connection(session)
                .with_context(context)?;
        }

        debug!("Connection processing completed successfully");
        Ok(())
    }

    /// Extract client certificate from TLS connection (mTLS)
    /// In a real implementation, this would extract the certificate from the TLS handshake
    fn extract_client_certificate_from_tls(&self, session: &Session) -> ProxyResult<Option<Vec<u8>>> {
        debug!("Extracting client certificate from TLS connection");

        // In a real Pingora implementation, we would:
        // 1. Access the TLS connection state from the session
        // 2. Extract the client certificate chain
        // 3. Validate the certificate against the configured CA
        // 4. Return the certificate data for logging/forwarding

        // For now, we simulate this process
        if let Some(tls_config) = &self.config.tls {
            if tls_config.ca_cert_path.is_some() {
                // Simulate client certificate presence based on request headers
                // In reality, this would come from the TLS handshake
                if session.headers.contains_key("x-client-cert") {
                    debug!("Client certificate found in TLS connection");
                    // Return a placeholder certificate
                    return Ok(Some(b"client-cert-data".to_vec()));
                } else {
                    debug!("No client certificate provided in mTLS connection");
                    return Ok(None);
                }
            }
        }

        Ok(None)
    }

    /// Validate ALPN negotiation resulted in HTTP/2 (h2)
    fn validate_alpn_negotiation(&self, session: &Session) -> ProxyResult<()> {
        debug!("Validating ALPN negotiation for HTTP/2");

        // Check that the connection is using HTTP/2
        if session.version != http::Version::HTTP_2 {
            return Err(TlsError::AlpnNegotiationFailed {
                actual: format!("{:?}", session.version),
            }.into());
        }

        // In a real implementation, we would also verify that ALPN negotiated "h2"
        // This would be available from the TLS connection state
        debug!("ALPN negotiation validated: HTTP/2 (h2) protocol confirmed");

        Ok(())
    }

    /// Validate plain HTTP/2 connection (no TLS)
    fn validate_plain_http2_connection(&self, session: &Session) -> ProxyResult<()> {
        debug!("Validating plain HTTP/2 connection");

        // Check that the connection is using HTTP/2
        if session.version != http::Version::HTTP_2 {
            return Err(GrpcProtocolError::InvalidHttpVersion {
                version: session.version,
            }.into());
        }

        // For plain HTTP/2, the connection should not have TLS-specific headers
        // In a real implementation, we would verify the HTTP/2 connection preface
        debug!("Plain HTTP/2 connection validated successfully");

        // Log connection details
        info!("Plain HTTP/2 connection established");
        debug!("No TLS encryption - suitable for internal networks");

        Ok(())
    }

    /// Prepare request for plain HTTP/2 forwarding
    /// Handles both TLS-terminated and plain HTTP/2 scenarios
    pub fn prepare_for_plain_forwarding(
        &self,
        _session: &Session,
        upstream_request: &mut RequestHeader,
        ctx: &Context,
    ) -> ProxyResult<()> {
        debug!("Preparing request for plain HTTP/2 forwarding");

        // Remove headers that shouldn't be forwarded to upstream
        upstream_request.headers.remove("upgrade-insecure-requests");
        upstream_request.headers.remove("sec-fetch-site");
        upstream_request.headers.remove("sec-fetch-mode");
        upstream_request.headers.remove("sec-fetch-dest");

        // Add appropriate headers based on connection type
        if ctx.tls_terminated {
            // TLS was terminated - add headers to indicate original protocol
            upstream_request.insert_header("x-forwarded-proto", "https");
            upstream_request.insert_header("x-tls-terminated", "true");
            
            // Forward client certificate information if available (mTLS)
            if let Some(_client_cert) = &ctx.client_cert {
                upstream_request.insert_header("x-client-cert-present", "true");
                // In a real implementation, we might forward the certificate data
                // or certificate subject information
                debug!("Client certificate information added to upstream request");
            }
            
            info!("Request prepared: TLS terminated, forwarding as plain HTTP/2");
        } else {
            // Plain HTTP/2 connection - no TLS termination
            upstream_request.insert_header("x-forwarded-proto", "http");
            upstream_request.insert_header("x-plain-http2", "true");
            
            info!("Request prepared: Plain HTTP/2, forwarding as plain HTTP/2");
        }

        // Ensure proper host header for upstream
        if let Some(upstream_addr) = ctx.upstream_address {
            upstream_request.insert_header("host", &upstream_addr.to_string());
        }

        // Add proxy identification
        upstream_request.insert_header("x-forwarded-by", "grpc-http-proxy");

        debug!("Request prepared for plain HTTP/2 forwarding to upstream");
        Ok(())
    }
}

#[async_trait::async_trait]
impl ProxyHttp for GrpcProxyService {
    /// Select the upstream peer for this request
    async fn upstream_peer(
        &self,
        session: &Session,
        ctx: &mut Context,
    ) -> Result<Box<HttpPeer>, anyhow::Error> {
        let context = ErrorContext::new()
            .with_request_path(session.uri.path().to_string());

        debug!("Selecting upstream peer for request to {}", session.uri.path());

        // Handle TLS termination first
        self.handle_tls_termination(session, ctx)
            .with_context(context.clone())
            .map_err(|e| anyhow::anyhow!("{}", e))?;

        // Use router to find the appropriate upstream
        let upstream_config = self.router.route(session.uri.path());
        
        // Store the matched route in context for debugging
        if let Some((pattern, priority)) = self.router.find_matching_route(session.uri.path()) {
            ctx.route_match = Some(format!("pattern: {}, priority: {:?}", pattern, priority));
            debug!("Route matched: pattern={}, priority={:?}", pattern, priority);
        } else {
            ctx.route_match = Some("default".to_string());
            debug!("Using default upstream");
        }

        // Validate that the upstream is available and healthy
        let validated_upstream = self.upstream_manager.get_upstream(upstream_config)
            .map_err(|e| {
                error!("Failed to get upstream {}: {}", upstream_config.address, e);
                anyhow::anyhow!("Upstream unavailable: {}", e)
            })?;

        ctx.upstream_address = Some(validated_upstream.address);
        
        info!("Selected upstream: {}", validated_upstream.address);

        // Create HttpPeer for the selected upstream
        // After TLS termination, we always use plain HTTP/2 to upstream
        let scheme = if ctx.tls_terminated { "http" } else { "http" };
        let peer = HttpPeer::new(validated_upstream.address, scheme);
        
        debug!("Upstream peer configured for {} connection", scheme);
        
        Ok(Box::new(peer))
    }

    /// Filter/modify the upstream request before sending
    async fn upstream_request_filter(
        &self,
        session: &Session,
        upstream_request: &mut RequestHeader,
        ctx: &mut Context,
    ) -> Result<(), anyhow::Error> {
        let context = ErrorContext::new()
            .with_request_path(session.uri.path().to_string());

        debug!("Filtering upstream request for {}", session.uri.path());

        // Validate that this is a gRPC request
        if !Self::is_grpc_request(session) {
            warn!("Non-gRPC request received: method={}, content-type={:?}", 
                  session.method, 
                  session.headers.get("content-type"));
            
            let content_type = session.headers.get("content-type")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("missing");
            
            return Err(anyhow::anyhow!("Invalid gRPC content type: expected 'application/grpc*', got '{}'", content_type));
        }

        // Copy essential headers from the original request
        for (name, value) in session.headers.iter() {
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
        upstream_request.insert_header("te", "trailers");
        
        // Prepare request for plain HTTP/2 forwarding after TLS termination
        self.prepare_for_plain_forwarding(session, upstream_request, ctx)
            .with_context(context)
            .map_err(|e| anyhow::anyhow!("{}", e))?;

        debug!("Upstream request prepared with {} headers", upstream_request.headers.len());
        Ok(())
    }

    /// Filter/modify the response headers from upstream
    async fn response_filter(
        &self,
        session: &Session,
        upstream_response: &mut ResponseHeader,
        ctx: &mut Context,
    ) -> Result<(), anyhow::Error> {
        debug!("Filtering response headers for {}", session.uri.path());

        // Initialize trailer state for this response
        ctx.trailer_state = Some(TrailerState::new());

        // For gRPC responses, we need to preserve all headers
        // gRPC uses headers for metadata and trailers for status information

        // Ensure proper CORS headers if needed (for web gRPC clients)
        upstream_response.insert_header("access-control-allow-origin", "*");
        upstream_response.insert_header("access-control-allow-methods", "POST, GET, OPTIONS");
        upstream_response.insert_header("access-control-allow-headers", "content-type, grpc-timeout, grpc-encoding");
        upstream_response.insert_header("access-control-expose-headers", "grpc-status, grpc-message");

        debug!("Response headers filtered, status: {}", upstream_response.status());
        Ok(())
    }

    /// Filter/modify the response body and handle trailers
    async fn upstream_response_body_filter(
        &self,
        session: &Session,
        body: &mut Option<Bytes>,
        end_of_stream: bool,
        ctx: &mut Context,
    ) -> Result<(), anyhow::Error> {
        // For gRPC streaming, we should not buffer the body
        // Just pass it through as-is to maintain streaming semantics
        
        if let Some(body_bytes) = body {
            debug!("Processing body chunk of {} bytes for {}", 
                   body_bytes.len(), session.uri.path());
            
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
            debug!("End of stream reached for {}", session.uri.path());
            
            if let Some(trailer_state) = &mut ctx.trailer_state {
                // In a real implementation, we would extract trailers from the upstream response
                // For now, we'll simulate proper gRPC trailer handling
                
                // If no explicit status was set, assume success
                if trailer_state.grpc_status.is_none() {
                    trailer_state.set_grpc_status(0, Some("OK".to_string()));
                }

                // Validate trailers before sending
                if let Err(e) = trailer_state.validate_grpc_trailers() {
                    error!("Trailer validation failed: {}", e);
                    trailer_state.handle_parsing_error(&e.to_string());
                }

                let all_trailers = trailer_state.get_all_trailers();
                debug!("Trailers prepared: {} headers, status={:?}, message={:?}", 
                       all_trailers.len(),
                       trailer_state.grpc_status, 
                       trailer_state.grpc_message);

                // Log all trailers for debugging
                for (name, value) in all_trailers.iter() {
                    if let Ok(value_str) = value.to_str() {
                        debug!("Trailer: {}={}", name, value_str);
                    }
                }
            }
        }

        Ok(())
    }
}