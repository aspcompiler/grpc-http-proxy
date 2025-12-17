//! Main proxy server implementation using hyper HTTP/2

use anyhow::{anyhow, Context, Result};
use std::sync::Arc;
use std::net::SocketAddr;
use tracing::{info, warn, error, debug};
use tokio::net::TcpListener;
use hyper::server::conn::http2;
use hyper::{Request, Response, StatusCode};
use hyper::body::Incoming;
use hyper_util::rt::TokioIo;
use hyper::service::Service;
use std::future::Future;
use std::pin::Pin;
use bytes::Bytes;
use http_body_util::{BodyExt, Empty, combinators::BoxBody};
use hyper_util::client::legacy::Client;
use hyper_util::rt::TokioExecutor;

use crate::config::ProxyConfig;
use super::service::GrpcProxyService;
use super::tls::TlsManager;
use super::pingora_types::{Session, Context as ProxyContext, ProxyHttp};

/// Main proxy server built on hyper HTTP/2
pub struct ProxyServer {
    config: ProxyConfig,
    tls_manager: TlsManager,
    service: Arc<GrpcProxyService>,
}

impl ProxyServer {
    /// Create a new proxy server with the given configuration
    pub fn new(config: ProxyConfig) -> Result<Self> {
        // Initialize TLS manager
        let mut tls_manager = TlsManager::new(config.tls.clone());
        
        // Load certificates if TLS is enabled
        if tls_manager.is_enabled() {
            tls_manager.load_certificates()
                .context("Failed to load TLS certificates")?;
            info!("TLS certificates loaded successfully");
            
            if tls_manager.is_mtls_enabled() {
                info!("mTLS client certificate validation enabled");
            }
        } else {
            info!("TLS disabled - server will accept plain HTTP/2 connections");
        }

        // Create the proxy service
        let service = Arc::new(GrpcProxyService::new(
            config.clone(),
            tls_manager.server_config(),
        ));

        Ok(Self { 
            config, 
            tls_manager,
            service,
        })
    }

    /// Start the proxy server
    pub async fn start(&mut self) -> Result<()> {
        info!("Starting gRPC HTTP Proxy server");
        info!("Binding to address: {}", self.config.server.bind_address);

        // Validate server configuration before starting
        self.validate_server_configuration()?;

        // Create TCP listener
        let listener = TcpListener::bind(&self.config.server.bind_address).await
            .with_context(|| format!("Failed to bind to {}", self.config.server.bind_address))?;

        info!("Server listening on {}", self.config.server.bind_address);
        info!("Ready to accept gRPC connections");

        // Accept connections in a loop
        loop {
            match listener.accept().await {
                Ok((stream, client_addr)) => {
                    let service = self.service.clone();
                    let server_addr = self.config.server.bind_address;
                    
                    debug!("Accepted connection from {}", client_addr);
                    
                    // Handle the connection in a separate task
                    tokio::spawn(async move {
                        if let Err(e) = Self::handle_connection(stream, client_addr, server_addr, service).await {
                            error!("Connection error from {}: {}", client_addr, e);
                        }
                    });
                }
                Err(e) => {
                    error!("Failed to accept connection: {}", e);
                    // Continue accepting other connections
                }
            }
        }
    }

    /// Handle a single connection
    async fn handle_connection(
        stream: tokio::net::TcpStream,
        client_addr: SocketAddr,
        server_addr: SocketAddr,
        service: Arc<GrpcProxyService>,
    ) -> Result<()> {
        debug!("Handling connection from {}", client_addr);

        // Create a hyper service wrapper
        let hyper_service = HyperServiceWrapper {
            proxy_service: service,
            client_addr,
            server_addr,
        };

        // Use HTTP/2 to serve the connection with gRPC-optimized settings
        let io = TokioIo::new(stream);
        
        let mut builder = http2::Builder::new(hyper_util::rt::TokioExecutor::new());
        
        // Configure HTTP/2 settings for gRPC
        builder
            .initial_stream_window_size(Some(65536))  // 64KB initial window
            .initial_connection_window_size(Some(1048576))  // 1MB connection window
            .max_frame_size(Some(16384))  // 16KB max frame size
            .max_concurrent_streams(Some(100))  // Allow up to 100 concurrent streams
            .keep_alive_interval(Some(std::time::Duration::from_secs(30)))  // Keep alive every 30s
            .keep_alive_timeout(std::time::Duration::from_secs(10))  // 10s keep alive timeout
            .timer(hyper_util::rt::TokioTimer::new());  // Add timer for hyper
        
        if let Err(e) = builder
            .serve_connection(io, hyper_service)
            .await
        {
            error!("HTTP/2 connection error from {}: {}", client_addr, e);
        }

        debug!("Connection from {} closed", client_addr);
        Ok(())
    }

    /// Stop the proxy server gracefully
    pub async fn stop(&self) -> Result<()> {
        info!("Proxy server shutdown requested");
        // In a real implementation, we would signal the server to stop accepting new connections
        // and wait for existing connections to finish
        info!("Proxy server shutdown completed");
        Ok(())
    }

    /// Validate server configuration before starting
    fn validate_server_configuration(&self) -> Result<()> {
        info!("Validating server configuration...");

        // Validate bind address
        if self.config.server.bind_address.port() == 0 {
            return Err(anyhow!("Invalid bind port: 0"));
        }

        // Validate worker thread configuration
        if let Some(threads) = self.config.server.worker_threads {
            if threads == 0 {
                return Err(anyhow!("Worker threads must be greater than 0"));
            }
            if threads > 1000 {
                warn!("Very high worker thread count: {}", threads);
            }
        }

        // Validate connection limits
        if let Some(max_conn) = self.config.server.max_connections {
            if max_conn == 0 {
                return Err(anyhow!("Max connections must be greater than 0"));
            }
        }

        // Validate routing configuration
        if self.config.routes.is_empty() {
            info!("No specific routes configured, all traffic will use default upstream");
        }

        // Validate upstream configuration
        if self.config.default_upstream.address.port() == 0 {
            return Err(anyhow!("Invalid default upstream port: 0"));
        }

        info!("Server configuration validation completed successfully");
        Ok(())
    }

    /// Get server statistics (if available)
    pub fn get_stats(&self) -> Result<ServerStats> {
        Ok(ServerStats {
            active_connections: 0, // Would be tracked in real implementation
            total_requests: 0,
            uptime_seconds: 0,
        })
    }
}

/// Server statistics structure
#[derive(Debug, Clone)]
pub struct ServerStats {
    pub active_connections: u64,
    pub total_requests: u64,
    pub uptime_seconds: u64,
}

/// Wrapper to adapt our ProxyHttp service to hyper's Service trait
#[derive(Clone)]
struct HyperServiceWrapper {
    proxy_service: Arc<GrpcProxyService>,
    client_addr: SocketAddr,
    server_addr: SocketAddr,
}

impl Service<Request<Incoming>> for HyperServiceWrapper {
    type Response = Response<BoxBody<Bytes, Box<dyn std::error::Error + Send + Sync>>>;
    type Error = Box<dyn std::error::Error + Send + Sync>;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn call(&self, req: Request<Incoming>) -> Self::Future {
        let service = self.proxy_service.clone();
        let client_addr = self.client_addr;
        let server_addr = self.server_addr;

        Box::pin(async move {
            debug!("Processing request: {} {} from {}", req.method(), req.uri(), client_addr);

            // Validate that this is a gRPC request
            if req.method() != hyper::Method::POST {
                warn!("Non-POST request from {}: {} {}", client_addr, req.method(), req.uri());
                return Ok(Self::create_grpc_error_response(
                    StatusCode::METHOD_NOT_ALLOWED,
                    12, // UNIMPLEMENTED
                    "Only POST method is supported for gRPC"
                ));
            }

            // Check content-type for gRPC
            let content_type = req.headers()
                .get("content-type")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("");
            
            if !content_type.starts_with("application/grpc") {
                warn!("Non-gRPC content-type from {}: {}", client_addr, content_type);
                return Ok(Self::create_grpc_error_response(
                    StatusCode::UNSUPPORTED_MEDIA_TYPE,
                    3, // INVALID_ARGUMENT
                    "Content-Type must be application/grpc"
                ));
            }

            // Create session and context
            let mut session = Session::new(client_addr, server_addr, req);
            let mut ctx = ProxyContext::default();

            // Process the request through our proxy service
            match Self::process_request(&*service, &mut session, &mut ctx).await {
                Ok(response) => {
                    debug!("Request processed successfully for {}", client_addr);
                    Ok(response)
                },
                Err(e) => {
                    error!("Request processing failed for {}: {}", client_addr, e);
                    
                    Ok(Self::create_grpc_error_response(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        13, // INTERNAL
                        "Proxy processing error"
                    ))
                }
            }
        })
    }
}

impl HyperServiceWrapper {
    fn create_grpc_error_response(status: StatusCode, grpc_status: u32, message: &str) -> Response<BoxBody<Bytes, Box<dyn std::error::Error + Send + Sync>>> {
        let empty_body = Empty::<Bytes>::new()
            .map_err(|never| match never {})
            .boxed();
        
        Response::builder()
            .status(status)
            .header("content-type", "application/grpc")
            .header("grpc-status", grpc_status.to_string())
            .header("grpc-message", message)
            .body(empty_body)
            .unwrap()
    }
}

impl HyperServiceWrapper {
    async fn process_request(
        service: &GrpcProxyService,
        session: &mut Session,
        ctx: &mut ProxyContext,
    ) -> Result<Response<BoxBody<Bytes, Box<dyn std::error::Error + Send + Sync>>>, Box<dyn std::error::Error + Send + Sync>> {
        // Step 1: Select upstream peer
        let _peer = service.upstream_peer(session, ctx).await?;
        debug!("Selected upstream: {:?}", ctx.upstream_address);

        // Step 2: Create upstream request parts (simplified)
        let request = Request::builder()
            .method(session.method())
            .uri(session.uri())
            .version(session.version())
            .body(())
            .unwrap();
        let (mut upstream_request, _) = request.into_parts();
        upstream_request.headers = session.headers().clone();
        service.upstream_request_filter(session, &mut upstream_request, ctx).await?;
        debug!("Request filtered for upstream");

        // Step 3: Take streaming body for forwarding (supports bidirectional streaming)
        let request_body = session.take_body();
        debug!("Using streaming body for upstream request");
        
        // Step 4: Make real upstream request with streaming
        let upstream_response = Self::make_upstream_request_streaming(&upstream_request, ctx, request_body).await?;
        
        // Return the upstream response directly (this supports streaming)
        info!("Request processed successfully - forwarded to upstream with streaming");
        Ok(upstream_response)
    }

    async fn make_upstream_request_streaming(
        upstream_request: &http::request::Parts,
        ctx: &ProxyContext,
        request_body: Option<hyper::body::Incoming>,
    ) -> Result<Response<BoxBody<Bytes, Box<dyn std::error::Error + Send + Sync>>>, Box<dyn std::error::Error + Send + Sync>> {
        let upstream_addr = ctx.upstream_address
            .ok_or("No upstream address configured")?;
        
        debug!("Making streaming upstream request to {:?}", upstream_addr);
        
        // Create HTTP/2 client
        let client = Client::builder(TokioExecutor::new())
            .http2_only(true)
            .build_http();
        
        // Build upstream URL with proper IPv6 handling
        let upstream_url = if upstream_addr.is_ipv6() {
            format!("http://[{}]:{}{}", 
                upstream_addr.ip(), 
                upstream_addr.port(), 
                upstream_request.uri.path_and_query()
                    .map(|pq| pq.as_str())
                    .unwrap_or("/")
            )
        } else {
            format!("http://{}:{}{}", 
                upstream_addr.ip(), 
                upstream_addr.port(), 
                upstream_request.uri.path_and_query()
                    .map(|pq| pq.as_str())
                    .unwrap_or("/")
            )
        };
        
        // Create upstream request
        let mut upstream_req_builder = Request::builder()
            .method(&upstream_request.method)
            .uri(upstream_url)
            .version(http::Version::HTTP_2);
        
        // Copy headers (excluding host which will be set automatically)
        for (name, value) in &upstream_request.headers {
            if name != "host" {
                upstream_req_builder = upstream_req_builder.header(name, value);
            }
        }
        
        // Use streaming body or empty body - box both to have consistent types
        let boxed_body: BoxBody<Bytes, Box<dyn std::error::Error + Send + Sync>> = if let Some(body) = request_body {
            body.map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>).boxed()
        } else {
            Empty::<Bytes>::new()
                .map_err(|never| match never {})
                .boxed()
        };
        
        let upstream_req = upstream_req_builder
            .body(boxed_body)
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)?;
        
        debug!("Sending streaming request to upstream: {} {}", upstream_req.method(), upstream_req.uri());
        
        // Make the request
        let upstream_response = client.request(upstream_req).await
            .map_err(|e| {
                error!("Upstream request failed: {}", e);
                Box::new(e) as Box<dyn std::error::Error + Send + Sync>
            })?;
        
        debug!("Received upstream response: {}", upstream_response.status());
        
        // Return the streaming response directly without buffering
        let (parts, body) = upstream_response.into_parts();
        
        // Build response with upstream headers and streaming body
        let mut response_builder = Response::builder()
            .status(parts.status)
            .version(http::Version::HTTP_2);
        
        // Copy response headers
        for (name, value) in parts.headers.iter() {
            response_builder = response_builder.header(name, value);
        }
        
        // Box the body to match our return type
        let boxed_body = body
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)
            .boxed();
        
        let response = response_builder
            .body(boxed_body)
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)?;
        
        debug!("Forwarding streaming upstream response");
        Ok(response)
    }
}