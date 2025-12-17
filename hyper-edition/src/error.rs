//! Error handling module
//! 
//! Defines comprehensive error types for the proxy server with proper HTTP status code mapping

use thiserror::Error;
use hyper::StatusCode;

/// Main error type for the proxy server
#[derive(Error, Debug)]
pub enum ProxyError {
    // TLS-related errors
    #[error("TLS handshake failed: {message}")]
    TlsHandshake { message: String },

    #[error("TLS certificate error: {message}")]
    TlsCertificate { message: String },

    #[error("TLS configuration error: {message}")]
    TlsConfig { message: String },

    #[error("TLS error: {0}")]
    Tls(String),

    // Network and connection errors
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Connection timeout to {address}")]
    ConnectionTimeout { address: String },

    #[error("Connection refused by {address}")]
    ConnectionRefused { address: String },

    #[error("Connection reset by {address}")]
    ConnectionReset { address: String },

    // HTTP/2 and protocol errors
    #[error("HTTP error: {0}")]
    Http(#[from] hyper::Error),

    #[error("HTTP/2 protocol error: {message}")]
    Http2Protocol { message: String },

    #[error("Invalid HTTP method: {method}")]
    InvalidHttpMethod { method: String },

    #[error("Invalid HTTP headers: {message}")]
    InvalidHttpHeaders { message: String },

    #[error("Protocol error: {0}")]
    ProtocolError(String),

    // gRPC-specific errors
    #[error("Invalid gRPC request: {message}")]
    InvalidGrpcRequest { message: String },

    #[error("gRPC status error: status={status}, message={message}")]
    GrpcStatus { status: u32, message: String },

    #[error("Missing gRPC content-type header")]
    MissingGrpcContentType,

    #[error("Invalid gRPC content-type: {content_type}")]
    InvalidGrpcContentType { content_type: String },

    // Routing errors
    #[error("No route found for path: {path}")]
    NoRouteFound { path: String },

    #[error("Invalid routing pattern: {pattern}, reason: {reason}")]
    InvalidRoutingPattern { pattern: String, reason: String },

    #[error("Routing configuration error: {message}")]
    RoutingConfigError { message: String },

    #[error("Routing error: {0}")]
    RoutingError(String),

    // Upstream server errors
    #[error("Upstream server unavailable: {address}")]
    UpstreamUnavailable { address: String },

    #[error("Upstream timeout: {address}, timeout: {timeout_ms}ms")]
    UpstreamTimeout { address: String, timeout_ms: u64 },

    #[error("Upstream connection pool exhausted for {address}")]
    UpstreamPoolExhausted { address: String },

    #[error("Upstream health check failed for {address}: {reason}")]
    UpstreamHealthCheckFailed { address: String, reason: String },

    #[error("Circuit breaker open for {address}")]
    CircuitBreakerOpen { address: String },

    #[error("Upstream error: {0}")]
    UpstreamError(String),

    // Configuration errors
    #[error("Configuration file not found: {path}")]
    ConfigFileNotFound { path: String },

    #[error("Invalid configuration: {field}, reason: {reason}")]
    InvalidConfig { field: String, reason: String },

    #[error("Configuration validation error: {message}")]
    ConfigValidation { message: String },

    #[error("Configuration error: {0}")]
    ConfigError(String),

    // Serialization and parsing errors
    #[error("YAML parsing error: {0}")]
    Serialization(#[from] serde_yaml::Error),

    #[error("JSON parsing error: {message}")]
    JsonParsing { message: String },

    #[error("Address parse error: {0}")]
    AddrParse(#[from] std::net::AddrParseError),

    #[error("URI parse error: {uri}, reason: {reason}")]
    UriParse { uri: String, reason: String },

    // Resource and capacity errors
    #[error("Resource exhausted: {resource}")]
    ResourceExhausted { resource: String },

    #[error("Rate limit exceeded for {identifier}")]
    RateLimitExceeded { identifier: String },

    #[error("Request too large: {size} bytes, max: {max_size} bytes")]
    RequestTooLarge { size: usize, max_size: usize },

    // Internal server errors
    #[error("Internal server error: {message}")]
    Internal { message: String },

    #[error("Service unavailable: {reason}")]
    ServiceUnavailable { reason: String },

    #[error("Shutdown in progress")]
    ShutdownInProgress,
}

impl ProxyError {
    /// Convert ProxyError to appropriate HTTP status code
    pub fn to_status_code(&self) -> StatusCode {
        match self {
            // Client errors (4xx)
            ProxyError::InvalidGrpcRequest { .. } => StatusCode::BAD_REQUEST,
            ProxyError::MissingGrpcContentType => StatusCode::BAD_REQUEST,
            ProxyError::InvalidGrpcContentType { .. } => StatusCode::BAD_REQUEST,
            ProxyError::InvalidHttpMethod { .. } => StatusCode::METHOD_NOT_ALLOWED,
            ProxyError::InvalidHttpHeaders { .. } => StatusCode::BAD_REQUEST,
            ProxyError::ProtocolError(_) => StatusCode::BAD_REQUEST,
            ProxyError::Http2Protocol { .. } => StatusCode::BAD_REQUEST,
            ProxyError::UriParse { .. } => StatusCode::BAD_REQUEST,
            ProxyError::RequestTooLarge { .. } => StatusCode::PAYLOAD_TOO_LARGE,
            ProxyError::RateLimitExceeded { .. } => StatusCode::TOO_MANY_REQUESTS,

            // Not found errors (404)
            ProxyError::NoRouteFound { .. } => StatusCode::NOT_FOUND,

            // Upstream errors (502)
            ProxyError::UpstreamUnavailable { .. } => StatusCode::BAD_GATEWAY,
            ProxyError::UpstreamTimeout { .. } => StatusCode::GATEWAY_TIMEOUT,
            ProxyError::UpstreamPoolExhausted { .. } => StatusCode::BAD_GATEWAY,
            ProxyError::UpstreamHealthCheckFailed { .. } => StatusCode::BAD_GATEWAY,
            ProxyError::CircuitBreakerOpen { .. } => StatusCode::SERVICE_UNAVAILABLE,
            ProxyError::UpstreamError(_) => StatusCode::BAD_GATEWAY,
            ProxyError::ConnectionTimeout { .. } => StatusCode::GATEWAY_TIMEOUT,
            ProxyError::ConnectionRefused { .. } => StatusCode::BAD_GATEWAY,
            ProxyError::ConnectionReset { .. } => StatusCode::BAD_GATEWAY,

            // Service unavailable (503)
            ProxyError::ServiceUnavailable { .. } => StatusCode::SERVICE_UNAVAILABLE,
            ProxyError::ResourceExhausted { .. } => StatusCode::SERVICE_UNAVAILABLE,
            ProxyError::ShutdownInProgress => StatusCode::SERVICE_UNAVAILABLE,

            // Internal server errors (500)
            ProxyError::TlsHandshake { .. } => StatusCode::INTERNAL_SERVER_ERROR,
            ProxyError::TlsCertificate { .. } => StatusCode::INTERNAL_SERVER_ERROR,
            ProxyError::TlsConfig { .. } => StatusCode::INTERNAL_SERVER_ERROR,
            ProxyError::Tls(_) => StatusCode::INTERNAL_SERVER_ERROR,
            ProxyError::ConfigFileNotFound { .. } => StatusCode::INTERNAL_SERVER_ERROR,
            ProxyError::InvalidConfig { .. } => StatusCode::INTERNAL_SERVER_ERROR,
            ProxyError::ConfigValidation { .. } => StatusCode::INTERNAL_SERVER_ERROR,
            ProxyError::ConfigError(_) => StatusCode::INTERNAL_SERVER_ERROR,
            ProxyError::InvalidRoutingPattern { .. } => StatusCode::INTERNAL_SERVER_ERROR,
            ProxyError::RoutingConfigError { .. } => StatusCode::INTERNAL_SERVER_ERROR,
            ProxyError::RoutingError(_) => StatusCode::INTERNAL_SERVER_ERROR,
            ProxyError::Internal { .. } => StatusCode::INTERNAL_SERVER_ERROR,
            ProxyError::Serialization(_) => StatusCode::INTERNAL_SERVER_ERROR,
            ProxyError::JsonParsing { .. } => StatusCode::INTERNAL_SERVER_ERROR,
            ProxyError::AddrParse(_) => StatusCode::INTERNAL_SERVER_ERROR,
            ProxyError::Io(_) => StatusCode::INTERNAL_SERVER_ERROR,
            ProxyError::Http(_) => StatusCode::INTERNAL_SERVER_ERROR,

            // gRPC status errors map to 200 OK with grpc-status trailer
            ProxyError::GrpcStatus { .. } => StatusCode::OK,
        }
    }

    /// Get error category for logging and monitoring
    pub fn category(&self) -> ErrorCategory {
        match self {
            ProxyError::TlsHandshake { .. } | 
            ProxyError::TlsCertificate { .. } | 
            ProxyError::TlsConfig { .. } | 
            ProxyError::Tls(_) => ErrorCategory::Tls,

            ProxyError::ConnectionTimeout { .. } | 
            ProxyError::ConnectionRefused { .. } | 
            ProxyError::ConnectionReset { .. } | 
            ProxyError::Io(_) => ErrorCategory::Network,

            ProxyError::Http(_) | 
            ProxyError::Http2Protocol { .. } | 
            ProxyError::InvalidHttpMethod { .. } | 
            ProxyError::InvalidHttpHeaders { .. } | 
            ProxyError::ProtocolError(_) => ErrorCategory::Protocol,

            ProxyError::InvalidGrpcRequest { .. } | 
            ProxyError::GrpcStatus { .. } | 
            ProxyError::MissingGrpcContentType | 
            ProxyError::InvalidGrpcContentType { .. } => ErrorCategory::Grpc,

            ProxyError::NoRouteFound { .. } | 
            ProxyError::InvalidRoutingPattern { .. } | 
            ProxyError::RoutingConfigError { .. } | 
            ProxyError::RoutingError(_) => ErrorCategory::Routing,

            ProxyError::UpstreamUnavailable { .. } | 
            ProxyError::UpstreamTimeout { .. } | 
            ProxyError::UpstreamPoolExhausted { .. } | 
            ProxyError::UpstreamHealthCheckFailed { .. } | 
            ProxyError::CircuitBreakerOpen { .. } | 
            ProxyError::UpstreamError(_) => ErrorCategory::Upstream,

            ProxyError::ConfigFileNotFound { .. } | 
            ProxyError::InvalidConfig { .. } | 
            ProxyError::ConfigValidation { .. } | 
            ProxyError::ConfigError(_) => ErrorCategory::Configuration,

            ProxyError::Serialization(_) | 
            ProxyError::JsonParsing { .. } | 
            ProxyError::AddrParse(_) | 
            ProxyError::UriParse { .. } => ErrorCategory::Parsing,

            ProxyError::ResourceExhausted { .. } | 
            ProxyError::RateLimitExceeded { .. } | 
            ProxyError::RequestTooLarge { .. } => ErrorCategory::Resource,

            ProxyError::Internal { .. } | 
            ProxyError::ServiceUnavailable { .. } | 
            ProxyError::ShutdownInProgress => ErrorCategory::Internal,
        }
    }

    /// Check if the error is retryable
    pub fn is_retryable(&self) -> bool {
        match self {
            // Network errors that might be temporary
            ProxyError::ConnectionTimeout { .. } => true,
            ProxyError::ConnectionReset { .. } => true,
            ProxyError::UpstreamTimeout { .. } => true,
            ProxyError::UpstreamUnavailable { .. } => true,
            ProxyError::ResourceExhausted { .. } => true,
            ProxyError::ServiceUnavailable { .. } => true,
            
            // Circuit breaker and health check failures might recover
            ProxyError::CircuitBreakerOpen { .. } => false, // Circuit breaker handles retries
            ProxyError::UpstreamHealthCheckFailed { .. } => false, // Health check will retry
            
            // Client errors are not retryable
            ProxyError::InvalidGrpcRequest { .. } => false,
            ProxyError::MissingGrpcContentType => false,
            ProxyError::InvalidGrpcContentType { .. } => false,
            ProxyError::InvalidHttpMethod { .. } => false,
            ProxyError::InvalidHttpHeaders { .. } => false,
            ProxyError::NoRouteFound { .. } => false,
            ProxyError::RequestTooLarge { .. } => false,
            ProxyError::RateLimitExceeded { .. } => false,
            
            // Configuration and internal errors are not retryable
            ProxyError::ConfigError(_) => false,
            ProxyError::ConfigFileNotFound { .. } => false,
            ProxyError::InvalidConfig { .. } => false,
            ProxyError::ConfigValidation { .. } => false,
            ProxyError::RoutingConfigError { .. } => false,
            ProxyError::InvalidRoutingPattern { .. } => false,
            ProxyError::TlsConfig { .. } => false,
            ProxyError::TlsCertificate { .. } => false,
            
            // Other errors default to not retryable
            _ => false,
        }
    }

    /// Get retry delay for retryable errors
    pub fn retry_delay(&self, attempt: u32) -> Option<std::time::Duration> {
        if !self.is_retryable() {
            return None;
        }

        // Exponential backoff with jitter
        let base_delay_ms = match self {
            ProxyError::ConnectionTimeout { .. } => 100,
            ProxyError::UpstreamTimeout { .. } => 200,
            ProxyError::UpstreamUnavailable { .. } => 500,
            ProxyError::ResourceExhausted { .. } => 1000,
            _ => 100,
        };

        let delay_ms = base_delay_ms * (2_u64.pow(attempt.min(5))); // Cap at 2^5 = 32x
        let jitter = fastrand::u64(0..=delay_ms / 4); // Add up to 25% jitter
        
        Some(std::time::Duration::from_millis(delay_ms + jitter))
    }
}

/// Error categories for monitoring and alerting
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ErrorCategory {
    Tls,
    Network,
    Protocol,
    Grpc,
    Routing,
    Upstream,
    Configuration,
    Parsing,
    Resource,
    Internal,
}

impl ErrorCategory {
    /// Get string representation for logging
    pub fn as_str(&self) -> &'static str {
        match self {
            ErrorCategory::Tls => "tls",
            ErrorCategory::Network => "network",
            ErrorCategory::Protocol => "protocol",
            ErrorCategory::Grpc => "grpc",
            ErrorCategory::Routing => "routing",
            ErrorCategory::Upstream => "upstream",
            ErrorCategory::Configuration => "configuration",
            ErrorCategory::Parsing => "parsing",
            ErrorCategory::Resource => "resource",
            ErrorCategory::Internal => "internal",
        }
    }
}

/// Helper functions for creating specific error types
impl ProxyError {
    /// Create a TLS handshake error
    pub fn tls_handshake<S: Into<String>>(message: S) -> Self {
        ProxyError::TlsHandshake { message: message.into() }
    }

    /// Create a connection timeout error
    pub fn connection_timeout<S: Into<String>>(address: S) -> Self {
        ProxyError::ConnectionTimeout { address: address.into() }
    }

    /// Create an upstream timeout error
    pub fn upstream_timeout<S: Into<String>>(address: S, timeout_ms: u64) -> Self {
        ProxyError::UpstreamTimeout { address: address.into(), timeout_ms }
    }

    /// Create a no route found error
    pub fn no_route_found<S: Into<String>>(path: S) -> Self {
        ProxyError::NoRouteFound { path: path.into() }
    }

    /// Create an invalid gRPC request error
    pub fn invalid_grpc_request<S: Into<String>>(message: S) -> Self {
        ProxyError::InvalidGrpcRequest { message: message.into() }
    }

    /// Create a gRPC status error
    pub fn grpc_status<S: Into<String>>(status: u32, message: S) -> Self {
        ProxyError::GrpcStatus { status, message: message.into() }
    }

    /// Create a circuit breaker open error
    pub fn circuit_breaker_open<S: Into<String>>(address: S) -> Self {
        ProxyError::CircuitBreakerOpen { address: address.into() }
    }

    /// Create an upstream health check failed error
    pub fn upstream_health_check_failed<S: Into<String>>(address: S, reason: S) -> Self {
        ProxyError::UpstreamHealthCheckFailed { 
            address: address.into(), 
            reason: reason.into() 
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hyper::StatusCode;
    use std::time::Duration;

    #[test]
    fn test_error_status_code_mapping() {
        // Test client errors (4xx)
        assert_eq!(
            ProxyError::invalid_grpc_request("bad request").to_status_code(),
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            ProxyError::MissingGrpcContentType.to_status_code(),
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            ProxyError::InvalidHttpMethod { method: "PATCH".to_string() }.to_status_code(),
            StatusCode::METHOD_NOT_ALLOWED
        );
        assert_eq!(
            ProxyError::RequestTooLarge { size: 1000, max_size: 500 }.to_status_code(),
            StatusCode::PAYLOAD_TOO_LARGE
        );
        assert_eq!(
            ProxyError::RateLimitExceeded { identifier: "client1".to_string() }.to_status_code(),
            StatusCode::TOO_MANY_REQUESTS
        );

        // Test not found errors (404)
        assert_eq!(
            ProxyError::no_route_found("/unknown/path").to_status_code(),
            StatusCode::NOT_FOUND
        );

        // Test upstream errors (502/503/504)
        assert_eq!(
            ProxyError::UpstreamUnavailable { address: "localhost:9090".to_string() }.to_status_code(),
            StatusCode::BAD_GATEWAY
        );
        assert_eq!(
            ProxyError::upstream_timeout("localhost:9090", 5000).to_status_code(),
            StatusCode::GATEWAY_TIMEOUT
        );
        assert_eq!(
            ProxyError::circuit_breaker_open("localhost:9090").to_status_code(),
            StatusCode::SERVICE_UNAVAILABLE
        );

        // Test internal server errors (500)
        assert_eq!(
            ProxyError::tls_handshake("certificate error").to_status_code(),
            StatusCode::INTERNAL_SERVER_ERROR
        );
        assert_eq!(
            ProxyError::ConfigError("invalid config".to_string()).to_status_code(),
            StatusCode::INTERNAL_SERVER_ERROR
        );

        // Test gRPC status errors (200 with grpc-status trailer)
        assert_eq!(
            ProxyError::grpc_status(3, "invalid argument").to_status_code(),
            StatusCode::OK
        );
    }

    #[test]
    fn test_error_categories() {
        assert_eq!(
            ProxyError::tls_handshake("error").category(),
            ErrorCategory::Tls
        );
        assert_eq!(
            ProxyError::connection_timeout("localhost:9090").category(),
            ErrorCategory::Network
        );
        assert_eq!(
            ProxyError::Http2Protocol { message: "error".to_string() }.category(),
            ErrorCategory::Protocol
        );
        assert_eq!(
            ProxyError::invalid_grpc_request("error").category(),
            ErrorCategory::Grpc
        );
        assert_eq!(
            ProxyError::no_route_found("/path").category(),
            ErrorCategory::Routing
        );
        assert_eq!(
            ProxyError::UpstreamUnavailable { address: "localhost:9090".to_string() }.category(),
            ErrorCategory::Upstream
        );
        assert_eq!(
            ProxyError::ConfigError("error".to_string()).category(),
            ErrorCategory::Configuration
        );
        assert_eq!(
            ProxyError::UriParse { uri: "invalid".to_string(), reason: "bad format".to_string() }.category(),
            ErrorCategory::Parsing
        );
        assert_eq!(
            ProxyError::ResourceExhausted { resource: "memory".to_string() }.category(),
            ErrorCategory::Resource
        );
        assert_eq!(
            ProxyError::Internal { message: "error".to_string() }.category(),
            ErrorCategory::Internal
        );
    }

    #[test]
    fn test_error_retryability() {
        // Retryable errors
        assert!(ProxyError::connection_timeout("localhost:9090").is_retryable());
        assert!(ProxyError::ConnectionReset { address: "localhost:9090".to_string() }.is_retryable());
        assert!(ProxyError::upstream_timeout("localhost:9090", 5000).is_retryable());
        assert!(ProxyError::UpstreamUnavailable { address: "localhost:9090".to_string() }.is_retryable());
        assert!(ProxyError::ResourceExhausted { resource: "memory".to_string() }.is_retryable());
        assert!(ProxyError::ServiceUnavailable { reason: "maintenance".to_string() }.is_retryable());

        // Non-retryable errors
        assert!(!ProxyError::invalid_grpc_request("bad request").is_retryable());
        assert!(!ProxyError::MissingGrpcContentType.is_retryable());
        assert!(!ProxyError::no_route_found("/path").is_retryable());
        assert!(!ProxyError::RequestTooLarge { size: 1000, max_size: 500 }.is_retryable());
        assert!(!ProxyError::RateLimitExceeded { identifier: "client1".to_string() }.is_retryable());
        assert!(!ProxyError::ConfigError("invalid config".to_string()).is_retryable());
        assert!(!ProxyError::circuit_breaker_open("localhost:9090").is_retryable());
        assert!(!ProxyError::upstream_health_check_failed("localhost:9090", "failed").is_retryable());
    }

    #[test]
    fn test_retry_delay() {
        // Retryable errors should return delay
        let error = ProxyError::connection_timeout("localhost:9090");
        assert!(error.retry_delay(0).is_some());
        assert!(error.retry_delay(1).is_some());
        assert!(error.retry_delay(2).is_some());

        // Non-retryable errors should return None
        let error = ProxyError::invalid_grpc_request("bad request");
        assert!(error.retry_delay(0).is_none());
        assert!(error.retry_delay(1).is_none());

        // Test exponential backoff
        let error = ProxyError::upstream_timeout("localhost:9090", 5000);
        let delay0 = error.retry_delay(0).unwrap();
        let delay1 = error.retry_delay(1).unwrap();
        let delay2 = error.retry_delay(2).unwrap();
        
        // Each delay should be roughly double the previous (with jitter)
        assert!(delay1 >= delay0);
        assert!(delay2 >= delay1);
        
        // Test delay bounds
        assert!(delay0 >= Duration::from_millis(200)); // base 200ms + jitter
        assert!(delay0 <= Duration::from_millis(300)); // base 200ms + 25% jitter
    }

    #[test]
    fn test_error_category_string_representation() {
        assert_eq!(ErrorCategory::Tls.as_str(), "tls");
        assert_eq!(ErrorCategory::Network.as_str(), "network");
        assert_eq!(ErrorCategory::Protocol.as_str(), "protocol");
        assert_eq!(ErrorCategory::Grpc.as_str(), "grpc");
        assert_eq!(ErrorCategory::Routing.as_str(), "routing");
        assert_eq!(ErrorCategory::Upstream.as_str(), "upstream");
        assert_eq!(ErrorCategory::Configuration.as_str(), "configuration");
        assert_eq!(ErrorCategory::Parsing.as_str(), "parsing");
        assert_eq!(ErrorCategory::Resource.as_str(), "resource");
        assert_eq!(ErrorCategory::Internal.as_str(), "internal");
    }

    #[test]
    fn test_error_helper_functions() {
        let error = ProxyError::tls_handshake("certificate expired");
        match error {
            ProxyError::TlsHandshake { message } => {
                assert_eq!(message, "certificate expired");
            }
            _ => panic!("Expected TlsHandshake error"),
        }

        let error = ProxyError::connection_timeout("192.168.1.1:8080");
        match error {
            ProxyError::ConnectionTimeout { address } => {
                assert_eq!(address, "192.168.1.1:8080");
            }
            _ => panic!("Expected ConnectionTimeout error"),
        }

        let error = ProxyError::upstream_timeout("example.com:9090", 30000);
        match error {
            ProxyError::UpstreamTimeout { address, timeout_ms } => {
                assert_eq!(address, "example.com:9090");
                assert_eq!(timeout_ms, 30000);
            }
            _ => panic!("Expected UpstreamTimeout error"),
        }

        let error = ProxyError::no_route_found("/api/v1/users");
        match error {
            ProxyError::NoRouteFound { path } => {
                assert_eq!(path, "/api/v1/users");
            }
            _ => panic!("Expected NoRouteFound error"),
        }

        let error = ProxyError::invalid_grpc_request("missing content-type header");
        match error {
            ProxyError::InvalidGrpcRequest { message } => {
                assert_eq!(message, "missing content-type header");
            }
            _ => panic!("Expected InvalidGrpcRequest error"),
        }

        let error = ProxyError::grpc_status(14, "service unavailable");
        match error {
            ProxyError::GrpcStatus { status, message } => {
                assert_eq!(status, 14);
                assert_eq!(message, "service unavailable");
            }
            _ => panic!("Expected GrpcStatus error"),
        }

        let error = ProxyError::circuit_breaker_open("backend.example.com:8080");
        match error {
            ProxyError::CircuitBreakerOpen { address } => {
                assert_eq!(address, "backend.example.com:8080");
            }
            _ => panic!("Expected CircuitBreakerOpen error"),
        }

        let error = ProxyError::upstream_health_check_failed("api.service.com:443", "connection refused");
        match error {
            ProxyError::UpstreamHealthCheckFailed { address, reason } => {
                assert_eq!(address, "api.service.com:443");
                assert_eq!(reason, "connection refused");
            }
            _ => panic!("Expected UpstreamHealthCheckFailed error"),
        }
    }

    #[test]
    fn test_error_display() {
        let error = ProxyError::tls_handshake("certificate validation failed");
        assert_eq!(error.to_string(), "TLS handshake failed: certificate validation failed");

        let error = ProxyError::upstream_timeout("service:8080", 5000);
        assert_eq!(error.to_string(), "Upstream timeout: service:8080, timeout: 5000ms");

        let error = ProxyError::no_route_found("/grpc.Service/Method");
        assert_eq!(error.to_string(), "No route found for path: /grpc.Service/Method");

        let error = ProxyError::grpc_status(3, "Invalid argument provided");
        assert_eq!(error.to_string(), "gRPC status error: status=3, message=Invalid argument provided");
    }

    #[test]
    fn test_error_from_conversions() {
        // Test std::io::Error conversion
        let io_error = std::io::Error::new(std::io::ErrorKind::ConnectionRefused, "connection refused");
        let proxy_error: ProxyError = io_error.into();
        match proxy_error {
            ProxyError::Io(_) => {} // Expected
            _ => panic!("Expected Io error"),
        }

        // Test hyper::Error conversion (would need actual hyper error)
        // This is tested implicitly in integration tests

        // Test serde_yaml::Error conversion (would need actual yaml error)
        // This is tested implicitly in configuration parsing tests

        // Test std::net::AddrParseError conversion
        let addr_parse_result = "invalid-address".parse::<std::net::SocketAddr>();
        assert!(addr_parse_result.is_err());
        let addr_error = addr_parse_result.unwrap_err();
        let proxy_error: ProxyError = addr_error.into();
        match proxy_error {
            ProxyError::AddrParse(_) => {} // Expected
            _ => panic!("Expected AddrParse error"),
        }
    }
}