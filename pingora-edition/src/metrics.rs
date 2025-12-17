//! Metrics collection and monitoring for the gRPC HTTP proxy
//! 
//! This module provides structured metrics collection for monitoring proxy
//! performance, connection statistics, and operational health.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};
use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};

/// Global metrics collector instance
static METRICS: once_cell::sync::Lazy<Arc<MetricsCollector>> = 
    once_cell::sync::Lazy::new(|| Arc::new(MetricsCollector::new()));

/// Get the global metrics collector instance
pub fn metrics() -> Arc<MetricsCollector> {
    METRICS.clone()
}

/// Main metrics collector for the proxy
pub struct MetricsCollector {
    /// Connection metrics
    connection_metrics: Arc<RwLock<ConnectionMetrics>>,
    /// Request metrics
    request_metrics: Arc<RwLock<RequestMetrics>>,
    /// Upstream metrics
    upstream_metrics: Arc<RwLock<UpstreamMetrics>>,
    /// TLS metrics
    tls_metrics: Arc<RwLock<TlsMetrics>>,
    /// Error metrics
    error_metrics: Arc<RwLock<ErrorMetrics>>,
    /// Performance metrics
    performance_metrics: Arc<RwLock<PerformanceMetrics>>,
}

impl MetricsCollector {
    /// Create a new metrics collector
    pub fn new() -> Self {
        Self {
            connection_metrics: Arc::new(RwLock::new(ConnectionMetrics::new())),
            request_metrics: Arc::new(RwLock::new(RequestMetrics::new())),
            upstream_metrics: Arc::new(RwLock::new(UpstreamMetrics::new())),
            tls_metrics: Arc::new(RwLock::new(TlsMetrics::new())),
            error_metrics: Arc::new(RwLock::new(ErrorMetrics::new())),
            performance_metrics: Arc::new(RwLock::new(PerformanceMetrics::new())),
        }
    }

    /// Record a new connection
    pub fn record_connection_established(&self, client_addr: SocketAddr, is_tls: bool) {
        if let Ok(mut metrics) = self.connection_metrics.write() {
            metrics.total_connections += 1;
            metrics.active_connections += 1;
            
            if is_tls {
                metrics.tls_connections += 1;
            } else {
                metrics.plain_connections += 1;
            }
        }

        debug!("Connection established from {}, TLS: {}", client_addr, is_tls);
    }

    /// Record a connection closed
    pub fn record_connection_closed(&self, client_addr: SocketAddr, duration: Duration) {
        if let Ok(mut metrics) = self.connection_metrics.write() {
            metrics.active_connections = metrics.active_connections.saturating_sub(1);
            metrics.total_connection_duration += duration;
        }

        debug!("Connection closed from {}, duration: {:?}", client_addr, duration);
    }

    /// Record a new request
    pub fn record_request_started(&self, method: &str, path: &str, upstream_addr: SocketAddr) -> RequestTracker {
        if let Ok(mut metrics) = self.request_metrics.write() {
            metrics.total_requests += 1;
            metrics.active_requests += 1;
            
            // Track requests by method
            *metrics.requests_by_method.entry(method.to_string()).or_insert(0) += 1;
            
            // Track requests by upstream
            *metrics.requests_by_upstream.entry(upstream_addr).or_insert(0) += 1;
        }

        debug!("Request started: {} {} -> {}", method, path, upstream_addr);
        
        RequestTracker {
            start_time: Instant::now(),
            method: method.to_string(),
            path: path.to_string(),
            upstream_addr,
        }
    }

    /// Record a request completed
    pub fn record_request_completed(&self, tracker: RequestTracker, status_code: u16, bytes_sent: u64, bytes_received: u64) {
        let duration = tracker.start_time.elapsed();
        
        if let Ok(mut metrics) = self.request_metrics.write() {
            metrics.active_requests = metrics.active_requests.saturating_sub(1);
            metrics.total_request_duration += duration;
            metrics.total_bytes_sent += bytes_sent;
            metrics.total_bytes_received += bytes_received;
            
            // Track response codes
            *metrics.response_codes.entry(status_code).or_insert(0) += 1;
            
            // Track request latency buckets
            let latency_ms = duration.as_millis() as u64;
            if latency_ms < 10 {
                metrics.latency_buckets.under_10ms += 1;
            } else if latency_ms < 50 {
                metrics.latency_buckets.under_50ms += 1;
            } else if latency_ms < 100 {
                metrics.latency_buckets.under_100ms += 1;
            } else if latency_ms < 500 {
                metrics.latency_buckets.under_500ms += 1;
            } else if latency_ms < 1000 {
                metrics.latency_buckets.under_1s += 1;
            } else {
                metrics.latency_buckets.over_1s += 1;
            }
        }

        info!(
            "Request completed: {} {} -> {} | Status: {} | Duration: {:?} | Sent: {} bytes | Received: {} bytes",
            tracker.method, tracker.path, tracker.upstream_addr, status_code, duration, bytes_sent, bytes_received
        );
    }

    /// Record upstream connection event
    pub fn record_upstream_connection(&self, upstream_addr: SocketAddr, success: bool, duration: Duration) {
        if let Ok(mut metrics) = self.upstream_metrics.write() {
            let upstream_stats = metrics.upstream_stats.entry(upstream_addr).or_insert_with(|| UpstreamStats::new(upstream_addr));
            
            if success {
                upstream_stats.successful_connections += 1;
                upstream_stats.total_connection_time += duration;
            } else {
                upstream_stats.failed_connections += 1;
            }
            
            upstream_stats.last_connection_attempt = Some(Instant::now());
        }

        if success {
            debug!("Upstream connection successful: {} in {:?}", upstream_addr, duration);
        } else {
            warn!("Upstream connection failed: {} after {:?}", upstream_addr, duration);
        }
    }

    /// Record upstream health check result
    pub fn record_upstream_health_check(&self, upstream_addr: SocketAddr, healthy: bool, response_time: Duration) {
        if let Ok(mut metrics) = self.upstream_metrics.write() {
            let upstream_stats = metrics.upstream_stats.entry(upstream_addr).or_insert_with(|| UpstreamStats::new(upstream_addr));
            
            upstream_stats.health_checks_total += 1;
            if healthy {
                upstream_stats.health_checks_successful += 1;
                upstream_stats.consecutive_failures = 0;
            } else {
                upstream_stats.consecutive_failures += 1;
            }
            
            upstream_stats.last_health_check = Some(Instant::now());
            upstream_stats.last_health_check_duration = Some(response_time);
        }

        if healthy {
            debug!("Upstream health check passed: {} in {:?}", upstream_addr, response_time);
        } else {
            warn!("Upstream health check failed: {} after {:?}", upstream_addr, response_time);
        }
    }

    /// Record TLS handshake event
    pub fn record_tls_handshake(&self, client_addr: SocketAddr, success: bool, duration: Duration, is_mtls: bool) {
        if let Ok(mut metrics) = self.tls_metrics.write() {
            metrics.total_handshakes += 1;
            
            if success {
                metrics.successful_handshakes += 1;
                metrics.total_handshake_duration += duration;
                
                if is_mtls {
                    metrics.mtls_handshakes += 1;
                }
            } else {
                metrics.failed_handshakes += 1;
            }
        }

        if success {
            info!("TLS handshake successful: {} in {:?}, mTLS: {}", client_addr, duration, is_mtls);
        } else {
            warn!("TLS handshake failed: {} after {:?}", client_addr, duration);
        }
    }

    /// Record an error occurrence
    pub fn record_error(&self, error_type: &str, error_message: &str, context: Option<&str>) {
        if let Ok(mut metrics) = self.error_metrics.write() {
            metrics.total_errors += 1;
            *metrics.errors_by_type.entry(error_type.to_string()).or_insert(0) += 1;
        }

        if let Some(ctx) = context {
            warn!("Error recorded: {} - {} (context: {})", error_type, error_message, ctx);
        } else {
            warn!("Error recorded: {} - {}", error_type, error_message);
        }
    }

    /// Record performance metrics
    pub fn record_memory_usage(&self, memory_bytes: u64) {
        if let Ok(mut metrics) = self.performance_metrics.write() {
            metrics.current_memory_usage = memory_bytes;
            if memory_bytes > metrics.peak_memory_usage {
                metrics.peak_memory_usage = memory_bytes;
            }
        }
    }

    /// Record CPU usage
    pub fn record_cpu_usage(&self, cpu_percent: f64) {
        if let Ok(mut metrics) = self.performance_metrics.write() {
            metrics.current_cpu_usage = cpu_percent;
            if cpu_percent > metrics.peak_cpu_usage {
                metrics.peak_cpu_usage = cpu_percent;
            }
        }
    }

    /// Get current metrics snapshot
    pub fn get_metrics_snapshot(&self) -> MetricsSnapshot {
        let connection_metrics = self.connection_metrics.read().unwrap().clone();
        let request_metrics = self.request_metrics.read().unwrap().clone();
        let upstream_metrics = self.upstream_metrics.read().unwrap().clone();
        let tls_metrics = self.tls_metrics.read().unwrap().clone();
        let error_metrics = self.error_metrics.read().unwrap().clone();
        let performance_metrics = self.performance_metrics.read().unwrap().clone();

        MetricsSnapshot {
            timestamp: Instant::now(),
            connection_metrics,
            request_metrics,
            upstream_metrics,
            tls_metrics,
            error_metrics,
            performance_metrics,
        }
    }

    /// Get health status based on metrics
    pub fn get_health_status(&self) -> HealthStatus {
        let snapshot = self.get_metrics_snapshot();
        
        // Check for critical issues
        let mut issues = Vec::new();
        
        // Check error rate
        if snapshot.request_metrics.total_requests > 0 {
            let error_rate = (snapshot.error_metrics.total_errors as f64) / (snapshot.request_metrics.total_requests as f64);
            if error_rate > 0.1 {  // More than 10% error rate
                issues.push(format!("High error rate: {:.2}%", error_rate * 100.0));
            }
        }
        
        // Check upstream health
        let unhealthy_upstreams: Vec<_> = snapshot.upstream_metrics.upstream_stats
            .values()
            .filter(|stats| {
                stats.health_checks_total > 0 && 
                (stats.health_checks_successful as f64 / stats.health_checks_total as f64) < 0.8
            })
            .map(|stats| stats.address)
            .collect();
        
        if !unhealthy_upstreams.is_empty() {
            issues.push(format!("Unhealthy upstreams: {:?}", unhealthy_upstreams));
        }
        
        // Check memory usage (if available)
        if snapshot.performance_metrics.current_memory_usage > 0 {
            let memory_mb = snapshot.performance_metrics.current_memory_usage / (1024 * 1024);
            if memory_mb > 1000 {  // More than 1GB
                issues.push(format!("High memory usage: {} MB", memory_mb));
            }
        }
        
        if issues.is_empty() {
            HealthStatus::Healthy
        } else {
            HealthStatus::Degraded { issues }
        }
    }

    /// Reset all metrics (useful for testing)
    pub fn reset(&self) {
        if let Ok(mut metrics) = self.connection_metrics.write() {
            *metrics = ConnectionMetrics::new();
        }
        if let Ok(mut metrics) = self.request_metrics.write() {
            *metrics = RequestMetrics::new();
        }
        if let Ok(mut metrics) = self.upstream_metrics.write() {
            *metrics = UpstreamMetrics::new();
        }
        if let Ok(mut metrics) = self.tls_metrics.write() {
            *metrics = TlsMetrics::new();
        }
        if let Ok(mut metrics) = self.error_metrics.write() {
            *metrics = ErrorMetrics::new();
        }
        if let Ok(mut metrics) = self.performance_metrics.write() {
            *metrics = PerformanceMetrics::new();
        }
    }
}

impl Default for MetricsCollector {
    fn default() -> Self {
        Self::new()
    }
}

/// Request tracker for measuring request duration and context
pub struct RequestTracker {
    start_time: Instant,
    method: String,
    path: String,
    upstream_addr: SocketAddr,
}

/// Connection-related metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionMetrics {
    pub total_connections: u64,
    pub active_connections: u64,
    pub tls_connections: u64,
    pub plain_connections: u64,
    pub total_connection_duration: Duration,
}

impl ConnectionMetrics {
    fn new() -> Self {
        Self {
            total_connections: 0,
            active_connections: 0,
            tls_connections: 0,
            plain_connections: 0,
            total_connection_duration: Duration::ZERO,
        }
    }
}

/// Request-related metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestMetrics {
    pub total_requests: u64,
    pub active_requests: u64,
    pub total_request_duration: Duration,
    pub total_bytes_sent: u64,
    pub total_bytes_received: u64,
    pub requests_by_method: HashMap<String, u64>,
    pub requests_by_upstream: HashMap<SocketAddr, u64>,
    pub response_codes: HashMap<u16, u64>,
    pub latency_buckets: LatencyBuckets,
}

impl RequestMetrics {
    fn new() -> Self {
        Self {
            total_requests: 0,
            active_requests: 0,
            total_request_duration: Duration::ZERO,
            total_bytes_sent: 0,
            total_bytes_received: 0,
            requests_by_method: HashMap::new(),
            requests_by_upstream: HashMap::new(),
            response_codes: HashMap::new(),
            latency_buckets: LatencyBuckets::new(),
        }
    }
}

/// Latency distribution buckets
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LatencyBuckets {
    pub under_10ms: u64,
    pub under_50ms: u64,
    pub under_100ms: u64,
    pub under_500ms: u64,
    pub under_1s: u64,
    pub over_1s: u64,
}

impl LatencyBuckets {
    fn new() -> Self {
        Self {
            under_10ms: 0,
            under_50ms: 0,
            under_100ms: 0,
            under_500ms: 0,
            under_1s: 0,
            over_1s: 0,
        }
    }
}

/// Upstream server metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpstreamMetrics {
    pub upstream_stats: HashMap<SocketAddr, UpstreamStats>,
}

impl UpstreamMetrics {
    fn new() -> Self {
        Self {
            upstream_stats: HashMap::new(),
        }
    }
}

/// Statistics for a specific upstream server
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpstreamStats {
    pub address: SocketAddr,
    pub successful_connections: u64,
    pub failed_connections: u64,
    pub total_connection_time: Duration,
    pub health_checks_total: u64,
    pub health_checks_successful: u64,
    pub consecutive_failures: u32,
    #[serde(skip)]
    pub last_connection_attempt: Option<Instant>,
    #[serde(skip)]
    pub last_health_check: Option<Instant>,
    pub last_health_check_duration: Option<Duration>,
}

impl UpstreamStats {
    fn new(address: SocketAddr) -> Self {
        Self {
            address,
            successful_connections: 0,
            failed_connections: 0,
            total_connection_time: Duration::ZERO,
            health_checks_total: 0,
            health_checks_successful: 0,
            consecutive_failures: 0,
            last_connection_attempt: None,
            last_health_check: None,
            last_health_check_duration: None,
        }
    }
}

/// TLS-related metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TlsMetrics {
    pub total_handshakes: u64,
    pub successful_handshakes: u64,
    pub failed_handshakes: u64,
    pub mtls_handshakes: u64,
    pub total_handshake_duration: Duration,
}

impl TlsMetrics {
    fn new() -> Self {
        Self {
            total_handshakes: 0,
            successful_handshakes: 0,
            failed_handshakes: 0,
            mtls_handshakes: 0,
            total_handshake_duration: Duration::ZERO,
        }
    }
}

/// Error-related metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorMetrics {
    pub total_errors: u64,
    pub errors_by_type: HashMap<String, u64>,
}

impl ErrorMetrics {
    fn new() -> Self {
        Self {
            total_errors: 0,
            errors_by_type: HashMap::new(),
        }
    }
}

/// Performance-related metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceMetrics {
    pub current_memory_usage: u64,
    pub peak_memory_usage: u64,
    pub current_cpu_usage: f64,
    pub peak_cpu_usage: f64,
}

impl PerformanceMetrics {
    fn new() -> Self {
        Self {
            current_memory_usage: 0,
            peak_memory_usage: 0,
            current_cpu_usage: 0.0,
            peak_cpu_usage: 0.0,
        }
    }
}

/// Complete metrics snapshot
#[derive(Debug, Clone, Serialize)]
pub struct MetricsSnapshot {
    #[serde(skip)]
    pub timestamp: Instant,
    pub connection_metrics: ConnectionMetrics,
    pub request_metrics: RequestMetrics,
    pub upstream_metrics: UpstreamMetrics,
    pub tls_metrics: TlsMetrics,
    pub error_metrics: ErrorMetrics,
    pub performance_metrics: PerformanceMetrics,
}

/// Health status of the proxy
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum HealthStatus {
    Healthy,
    Degraded { issues: Vec<String> },
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn test_metrics_collector_creation() {
        let collector = MetricsCollector::new();
        let snapshot = collector.get_metrics_snapshot();
        
        assert_eq!(snapshot.connection_metrics.total_connections, 0);
        assert_eq!(snapshot.request_metrics.total_requests, 0);
        assert_eq!(snapshot.error_metrics.total_errors, 0);
    }

    #[test]
    fn test_connection_metrics() {
        let collector = MetricsCollector::new();
        let client_addr: SocketAddr = "127.0.0.1:8080".parse().unwrap();
        
        collector.record_connection_established(client_addr, true);
        collector.record_connection_established(client_addr, false);
        
        let snapshot = collector.get_metrics_snapshot();
        assert_eq!(snapshot.connection_metrics.total_connections, 2);
        assert_eq!(snapshot.connection_metrics.active_connections, 2);
        assert_eq!(snapshot.connection_metrics.tls_connections, 1);
        assert_eq!(snapshot.connection_metrics.plain_connections, 1);
        
        collector.record_connection_closed(client_addr, Duration::from_millis(100));
        
        let snapshot = collector.get_metrics_snapshot();
        assert_eq!(snapshot.connection_metrics.active_connections, 1);
    }

    #[test]
    fn test_request_metrics() {
        let collector = MetricsCollector::new();
        let upstream_addr: SocketAddr = "127.0.0.1:9000".parse().unwrap();
        
        let tracker = collector.record_request_started("POST", "/api/v1/test", upstream_addr);
        
        // Simulate some processing time
        thread::sleep(Duration::from_millis(10));
        
        collector.record_request_completed(tracker, 200, 1024, 2048);
        
        let snapshot = collector.get_metrics_snapshot();
        assert_eq!(snapshot.request_metrics.total_requests, 1);
        assert_eq!(snapshot.request_metrics.active_requests, 0);
        assert_eq!(snapshot.request_metrics.total_bytes_sent, 1024);
        assert_eq!(snapshot.request_metrics.total_bytes_received, 2048);
        assert_eq!(snapshot.request_metrics.response_codes.get(&200), Some(&1));
        assert_eq!(snapshot.request_metrics.requests_by_method.get("POST"), Some(&1));
    }

    #[test]
    fn test_upstream_metrics() {
        let collector = MetricsCollector::new();
        let upstream_addr: SocketAddr = "127.0.0.1:9000".parse().unwrap();
        
        collector.record_upstream_connection(upstream_addr, true, Duration::from_millis(50));
        collector.record_upstream_connection(upstream_addr, false, Duration::from_millis(100));
        collector.record_upstream_health_check(upstream_addr, true, Duration::from_millis(25));
        
        let snapshot = collector.get_metrics_snapshot();
        let upstream_stats = snapshot.upstream_metrics.upstream_stats.get(&upstream_addr).unwrap();
        
        assert_eq!(upstream_stats.successful_connections, 1);
        assert_eq!(upstream_stats.failed_connections, 1);
        assert_eq!(upstream_stats.health_checks_total, 1);
        assert_eq!(upstream_stats.health_checks_successful, 1);
    }

    #[test]
    fn test_error_metrics() {
        let collector = MetricsCollector::new();
        
        collector.record_error("ConfigError", "Invalid configuration", Some("config.yaml"));
        collector.record_error("UpstreamError", "Connection failed", None);
        collector.record_error("ConfigError", "Missing field", Some("server.bind_address"));
        
        let snapshot = collector.get_metrics_snapshot();
        assert_eq!(snapshot.error_metrics.total_errors, 3);
        assert_eq!(snapshot.error_metrics.errors_by_type.get("ConfigError"), Some(&2));
        assert_eq!(snapshot.error_metrics.errors_by_type.get("UpstreamError"), Some(&1));
    }

    #[test]
    fn test_health_status() {
        let collector = MetricsCollector::new();
        
        // Initially healthy
        let health = collector.get_health_status();
        assert!(matches!(health, HealthStatus::Healthy));
        
        // Add some errors to make it degraded
        for _ in 0..10 {
            collector.record_request_started("POST", "/test", "127.0.0.1:9000".parse().unwrap());
        }
        
        for _ in 0..2 {
            collector.record_error("TestError", "Test error", None);
        }
        
        let health = collector.get_health_status();
        assert!(matches!(health, HealthStatus::Degraded { .. }));
    }

    #[test]
    fn test_metrics_reset() {
        let collector = MetricsCollector::new();
        let client_addr: SocketAddr = "127.0.0.1:8080".parse().unwrap();
        
        collector.record_connection_established(client_addr, true);
        collector.record_error("TestError", "Test error", None);
        
        let snapshot_before = collector.get_metrics_snapshot();
        assert_eq!(snapshot_before.connection_metrics.total_connections, 1);
        assert_eq!(snapshot_before.error_metrics.total_errors, 1);
        
        collector.reset();
        
        let snapshot_after = collector.get_metrics_snapshot();
        assert_eq!(snapshot_after.connection_metrics.total_connections, 0);
        assert_eq!(snapshot_after.error_metrics.total_errors, 0);
    }
}