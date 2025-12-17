//! Comprehensive error handling for the gRPC HTTP proxy
//! 
//! This module provides structured error types for different failure scenarios
//! throughout the application, with proper error propagation and context information.

use std::fmt;
use std::net::SocketAddr;
use thiserror::Error;

/// Main error type for the gRPC HTTP proxy
#[derive(Error, Debug)]
pub enum ProxyError {
    /// Configuration-related errors
    #[error("Configuration error: {0}")]
    Config(#[from] ConfigError),

    /// TLS-related errors
    #[error("TLS error: {0}")]
    Tls(#[from] TlsError),

    /// Routing-related errors
    #[error("Routing error: {0}")]
    Routing(#[from] RoutingError),

    /// Upstream server errors
    #[error("Upstream error: {0}")]
    Upstream(#[from] UpstreamError),

    /// gRPC protocol errors
    #[error("gRPC protocol error: {0}")]
    GrpcProtocol(#[from] GrpcProtocolError),

    /// Network-related errors
    #[error("Network error: {0}")]
    Network(#[from] NetworkError),

    /// Server lifecycle errors
    #[error("Server error: {0}")]
    Server(#[from] ServerError),

    /// Internal proxy errors
    #[error("Internal proxy error: {0}")]
    Internal(#[from] InternalError),
}

/// Configuration-related errors (Requirements 5.1, 5.4)
#[derive(Error, Debug)]
pub enum ConfigError {
    /// Configuration file not found
    #[error("Configuration file not found: {path}")]
    FileNotFound { path: String },

    /// Configuration file cannot be read
    #[error("Cannot read configuration file '{path}': {source}")]
    FileReadError { path: String, source: std::io::Error },

    /// Configuration file has invalid format
    #[error("Invalid configuration format in '{path}': {reason}")]
    InvalidFormat { path: String, reason: String },

    /// Configuration validation failed
    #[error("Configuration validation failed: {field} - {reason}")]
    ValidationFailed { field: String, reason: String },

    /// Invalid route pattern
    #[error("Invalid route pattern '{pattern}': {reason}")]
    InvalidRoutePattern { pattern: String, reason: String },

    /// Conflicting route patterns
    #[error("Conflicting route patterns: '{pattern1}' and '{pattern2}' both match '{path}'")]
    ConflictingRoutes {
        pattern1: String,
        pattern2: String,
        path: String,
    },

    /// Missing required configuration field
    #[error("Missing required configuration field: {field}")]
    MissingField { field: String },

    /// Invalid network address
    #[error("Invalid network address '{address}': {reason}")]
    InvalidAddress { address: String, reason: String },

    /// Invalid timeout value
    #[error("Invalid timeout value '{value}': {reason}")]
    InvalidTimeout { value: String, reason: String },
}

/// TLS-related errors (Requirements 2.1, 2.2, 2.3, 5.2, 5.3)
#[derive(Error, Debug)]
pub enum TlsError {
    /// Certificate file not found
    #[error("Certificate file not found: {path}")]
    CertificateNotFound { path: String },

    /// Certificate file cannot be read
    #[error("Cannot read certificate file '{path}': {source}")]
    CertificateReadError { path: String, source: std::io::Error },

    /// Invalid certificate format
    #[error("Invalid certificate format in '{path}': {reason}")]
    InvalidCertificate { path: String, reason: String },

    /// Certificate has expired
    #[error("Certificate '{path}' has expired: not_after={not_after}")]
    CertificateExpired { path: String, not_after: String },

    /// Certificate is not yet valid
    #[error("Certificate '{path}' is not yet valid: not_before={not_before}")]
    CertificateNotYetValid { path: String, not_before: String },

    /// Private key file not found
    #[error("Private key file not found: {path}")]
    PrivateKeyNotFound { path: String },

    /// Private key file cannot be read
    #[error("Cannot read private key file '{path}': {source}")]
    PrivateKeyReadError { path: String, source: std::io::Error },

    /// Invalid private key format
    #[error("Invalid private key format in '{path}': {reason}")]
    InvalidPrivateKey { path: String, reason: String },

    /// Certificate and private key mismatch
    #[error("Certificate and private key do not match: cert='{cert_path}', key='{key_path}'")]
    CertificateKeyMismatch { cert_path: String, key_path: String },

    /// CA certificate validation failed
    #[error("CA certificate validation failed for '{path}': {reason}")]
    CaValidationFailed { path: String, reason: String },

    /// Client certificate validation failed (mTLS)
    #[error("Client certificate validation failed: {reason}")]
    ClientCertValidationFailed { reason: String },

    /// ALPN negotiation failed
    #[error("ALPN negotiation failed: expected 'h2', got '{actual}'")]
    AlpnNegotiationFailed { actual: String },

    /// TLS handshake failed
    #[error("TLS handshake failed with client {client_addr}: {reason}")]
    HandshakeFailed { client_addr: SocketAddr, reason: String },

    /// TLS configuration error
    #[error("TLS configuration error: {reason}")]
    ConfigurationError { reason: String },
}

/// Routing-related errors (Requirements 3.1, 3.2, 3.3)
#[derive(Error, Debug)]
pub enum RoutingError {
    /// No route found for the given path
    #[error("No route found for path '{path}'")]
    NoRouteFound { path: String },

    /// Multiple routes match with same priority
    #[error("Ambiguous routing: multiple routes match path '{path}' with same priority {priority}")]
    AmbiguousRoute { path: String, priority: u32 },

    /// Route pattern compilation failed
    #[error("Failed to compile route pattern '{pattern}': {reason}")]
    PatternCompilationFailed { pattern: String, reason: String },

    /// Invalid route priority
    #[error("Invalid route priority {priority} for pattern '{pattern}': must be between 0 and 1000")]
    InvalidPriority { pattern: String, priority: u32 },

    /// Route table is empty
    #[error("Route table is empty: no routes configured")]
    EmptyRouteTable,

    /// Default upstream not configured
    #[error("Default upstream not configured and no routes match path '{path}'")]
    NoDefaultUpstream { path: String },
}

/// Upstream server errors (Requirements 3.5)
#[derive(Error, Debug)]
pub enum UpstreamError {
    /// Upstream server is unreachable
    #[error("Upstream server {address} is unreachable: {reason}")]
    Unreachable { address: SocketAddr, reason: String },

    /// Connection to upstream failed
    #[error("Failed to connect to upstream {address}: {source}")]
    ConnectionFailed { address: SocketAddr, source: std::io::Error },

    /// Connection timeout to upstream
    #[error("Connection timeout to upstream {address} after {timeout_ms}ms")]
    ConnectionTimeout { address: SocketAddr, timeout_ms: u64 },

    /// Upstream returned invalid response
    #[error("Upstream {address} returned invalid response: {reason}")]
    InvalidResponse { address: SocketAddr, reason: String },

    /// Upstream health check failed
    #[error("Health check failed for upstream {address}: status={status}, body='{body}'")]
    HealthCheckFailed {
        address: SocketAddr,
        status: u16,
        body: String,
    },

    /// Connection pool exhausted
    #[error("Connection pool exhausted for upstream {address}: {current}/{max} connections")]
    PoolExhausted {
        address: SocketAddr,
        current: usize,
        max: usize,
    },

    /// No healthy upstreams available
    #[error("No healthy upstreams available for load balancing")]
    NoHealthyUpstreams,

    /// Upstream configuration error
    #[error("Upstream configuration error for {address}: {reason}")]
    ConfigurationError { address: SocketAddr, reason: String },
}

/// gRPC protocol errors (Requirements 1.1, 1.2, 1.3, 1.4, 1.5)
#[derive(Error, Debug)]
pub enum GrpcProtocolError {
    /// Invalid gRPC request format
    #[error("Invalid gRPC request: {reason}")]
    InvalidRequest { reason: String },

    /// Invalid HTTP/2 version
    #[error("Invalid HTTP version for gRPC: expected HTTP/2, got {version:?}")]
    InvalidHttpVersion { version: http::Version },

    /// Missing required gRPC headers
    #[error("Missing required gRPC header: {header}")]
    MissingHeader { header: String },

    /// Invalid gRPC content type
    #[error("Invalid gRPC content type: expected 'application/grpc*', got '{content_type}'")]
    InvalidContentType { content_type: String },

    /// gRPC trailer parsing failed
    #[error("Failed to parse gRPC trailers: {reason}")]
    TrailerParsingFailed { reason: String },

    /// gRPC status code missing
    #[error("gRPC status code missing in trailers")]
    MissingGrpcStatus,

    /// Invalid gRPC status code
    #[error("Invalid gRPC status code: {status}")]
    InvalidGrpcStatus { status: i32 },

    /// gRPC frame parsing error
    #[error("Failed to parse gRPC frame: {reason}")]
    FrameParsingError { reason: String },

    /// Streaming error
    #[error("gRPC streaming error: {reason}")]
    StreamingError { reason: String },

    /// Protocol violation
    #[error("gRPC protocol violation: {reason}")]
    ProtocolViolation { reason: String },
}

/// Network-related errors
#[derive(Error, Debug)]
pub enum NetworkError {
    /// Failed to bind to address
    #[error("Failed to bind to address {address}: {source}")]
    BindFailed { address: SocketAddr, source: std::io::Error },

    /// Network I/O error
    #[error("Network I/O error: {0}")]
    IoError(#[from] std::io::Error),

    /// DNS resolution failed
    #[error("DNS resolution failed for '{hostname}': {reason}")]
    DnsResolutionFailed { hostname: String, reason: String },

    /// Invalid network address format
    #[error("Invalid network address format: '{address}'")]
    InvalidAddressFormat { address: String },

    /// Connection reset by peer
    #[error("Connection reset by peer: {peer_addr}")]
    ConnectionReset { peer_addr: SocketAddr },

    /// Network timeout
    #[error("Network timeout after {timeout_ms}ms")]
    Timeout { timeout_ms: u64 },
}

/// Server lifecycle errors
#[derive(Error, Debug)]
pub enum ServerError {
    /// Server initialization failed
    #[error("Server initialization failed: {reason}")]
    InitializationFailed { reason: String },

    /// Server startup failed
    #[error("Server startup failed: {reason}")]
    StartupFailed { reason: String },

    /// Server shutdown error
    #[error("Server shutdown error: {reason}")]
    ShutdownError { reason: String },

    /// Server is already running
    #[error("Server is already running on {address}")]
    AlreadyRunning { address: SocketAddr },

    /// Server is not running
    #[error("Server is not running")]
    NotRunning,

    /// Worker thread panic
    #[error("Worker thread panicked: {reason}")]
    WorkerPanic { reason: String },

    /// Resource exhaustion
    #[error("Server resource exhausted: {resource} - {reason}")]
    ResourceExhausted { resource: String, reason: String },
}

/// Internal proxy errors
#[derive(Error, Debug)]
pub enum InternalError {
    /// Unexpected state
    #[error("Unexpected internal state: {reason}")]
    UnexpectedState { reason: String },

    /// Memory allocation failed
    #[error("Memory allocation failed: {reason}")]
    MemoryAllocationFailed { reason: String },

    /// Thread synchronization error
    #[error("Thread synchronization error: {reason}")]
    SynchronizationError { reason: String },

    /// Serialization/deserialization error
    #[error("Serialization error: {reason}")]
    SerializationError { reason: String },

    /// Feature not implemented
    #[error("Feature not implemented: {feature}")]
    NotImplemented { feature: String },

    /// Assertion failed
    #[error("Internal assertion failed: {assertion} - {context}")]
    AssertionFailed { assertion: String, context: String },
}

/// Result type alias for proxy operations
pub type ProxyResult<T> = Result<T, ProxyError>;

/// Context information for error reporting and debugging
#[derive(Debug, Clone)]
pub struct ErrorContext {
    /// Request ID for tracing
    pub request_id: Option<String>,
    /// Client address
    pub client_addr: Option<SocketAddr>,
    /// Request path
    pub request_path: Option<String>,
    /// Upstream address
    pub upstream_addr: Option<SocketAddr>,
    /// Additional context fields
    pub additional: std::collections::HashMap<String, String>,
}

impl ErrorContext {
    /// Create a new empty error context
    pub fn new() -> Self {
        Self {
            request_id: None,
            client_addr: None,
            request_path: None,
            upstream_addr: None,
            additional: std::collections::HashMap::new(),
        }
    }

    /// Set request ID
    pub fn with_request_id(mut self, request_id: String) -> Self {
        self.request_id = Some(request_id);
        self
    }

    /// Set client address
    pub fn with_client_addr(mut self, client_addr: SocketAddr) -> Self {
        self.client_addr = Some(client_addr);
        self
    }

    /// Set request path
    pub fn with_request_path(mut self, request_path: String) -> Self {
        self.request_path = Some(request_path);
        self
    }

    /// Set upstream address
    pub fn with_upstream_addr(mut self, upstream_addr: SocketAddr) -> Self {
        self.upstream_addr = Some(upstream_addr);
        self
    }

    /// Add additional context field
    pub fn with_context(mut self, key: String, value: String) -> Self {
        self.additional.insert(key, value);
        self
    }
}

impl Default for ErrorContext {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for ErrorContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut parts = Vec::new();

        if let Some(request_id) = &self.request_id {
            parts.push(format!("request_id={}", request_id));
        }
        if let Some(client_addr) = &self.client_addr {
            parts.push(format!("client_addr={}", client_addr));
        }
        if let Some(request_path) = &self.request_path {
            parts.push(format!("request_path={}", request_path));
        }
        if let Some(upstream_addr) = &self.upstream_addr {
            parts.push(format!("upstream_addr={}", upstream_addr));
        }

        for (key, value) in &self.additional {
            parts.push(format!("{}={}", key, value));
        }

        write!(f, "[{}]", parts.join(", "))
    }
}

/// Extension trait for adding context to errors
pub trait ErrorContextExt<T> {
    /// Add context information to an error
    fn with_context(self, context: ErrorContext) -> Result<T, ProxyError>;
    
    /// Add context information with a closure
    fn with_context_fn<F>(self, f: F) -> Result<T, ProxyError>
    where
        F: FnOnce() -> ErrorContext;
}

impl<T, E> ErrorContextExt<T> for Result<T, E>
where
    E: Into<ProxyError>,
{
    fn with_context(self, context: ErrorContext) -> Result<T, ProxyError> {
        self.map_err(|e| {
            let error = e.into();
            // Add context information to the error
            // In a real implementation, we might store context in the error
            tracing::error!("Error with context {}: {}", context, error);
            error
        })
    }

    fn with_context_fn<F>(self, f: F) -> Result<T, ProxyError>
    where
        F: FnOnce() -> ErrorContext,
    {
        self.with_context(f())
    }
}

/// Utility functions for error handling
pub mod utils {
    use super::*;
    use std::net::SocketAddr;

    /// Convert anyhow::Error to ProxyError with context
    pub fn anyhow_to_proxy_error(err: anyhow::Error, context: ErrorContext) -> ProxyError {
        tracing::error!("Converting anyhow error with context {}: {}", context, err);
        
        // Try to downcast to specific error types
        if let Some(io_err) = err.downcast_ref::<std::io::Error>() {
            return ProxyError::Network(NetworkError::IoError(
                std::io::Error::new(io_err.kind(), format!("{} (context: {})", io_err, context))
            ));
        }

        // Default to internal error
        ProxyError::Internal(InternalError::UnexpectedState {
            reason: format!("{} (context: {})", err, context),
        })
    }

    /// Create a configuration error for missing field
    pub fn missing_config_field(field: &str) -> ConfigError {
        ConfigError::MissingField {
            field: field.to_string(),
        }
    }

    /// Create a configuration error for invalid format
    pub fn invalid_config_format(path: &str, reason: &str) -> ConfigError {
        ConfigError::InvalidFormat {
            path: path.to_string(),
            reason: reason.to_string(),
        }
    }

    /// Create an upstream unreachable error
    pub fn upstream_unreachable(address: SocketAddr, reason: &str) -> UpstreamError {
        UpstreamError::Unreachable {
            address,
            reason: reason.to_string(),
        }
    }

    /// Create a gRPC protocol error for invalid request
    pub fn invalid_grpc_request(reason: &str) -> GrpcProtocolError {
        GrpcProtocolError::InvalidRequest {
            reason: reason.to_string(),
        }
    }

    /// Create a TLS error for certificate issues
    pub fn certificate_error(path: &str, reason: &str) -> TlsError {
        TlsError::InvalidCertificate {
            path: path.to_string(),
            reason: reason.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_context_creation() {
        let context = ErrorContext::new()
            .with_request_id("req-123".to_string())
            .with_client_addr("127.0.0.1:8080".parse().unwrap())
            .with_request_path("/api/v1/test".to_string())
            .with_context("custom".to_string(), "value".to_string());

        assert_eq!(context.request_id, Some("req-123".to_string()));
        assert_eq!(context.client_addr, Some("127.0.0.1:8080".parse().unwrap()));
        assert_eq!(context.request_path, Some("/api/v1/test".to_string()));
        assert_eq!(context.additional.get("custom"), Some(&"value".to_string()));
    }

    #[test]
    fn test_error_context_display() {
        let context = ErrorContext::new()
            .with_request_id("req-123".to_string())
            .with_client_addr("127.0.0.1:8080".parse().unwrap());

        let display = format!("{}", context);
        assert!(display.contains("request_id=req-123"));
        assert!(display.contains("client_addr=127.0.0.1:8080"));
    }

    #[test]
    fn test_config_error_creation() {
        let error = ConfigError::FileNotFound {
            path: "/path/to/config.yaml".to_string(),
        };
        assert!(error.to_string().contains("Configuration file not found"));
        assert!(error.to_string().contains("/path/to/config.yaml"));
    }

    #[test]
    fn test_upstream_error_creation() {
        let addr: SocketAddr = "127.0.0.1:9000".parse().unwrap();
        let error = UpstreamError::Unreachable {
            address: addr,
            reason: "Connection refused".to_string(),
        };
        assert!(error.to_string().contains("127.0.0.1:9000"));
        assert!(error.to_string().contains("Connection refused"));
    }

    #[test]
    fn test_grpc_protocol_error_creation() {
        let error = GrpcProtocolError::InvalidHttpVersion {
            version: http::Version::HTTP_11,
        };
        assert!(error.to_string().contains("Invalid HTTP version"));
        assert!(error.to_string().contains("HTTP/2"));
    }

    #[test]
    fn test_error_conversion() {
        let config_error = ConfigError::FileNotFound {
            path: "test.yaml".to_string(),
        };
        let proxy_error: ProxyError = config_error.into();
        assert!(matches!(proxy_error, ProxyError::Config(_)));
    }

    #[test]
    fn test_utility_functions() {
        let error = utils::missing_config_field("server.bind_address");
        assert!(error.to_string().contains("Missing required configuration field"));
        assert!(error.to_string().contains("server.bind_address"));

        let addr: SocketAddr = "127.0.0.1:9000".parse().unwrap();
        let upstream_error = utils::upstream_unreachable(addr, "Connection timeout");
        assert!(upstream_error.to_string().contains("127.0.0.1:9000"));
        assert!(upstream_error.to_string().contains("Connection timeout"));
    }
}