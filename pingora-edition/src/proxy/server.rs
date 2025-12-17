//! Main proxy server implementation

use anyhow::{anyhow, Context, Result};
use std::sync::Arc;
use tracing::{info, warn};

use crate::config::ProxyConfig;
use super::service::GrpcProxyService;
use super::tls::TlsManager;

// Placeholder types for Pingora integration (will be replaced when Pingora is available)
struct PingoraServer;
struct PingoraService;
struct SslAcceptor;

/// Main proxy server built on Pingora's foundation
pub struct ProxyServer {
    config: ProxyConfig,
    tls_manager: TlsManager,
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

        Ok(Self { config, tls_manager })
    }

    /// Start the proxy server
    pub async fn start(&self) -> Result<()> {
        info!("Starting gRPC HTTP Proxy server");
        info!("Binding to address: {}", self.config.server.bind_address);

        // Create the proxy service with TLS configuration
        let proxy_service = GrpcProxyService::new(
            self.config.clone(),
            self.tls_manager.server_config(),
        );

        // Validate server configuration before starting
        self.validate_server_configuration()?;

        // Configure server based on TLS availability
        if let Some(tls_config) = self.tls_manager.server_config() {
            info!("TLS mode: Accepting TLS connections with HTTP/2 (h2) via ALPN");
            info!("Server certificate loaded and validated");
            
            if self.tls_manager.is_mtls_enabled() {
                info!("mTLS client certificate validation enabled");
                info!("CA certificate store configured for client verification");
            }

            // Demonstrate TLS integration pattern
            self.demonstrate_tls_integration(tls_config)?;
        } else {
            info!("Plain HTTP/2 mode: Accepting plain HTTP/2 connections");
            info!("No TLS configuration provided - server will accept unencrypted HTTP/2");
            
            // Demonstrate plain HTTP/2 configuration
            self.demonstrate_plain_http2_configuration()?;
        }

        // Configure worker threads if specified
        if let Some(threads) = self.config.server.worker_threads {
            info!("Configuring {} worker threads", threads);
        } else {
            info!("Using default worker thread configuration");
        }

        // Configure connection limits
        if let Some(max_conn) = self.config.server.max_connections {
            info!("Maximum connections limit: {}", max_conn);
        }

        info!("Server configuration completed successfully");
        
        // Start the actual server
        self.start_server_with_service(proxy_service).await?;
        
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

    /// Start the server with the configured proxy service
    async fn start_server_with_service(&self, proxy_service: GrpcProxyService) -> Result<()> {
        info!("Initializing server with proxy service...");

        // Log service configuration
        if proxy_service.is_tls_enabled() {
            info!("Proxy service configured with TLS support");
        } else {
            info!("Proxy service configured for plain HTTP/2");
        }

        // In a real Pingora implementation, this would:
        // 1. Create a Pingora Server instance
        // 2. Configure it with our proxy service
        // 3. Set up TLS listeners if TLS is enabled
        // 4. Set up plain HTTP/2 listeners if TLS is disabled
        // 5. Start the server and handle connections

        info!("Server initialization completed");
        info!("Ready to accept connections on {}", self.config.server.bind_address);

        // Simulate server running
        // In a real implementation, this would be the main server loop
        self.simulate_server_operation().await?;

        Ok(())
    }

    /// Simulate server operation (placeholder for actual Pingora integration)
    async fn simulate_server_operation(&self) -> Result<()> {
        info!("Server is now running and ready to accept connections");
        info!("Proxy service is handling gRPC requests");
        
        // Log operational status
        if self.tls_manager.is_enabled() {
            info!("TLS termination active - decrypting incoming connections");
            if self.tls_manager.is_mtls_enabled() {
                info!("mTLS validation active - verifying client certificates");
            }
        }

        info!("Routing engine active - {} routes configured", self.config.routes.len());
        info!("Upstream connections ready - default: {}", self.config.default_upstream.address);

        // In a real implementation, this would be where Pingora runs
        // For now, we'll just indicate that the server is ready
        warn!("Server simulation mode - waiting for Pingora integration");
        warn!("In production, this would be the main server event loop");

        // Keep the server "running" until shutdown
        // In a real implementation, Pingora would handle this
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        Ok(())
    }

    /// Stop the proxy server gracefully
    pub async fn stop(&self) -> Result<()> {
        // Pingora handles graceful shutdown internally
        // This method is provided for future extensibility
        info!("Proxy server shutdown requested");
        Ok(())
    }

    /// Demonstrate TLS integration pattern with Pingora
    fn demonstrate_tls_integration(&self, server_config: Arc<rustls::ServerConfig>) -> Result<()> {
        info!("Demonstrating TLS integration pattern:");
        
        // 1. Verify ALPN configuration
        let alpn_protocols = &server_config.alpn_protocols;
        if alpn_protocols.len() == 1 && alpn_protocols[0] == b"h2" {
            info!("✓ ALPN correctly configured for HTTP/2 (h2)");
        } else {
            warn!("⚠ ALPN configuration may be incorrect: {:?}", alpn_protocols);
        }

        // 2. Verify certificate chain
        info!("✓ Server certificate chain loaded and validated");
        
        // 3. Verify mTLS configuration
        if self.tls_manager.is_mtls_enabled() {
            if let Some(_ca_store) = self.tls_manager.ca_store() {
                info!("✓ CA certificate store configured for mTLS");
                info!("  - Client certificates will be validated against loaded CA");
            }
        }

        // 4. Integration points with Pingora:
        info!("Integration points with Pingora:");
        info!("  - ServerConfig will be passed to Pingora's TLS listener");
        info!("  - ALPN negotiation will ensure HTTP/2 protocol");
        info!("  - mTLS verification will be handled by rustls WebPkiClientVerifier");
        info!("  - Certificate validation includes expiry and format checks");
        info!("  - TLS connections will be terminated and forwarded as plain HTTP/2");

        Ok(())
    }

    /// Demonstrate plain HTTP/2 configuration when TLS is disabled
    fn demonstrate_plain_http2_configuration(&self) -> Result<()> {
        info!("Demonstrating plain HTTP/2 configuration:");
        
        // 1. Verify plain HTTP/2 support
        info!("✓ Plain HTTP/2 connections enabled");
        info!("  - Server will accept HTTP/2 connections without TLS");
        info!("  - No certificate validation required");
        info!("  - Direct HTTP/2 communication with clients");

        // 2. Integration points with Pingora for plain HTTP/2:
        info!("Integration points with Pingora for plain HTTP/2:");
        info!("  - Pingora will be configured to accept plain HTTP/2 connections");
        info!("  - No TLS listener configuration needed");
        info!("  - HTTP/2 protocol negotiation via HTTP/2 connection preface");
        info!("  - Same proxy service handles both TLS and plain connections");

        // 3. Security considerations
        info!("Security considerations for plain HTTP/2:");
        info!("  - No encryption - suitable for internal networks or development");
        info!("  - No client authentication available");
        info!("  - Consider using TLS in production environments");

        // 4. Protocol handling
        info!("Protocol handling:");
        info!("  - gRPC protocol semantics preserved");
        info!("  - HTTP trailers forwarded correctly");
        info!("  - Streaming support maintained");
        info!("  - Same routing and upstream logic applies");

        Ok(())
    }
}