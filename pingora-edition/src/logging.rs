//! Structured logging for the gRPC HTTP proxy
//! 
//! This module provides comprehensive logging capabilities with structured
//! output, request tracing, and performance monitoring.

use std::net::SocketAddr;
use std::time::{Duration, Instant};
use serde::{Deserialize, Serialize};
use tracing::{debug, error, info, warn, Span};
use uuid::Uuid;

/// Request context for structured logging
#[derive(Debug, Clone, Serialize)]
pub struct RequestContext {
    /// Unique request ID for tracing
    pub request_id: String,
    /// Client address
    pub client_addr: Option<SocketAddr>,
    /// Request method
    pub method: String,
    /// Request path
    pub path: String,
    /// Upstream address
    pub upstream_addr: Option<SocketAddr>,
    /// Request start time
    #[serde(skip)]
    pub start_time: Instant,
    /// TLS information
    pub tls_info: Option<TlsInfo>,
    /// Route information
    pub route_info: Option<RouteInfo>,
}

impl RequestContext {
    /// Create a new request context
    pub fn new(method: String, path: String) -> Self {
        Self {
            request_id: Uuid::new_v4().to_string(),
            client_addr: None,
            method,
            path,
            upstream_addr: None,
            start_time: Instant::now(),
            tls_info: None,
            route_info: None,
        }
    }

    /// Set client address
    pub fn with_client_addr(mut self, client_addr: SocketAddr) -> Self {
        self.client_addr = Some(client_addr);
        self
    }

    /// Set upstream address
    pub fn with_upstream_addr(mut self, upstream_addr: SocketAddr) -> Self {
        self.upstream_addr = Some(upstream_addr);
        self
    }

    /// Set TLS information
    pub fn with_tls_info(mut self, tls_info: TlsInfo) -> Self {
        self.tls_info = Some(tls_info);
        self
    }

    /// Set route information
    pub fn with_route_info(mut self, route_info: RouteInfo) -> Self {
        self.route_info = Some(route_info);
        self
    }

    /// Get elapsed time since request start
    pub fn elapsed(&self) -> Duration {
        self.start_time.elapsed()
    }
}

/// TLS connection information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TlsInfo {
    /// Whether TLS is enabled
    pub enabled: bool,
    /// Whether mTLS is used
    pub mtls: bool,
    /// TLS handshake duration
    pub handshake_duration: Option<Duration>,
    /// Client certificate subject (for mTLS)
    pub client_cert_subject: Option<String>,
    /// TLS version
    pub tls_version: Option<String>,
    /// Cipher suite
    pub cipher_suite: Option<String>,
}

impl TlsInfo {
    /// Create TLS info for plain HTTP/2 connection
    pub fn plain_http2() -> Self {
        Self {
            enabled: false,
            mtls: false,
            handshake_duration: None,
            client_cert_subject: None,
            tls_version: None,
            cipher_suite: None,
        }
    }

    /// Create TLS info for TLS connection
    pub fn tls(handshake_duration: Duration) -> Self {
        Self {
            enabled: true,
            mtls: false,
            handshake_duration: Some(handshake_duration),
            client_cert_subject: None,
            tls_version: Some("TLS 1.3".to_string()), // Default assumption
            cipher_suite: None,
        }
    }

    /// Create TLS info for mTLS connection
    pub fn mtls(handshake_duration: Duration, client_cert_subject: String) -> Self {
        Self {
            enabled: true,
            mtls: true,
            handshake_duration: Some(handshake_duration),
            client_cert_subject: Some(client_cert_subject),
            tls_version: Some("TLS 1.3".to_string()),
            cipher_suite: None,
        }
    }
}

/// Route matching information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteInfo {
    /// Matched route pattern
    pub pattern: String,
    /// Route priority
    pub priority: Option<u32>,
    /// Whether this is the default route
    pub is_default: bool,
}

impl RouteInfo {
    /// Create route info for a matched pattern
    pub fn matched(pattern: String, priority: Option<u32>) -> Self {
        Self {
            pattern,
            priority,
            is_default: false,
        }
    }

    /// Create route info for default route
    pub fn default() -> Self {
        Self {
            pattern: "default".to_string(),
            priority: None,
            is_default: true,
        }
    }
}

/// Connection event information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionEvent {
    /// Event type
    pub event_type: ConnectionEventType,
    /// Client address
    pub client_addr: SocketAddr,
    /// Connection duration (for close events)
    pub duration: Option<Duration>,
    /// TLS information
    pub tls_info: Option<TlsInfo>,
    /// Error information (for failed events)
    pub error: Option<String>,
}

/// Types of connection events
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ConnectionEventType {
    Established,
    Closed,
    Failed,
    TlsHandshakeStarted,
    TlsHandshakeCompleted,
    TlsHandshakeFailed,
}

/// Upstream event information
#[derive(Debug, Clone, Serialize)]
pub struct UpstreamEvent {
    /// Event type
    pub event_type: UpstreamEventType,
    /// Upstream address
    pub upstream_addr: SocketAddr,
    /// Duration (for connection/health check events)
    pub duration: Option<Duration>,
    /// Success status
    pub success: bool,
    /// Error information (for failed events)
    pub error: Option<String>,
    /// Request context (for request-related events)
    pub request_context: Option<RequestContext>,
}

/// Types of upstream events
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum UpstreamEventType {
    ConnectionAttempt,
    ConnectionEstablished,
    ConnectionFailed,
    HealthCheckStarted,
    HealthCheckCompleted,
    RequestForwarded,
    ResponseReceived,
}

/// Structured logger for the proxy
pub struct ProxyLogger;

impl ProxyLogger {
    /// Log server startup
    pub fn log_server_startup(bind_addr: SocketAddr, tls_enabled: bool, worker_threads: Option<usize>) {
        info!(
            event = "server_startup",
            bind_address = %bind_addr,
            tls_enabled = tls_enabled,
            worker_threads = worker_threads,
            "gRPC HTTP Proxy server starting"
        );
    }

    /// Log server shutdown
    pub fn log_server_shutdown(uptime: Duration) {
        info!(
            event = "server_shutdown",
            uptime_seconds = uptime.as_secs(),
            "gRPC HTTP Proxy server shutting down"
        );
    }

    /// Log configuration loaded
    pub fn log_configuration_loaded(config_path: &str, routes_count: usize, upstreams_count: usize) {
        info!(
            event = "configuration_loaded",
            config_path = config_path,
            routes_count = routes_count,
            upstreams_count = upstreams_count,
            "Configuration loaded successfully"
        );
    }

    /// Log configuration error
    pub fn log_configuration_error(config_path: &str, error: &str) {
        error!(
            event = "configuration_error",
            config_path = config_path,
            error = error,
            "Configuration loading failed"
        );
    }

    /// Log connection event
    pub fn log_connection_event(event: ConnectionEvent) {
        match event.event_type {
            ConnectionEventType::Established => {
                info!(
                    event = "connection_established",
                    client_addr = %event.client_addr,
                    tls_enabled = event.tls_info.as_ref().map(|t| t.enabled).unwrap_or(false),
                    mtls_enabled = event.tls_info.as_ref().map(|t| t.mtls).unwrap_or(false),
                    "Client connection established"
                );
            }
            ConnectionEventType::Closed => {
                info!(
                    event = "connection_closed",
                    client_addr = %event.client_addr,
                    duration_ms = event.duration.map(|d| d.as_millis()),
                    "Client connection closed"
                );
            }
            ConnectionEventType::Failed => {
                warn!(
                    event = "connection_failed",
                    client_addr = %event.client_addr,
                    error = event.error.as_deref().unwrap_or("unknown"),
                    "Client connection failed"
                );
            }
            ConnectionEventType::TlsHandshakeStarted => {
                debug!(
                    event = "tls_handshake_started",
                    client_addr = %event.client_addr,
                    "TLS handshake started"
                );
            }
            ConnectionEventType::TlsHandshakeCompleted => {
                info!(
                    event = "tls_handshake_completed",
                    client_addr = %event.client_addr,
                    duration_ms = event.tls_info.as_ref()
                        .and_then(|t| t.handshake_duration)
                        .map(|d| d.as_millis()),
                    mtls = event.tls_info.as_ref().map(|t| t.mtls).unwrap_or(false),
                    client_cert_subject = event.tls_info.as_ref()
                        .and_then(|t| t.client_cert_subject.as_deref()),
                    "TLS handshake completed"
                );
            }
            ConnectionEventType::TlsHandshakeFailed => {
                warn!(
                    event = "tls_handshake_failed",
                    client_addr = %event.client_addr,
                    error = event.error.as_deref().unwrap_or("unknown"),
                    "TLS handshake failed"
                );
            }
        }
    }

    /// Log request start
    pub fn log_request_start(context: &RequestContext) -> Span {
        let span = tracing::info_span!(
            "request",
            request_id = %context.request_id,
            method = %context.method,
            path = %context.path,
            client_addr = context.client_addr.map(|a| a.to_string()).as_deref(),
            upstream_addr = context.upstream_addr.map(|a| a.to_string()).as_deref(),
        );

        {
            let _enter = span.enter();
            info!(
                event = "request_start",
                request_id = %context.request_id,
                method = %context.method,
                path = %context.path,
                client_addr = context.client_addr.map(|a| a.to_string()).as_deref(),
                tls_enabled = context.tls_info.as_ref().map(|t| t.enabled),
                mtls_enabled = context.tls_info.as_ref().map(|t| t.mtls),
                route_pattern = context.route_info.as_ref().map(|r| r.pattern.as_str()),
                route_priority = context.route_info.as_ref().and_then(|r| r.priority),
                is_default_route = context.route_info.as_ref().map(|r| r.is_default),
                "Request started"
            );
        }

        span
    }

    /// Log request completion
    pub fn log_request_complete(
        context: &RequestContext,
        status_code: u16,
        bytes_sent: u64,
        bytes_received: u64,
        trailers_count: usize,
    ) {
        let duration = context.elapsed();
        
        info!(
            event = "request_complete",
            request_id = %context.request_id,
            method = %context.method,
            path = %context.path,
            status_code = status_code,
            duration_ms = duration.as_millis(),
            bytes_sent = bytes_sent,
            bytes_received = bytes_received,
            trailers_count = trailers_count,
            upstream_addr = context.upstream_addr.map(|a| a.to_string()).as_deref(),
            "Request completed"
        );
    }

    /// Log request error
    pub fn log_request_error(context: &RequestContext, error: &str, error_type: &str) {
        let duration = context.elapsed();
        
        error!(
            event = "request_error",
            request_id = %context.request_id,
            method = %context.method,
            path = %context.path,
            error = error,
            error_type = error_type,
            duration_ms = duration.as_millis(),
            upstream_addr = context.upstream_addr.map(|a| a.to_string()).as_deref(),
            "Request failed"
        );
    }

    /// Log upstream event
    pub fn log_upstream_event(event: UpstreamEvent) {
        match event.event_type {
            UpstreamEventType::ConnectionAttempt => {
                debug!(
                    event = "upstream_connection_attempt",
                    upstream_addr = %event.upstream_addr,
                    request_id = event.request_context.as_ref().map(|c| c.request_id.as_str()),
                    "Attempting connection to upstream"
                );
            }
            UpstreamEventType::ConnectionEstablished => {
                info!(
                    event = "upstream_connection_established",
                    upstream_addr = %event.upstream_addr,
                    duration_ms = event.duration.map(|d| d.as_millis()),
                    request_id = event.request_context.as_ref().map(|c| c.request_id.as_str()),
                    "Connection to upstream established"
                );
            }
            UpstreamEventType::ConnectionFailed => {
                warn!(
                    event = "upstream_connection_failed",
                    upstream_addr = %event.upstream_addr,
                    error = event.error.as_deref().unwrap_or("unknown"),
                    duration_ms = event.duration.map(|d| d.as_millis()),
                    request_id = event.request_context.as_ref().map(|c| c.request_id.as_str()),
                    "Connection to upstream failed"
                );
            }
            UpstreamEventType::HealthCheckStarted => {
                debug!(
                    event = "upstream_health_check_started",
                    upstream_addr = %event.upstream_addr,
                    "Health check started for upstream"
                );
            }
            UpstreamEventType::HealthCheckCompleted => {
                if event.success {
                    info!(
                        event = "upstream_health_check_completed",
                        upstream_addr = %event.upstream_addr,
                        success = event.success,
                        duration_ms = event.duration.map(|d| d.as_millis()),
                        "Health check completed for upstream"
                    );
                } else {
                    warn!(
                        event = "upstream_health_check_failed",
                        upstream_addr = %event.upstream_addr,
                        error = event.error.as_deref().unwrap_or("unknown"),
                        duration_ms = event.duration.map(|d| d.as_millis()),
                        "Health check failed for upstream"
                    );
                }
            }
            UpstreamEventType::RequestForwarded => {
                debug!(
                    event = "upstream_request_forwarded",
                    upstream_addr = %event.upstream_addr,
                    request_id = event.request_context.as_ref().map(|c| c.request_id.as_str()),
                    "Request forwarded to upstream"
                );
            }
            UpstreamEventType::ResponseReceived => {
                debug!(
                    event = "upstream_response_received",
                    upstream_addr = %event.upstream_addr,
                    request_id = event.request_context.as_ref().map(|c| c.request_id.as_str()),
                    duration_ms = event.duration.map(|d| d.as_millis()),
                    "Response received from upstream"
                );
            }
        }
    }

    /// Log routing decision
    pub fn log_routing_decision(
        request_path: &str,
        matched_pattern: Option<&str>,
        upstream_addr: SocketAddr,
        is_default: bool,
    ) {
        if is_default {
            info!(
                event = "routing_decision",
                request_path = request_path,
                upstream_addr = %upstream_addr,
                route_type = "default",
                "Request routed to default upstream"
            );
        } else {
            info!(
                event = "routing_decision",
                request_path = request_path,
                matched_pattern = matched_pattern.unwrap_or("unknown"),
                upstream_addr = %upstream_addr,
                route_type = "pattern_match",
                "Request routed via pattern match"
            );
        }
    }

    /// Log performance metrics
    pub fn log_performance_metrics(
        active_connections: u64,
        active_requests: u64,
        memory_usage_mb: Option<u64>,
        cpu_usage_percent: Option<f64>,
    ) {
        info!(
            event = "performance_metrics",
            active_connections = active_connections,
            active_requests = active_requests,
            memory_usage_mb = memory_usage_mb,
            cpu_usage_percent = cpu_usage_percent,
            "Performance metrics snapshot"
        );
    }

    /// Log error with context
    pub fn log_error_with_context(
        error_type: &str,
        error_message: &str,
        context: Option<&RequestContext>,
        additional_context: Option<&str>,
    ) {
        error!(
            event = "error_occurred",
            error_type = error_type,
            error_message = error_message,
            request_id = context.map(|c| c.request_id.as_str()),
            request_path = context.map(|c| c.path.as_str()),
            upstream_addr = context.and_then(|c| c.upstream_addr).map(|a| a.to_string()).as_deref(),
            additional_context = additional_context,
            "Error occurred during proxy operation"
        );
    }
}

/// Initialize structured logging for the proxy
pub fn init_logging(log_level: &str, json_format: bool) -> Result<(), Box<dyn std::error::Error>> {
    use tracing_subscriber::{fmt, EnvFilter, Registry};
    use tracing_subscriber::prelude::*;

    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(format!("grpc_http_proxy={},warn", log_level)));

    let registry = Registry::default().with(env_filter);

    if json_format {
        // JSON structured logging for production
        let json_layer = fmt::layer()
            .json()
            .with_current_span(true)
            .with_span_list(true)
            .with_target(false)
            .with_thread_ids(true)
            .with_file(true)
            .with_line_number(true);

        registry.with(json_layer).init();
    } else {
        // Human-readable logging for development
        let fmt_layer = fmt::layer()
            .with_target(false)
            .with_thread_ids(true)
            .with_file(true)
            .with_line_number(true)
            .with_ansi(true);

        registry.with(fmt_layer).init();
    }

    info!("Structured logging initialized with level: {}", log_level);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_request_context_creation() {
        let context = RequestContext::new("POST".to_string(), "/api/v1/test".to_string());
        
        assert_eq!(context.method, "POST");
        assert_eq!(context.path, "/api/v1/test");
        assert!(!context.request_id.is_empty());
        assert!(context.client_addr.is_none());
        assert!(context.upstream_addr.is_none());
    }

    #[test]
    fn test_request_context_builder() {
        let client_addr: SocketAddr = "127.0.0.1:8080".parse().unwrap();
        let upstream_addr: SocketAddr = "127.0.0.1:9000".parse().unwrap();
        
        let context = RequestContext::new("GET".to_string(), "/test".to_string())
            .with_client_addr(client_addr)
            .with_upstream_addr(upstream_addr)
            .with_tls_info(TlsInfo::tls(Duration::from_millis(50)))
            .with_route_info(RouteInfo::matched("/test".to_string(), Some(100)));
        
        assert_eq!(context.client_addr, Some(client_addr));
        assert_eq!(context.upstream_addr, Some(upstream_addr));
        assert!(context.tls_info.is_some());
        assert!(context.route_info.is_some());
    }

    #[test]
    fn test_tls_info_creation() {
        let plain = TlsInfo::plain_http2();
        assert!(!plain.enabled);
        assert!(!plain.mtls);
        
        let tls = TlsInfo::tls(Duration::from_millis(100));
        assert!(tls.enabled);
        assert!(!tls.mtls);
        assert_eq!(tls.handshake_duration, Some(Duration::from_millis(100)));
        
        let mtls = TlsInfo::mtls(Duration::from_millis(150), "CN=client".to_string());
        assert!(mtls.enabled);
        assert!(mtls.mtls);
        assert_eq!(mtls.client_cert_subject, Some("CN=client".to_string()));
    }

    #[test]
    fn test_route_info_creation() {
        let matched = RouteInfo::matched("/api/*".to_string(), Some(100));
        assert_eq!(matched.pattern, "/api/*");
        assert_eq!(matched.priority, Some(100));
        assert!(!matched.is_default);
        
        let default = RouteInfo::default();
        assert_eq!(default.pattern, "default");
        assert!(default.is_default);
        assert!(default.priority.is_none());
    }

    #[test]
    fn test_connection_event_creation() {
        let client_addr: SocketAddr = "127.0.0.1:8080".parse().unwrap();
        
        let event = ConnectionEvent {
            event_type: ConnectionEventType::Established,
            client_addr,
            duration: None,
            tls_info: Some(TlsInfo::tls(Duration::from_millis(50))),
            error: None,
        };
        
        assert!(matches!(event.event_type, ConnectionEventType::Established));
        assert_eq!(event.client_addr, client_addr);
        assert!(event.tls_info.is_some());
    }

    #[test]
    fn test_upstream_event_creation() {
        let upstream_addr: SocketAddr = "127.0.0.1:9000".parse().unwrap();
        
        let event = UpstreamEvent {
            event_type: UpstreamEventType::ConnectionEstablished,
            upstream_addr,
            duration: Some(Duration::from_millis(25)),
            success: true,
            error: None,
            request_context: None,
        };
        
        assert!(matches!(event.event_type, UpstreamEventType::ConnectionEstablished));
        assert_eq!(event.upstream_addr, upstream_addr);
        assert!(event.success);
        assert_eq!(event.duration, Some(Duration::from_millis(25)));
    }
}