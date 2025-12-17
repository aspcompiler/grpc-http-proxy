//! Health check endpoint for monitoring the gRPC HTTP proxy
//! 
//! This module provides HTTP endpoints for health checking and metrics
//! collection, suitable for integration with load balancers and monitoring systems.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::time::{SystemTime, UNIX_EPOCH};
use serde::{Deserialize, Serialize};
use tokio::net::TcpListener;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tracing::{debug, error, info};

use crate::metrics::{metrics, HealthStatus, MetricsSnapshot};

/// Health check server for monitoring endpoints
pub struct HealthServer {
    bind_addr: SocketAddr,
    listener: Option<TcpListener>,
}

impl HealthServer {
    /// Create a new health check server
    pub fn new(bind_addr: SocketAddr) -> Self {
        Self {
            bind_addr,
            listener: None,
        }
    }

    /// Start the health check server
    pub async fn start(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let listener = TcpListener::bind(self.bind_addr).await?;
        info!("Health check server listening on {}", self.bind_addr);
        
        self.listener = Some(listener);
        Ok(())
    }

    /// Run the health check server (blocking)
    pub async fn run(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        if self.listener.is_none() {
            self.start().await?;
        }

        let listener = self.listener.as_ref().unwrap();
        
        loop {
            match listener.accept().await {
                Ok((mut stream, addr)) => {
                    debug!("Health check request from {}", addr);
                    
                    tokio::spawn(async move {
                        if let Err(e) = handle_health_request(&mut stream).await {
                            error!("Error handling health check request from {}: {}", addr, e);
                        }
                    });
                }
                Err(e) => {
                    error!("Error accepting health check connection: {}", e);
                }
            }
        }
    }

    /// Stop the health check server
    pub async fn stop(&mut self) {
        if let Some(listener) = self.listener.take() {
            drop(listener);
            info!("Health check server stopped");
        }
    }
}

/// Handle a health check HTTP request
async fn handle_health_request(stream: &mut tokio::net::TcpStream) -> Result<(), Box<dyn std::error::Error>> {
    let mut buffer = [0; 1024];
    let bytes_read = stream.read(&mut buffer).await?;
    
    if bytes_read == 0 {
        return Ok(());
    }
    
    let request = String::from_utf8_lossy(&buffer[..bytes_read]);
    debug!("Health check request: {}", request.lines().next().unwrap_or(""));
    
    // Parse the HTTP request line
    let first_line = request.lines().next().unwrap_or("");
    let parts: Vec<&str> = first_line.split_whitespace().collect();
    
    if parts.len() < 2 {
        send_response(stream, 400, "Bad Request", "text/plain", b"Bad Request").await?;
        return Ok(());
    }
    
    let method = parts[0];
    let path = parts[1];
    
    if method != "GET" {
        send_response(stream, 405, "Method Not Allowed", "text/plain", b"Method Not Allowed").await?;
        return Ok(());
    }
    
    match path {
        "/health" => handle_health_endpoint(stream).await?,
        "/health/ready" => handle_readiness_endpoint(stream).await?,
        "/health/live" => handle_liveness_endpoint(stream).await?,
        "/metrics" => handle_metrics_endpoint(stream).await?,
        "/metrics/json" => handle_metrics_json_endpoint(stream).await?,
        "/status" => handle_status_endpoint(stream).await?,
        _ => send_response(stream, 404, "Not Found", "text/plain", b"Not Found").await?,
    }
    
    Ok(())
}

/// Handle /health endpoint - comprehensive health check
async fn handle_health_endpoint(stream: &mut tokio::net::TcpStream) -> Result<(), Box<dyn std::error::Error>> {
    let health_status = metrics().get_health_status();
    let health_response = HealthResponse::from_status(health_status);
    
    let response_body = serde_json::to_string_pretty(&health_response)?;
    
    match health_response.status.as_str() {
        "healthy" => {
            send_response(stream, 200, "OK", "application/json", response_body.as_bytes()).await?;
        }
        "degraded" => {
            send_response(stream, 200, "OK", "application/json", response_body.as_bytes()).await?;
        }
        _ => {
            send_response(stream, 503, "Service Unavailable", "application/json", response_body.as_bytes()).await?;
        }
    }
    
    Ok(())
}

/// Handle /health/ready endpoint - readiness probe
async fn handle_readiness_endpoint(stream: &mut tokio::net::TcpStream) -> Result<(), Box<dyn std::error::Error>> {
    let health_status = metrics().get_health_status();
    
    match health_status {
        HealthStatus::Healthy => {
            let response = ReadinessResponse {
                status: "ready".to_string(),
                timestamp: current_timestamp(),
            };
            let response_body = serde_json::to_string(&response)?;
            send_response(stream, 200, "OK", "application/json", response_body.as_bytes()).await?;
        }
        HealthStatus::Degraded { .. } => {
            let response = ReadinessResponse {
                status: "not_ready".to_string(),
                timestamp: current_timestamp(),
            };
            let response_body = serde_json::to_string(&response)?;
            send_response(stream, 503, "Service Unavailable", "application/json", response_body.as_bytes()).await?;
        }
    }
    
    Ok(())
}

/// Handle /health/live endpoint - liveness probe
async fn handle_liveness_endpoint(stream: &mut tokio::net::TcpStream) -> Result<(), Box<dyn std::error::Error>> {
    // For liveness, we just check if the service is running
    // This is a simple check that the process is alive and responding
    let response = LivenessResponse {
        status: "alive".to_string(),
        timestamp: current_timestamp(),
        uptime_seconds: get_uptime_seconds(),
    };
    
    let response_body = serde_json::to_string(&response)?;
    send_response(stream, 200, "OK", "application/json", response_body.as_bytes()).await?;
    
    Ok(())
}

/// Handle /metrics endpoint - Prometheus-style metrics
async fn handle_metrics_endpoint(stream: &mut tokio::net::TcpStream) -> Result<(), Box<dyn std::error::Error>> {
    let snapshot = metrics().get_metrics_snapshot();
    let prometheus_metrics = format_prometheus_metrics(&snapshot);
    
    send_response(stream, 200, "OK", "text/plain; version=0.0.4", prometheus_metrics.as_bytes()).await?;
    
    Ok(())
}

/// Handle /metrics/json endpoint - JSON metrics
async fn handle_metrics_json_endpoint(stream: &mut tokio::net::TcpStream) -> Result<(), Box<dyn std::error::Error>> {
    let snapshot = metrics().get_metrics_snapshot();
    let json_metrics = serde_json::to_string_pretty(&snapshot)?;
    
    send_response(stream, 200, "OK", "application/json", json_metrics.as_bytes()).await?;
    
    Ok(())
}

/// Handle /status endpoint - detailed status information
async fn handle_status_endpoint(stream: &mut tokio::net::TcpStream) -> Result<(), Box<dyn std::error::Error>> {
    let snapshot = metrics().get_metrics_snapshot();
    let health_status = metrics().get_health_status();
    
    let status_response = StatusResponse {
        status: match health_status {
            HealthStatus::Healthy => "healthy".to_string(),
            HealthStatus::Degraded { .. } => "degraded".to_string(),
        },
        timestamp: current_timestamp(),
        uptime_seconds: get_uptime_seconds(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        connections: ConnectionStatus {
            total: snapshot.connection_metrics.total_connections,
            active: snapshot.connection_metrics.active_connections,
            tls: snapshot.connection_metrics.tls_connections,
            plain: snapshot.connection_metrics.plain_connections,
        },
        requests: RequestStatus {
            total: snapshot.request_metrics.total_requests,
            active: snapshot.request_metrics.active_requests,
            average_duration_ms: if snapshot.request_metrics.total_requests > 0 {
                Some(snapshot.request_metrics.total_request_duration.as_millis() as u64 / snapshot.request_metrics.total_requests)
            } else {
                None
            },
        },
        upstreams: snapshot.upstream_metrics.upstream_stats.iter().map(|(addr, stats)| {
            UpstreamStatus {
                address: *addr,
                healthy: stats.health_checks_total == 0 || 
                        (stats.health_checks_successful as f64 / stats.health_checks_total as f64) >= 0.8,
                total_connections: stats.successful_connections + stats.failed_connections,
                successful_connections: stats.successful_connections,
                failed_connections: stats.failed_connections,
                consecutive_failures: stats.consecutive_failures,
            }
        }).collect(),
        errors: ErrorStatus {
            total: snapshot.error_metrics.total_errors,
            by_type: snapshot.error_metrics.errors_by_type.clone(),
        },
    };
    
    let response_body = serde_json::to_string_pretty(&status_response)?;
    send_response(stream, 200, "OK", "application/json", response_body.as_bytes()).await?;
    
    Ok(())
}

/// Send HTTP response
async fn send_response(
    stream: &mut tokio::net::TcpStream,
    status_code: u16,
    status_text: &str,
    content_type: &str,
    body: &[u8],
) -> Result<(), Box<dyn std::error::Error>> {
    let response = format!(
        "HTTP/1.1 {} {}\r\n\
         Content-Type: {}\r\n\
         Content-Length: {}\r\n\
         Connection: close\r\n\
         Server: grpc-http-proxy/{}\r\n\
         \r\n",
        status_code,
        status_text,
        content_type,
        body.len(),
        env!("CARGO_PKG_VERSION")
    );
    
    stream.write_all(response.as_bytes()).await?;
    stream.write_all(body).await?;
    stream.flush().await?;
    
    Ok(())
}

/// Format metrics in Prometheus format
fn format_prometheus_metrics(snapshot: &MetricsSnapshot) -> String {
    let mut output = String::new();
    
    // Connection metrics
    output.push_str("# HELP grpc_proxy_connections_total Total number of connections\n");
    output.push_str("# TYPE grpc_proxy_connections_total counter\n");
    output.push_str(&format!("grpc_proxy_connections_total {}\n", snapshot.connection_metrics.total_connections));
    
    output.push_str("# HELP grpc_proxy_connections_active Current active connections\n");
    output.push_str("# TYPE grpc_proxy_connections_active gauge\n");
    output.push_str(&format!("grpc_proxy_connections_active {}\n", snapshot.connection_metrics.active_connections));
    
    output.push_str("# HELP grpc_proxy_connections_tls_total Total TLS connections\n");
    output.push_str("# TYPE grpc_proxy_connections_tls_total counter\n");
    output.push_str(&format!("grpc_proxy_connections_tls_total {}\n", snapshot.connection_metrics.tls_connections));
    
    // Request metrics
    output.push_str("# HELP grpc_proxy_requests_total Total number of requests\n");
    output.push_str("# TYPE grpc_proxy_requests_total counter\n");
    output.push_str(&format!("grpc_proxy_requests_total {}\n", snapshot.request_metrics.total_requests));
    
    output.push_str("# HELP grpc_proxy_requests_active Current active requests\n");
    output.push_str("# TYPE grpc_proxy_requests_active gauge\n");
    output.push_str(&format!("grpc_proxy_requests_active {}\n", snapshot.request_metrics.active_requests));
    
    output.push_str("# HELP grpc_proxy_request_duration_seconds Total request duration\n");
    output.push_str("# TYPE grpc_proxy_request_duration_seconds counter\n");
    output.push_str(&format!("grpc_proxy_request_duration_seconds {}\n", snapshot.request_metrics.total_request_duration.as_secs_f64()));
    
    output.push_str("# HELP grpc_proxy_bytes_sent_total Total bytes sent\n");
    output.push_str("# TYPE grpc_proxy_bytes_sent_total counter\n");
    output.push_str(&format!("grpc_proxy_bytes_sent_total {}\n", snapshot.request_metrics.total_bytes_sent));
    
    output.push_str("# HELP grpc_proxy_bytes_received_total Total bytes received\n");
    output.push_str("# TYPE grpc_proxy_bytes_received_total counter\n");
    output.push_str(&format!("grpc_proxy_bytes_received_total {}\n", snapshot.request_metrics.total_bytes_received));
    
    // Response code metrics
    for (code, count) in &snapshot.request_metrics.response_codes {
        output.push_str(&format!("grpc_proxy_responses_total{{code=\"{}\"}} {}\n", code, count));
    }
    
    // Latency bucket metrics
    output.push_str("# HELP grpc_proxy_request_latency_bucket Request latency buckets\n");
    output.push_str("# TYPE grpc_proxy_request_latency_bucket histogram\n");
    output.push_str(&format!("grpc_proxy_request_latency_bucket{{le=\"0.01\"}} {}\n", snapshot.request_metrics.latency_buckets.under_10ms));
    output.push_str(&format!("grpc_proxy_request_latency_bucket{{le=\"0.05\"}} {}\n", snapshot.request_metrics.latency_buckets.under_50ms));
    output.push_str(&format!("grpc_proxy_request_latency_bucket{{le=\"0.1\"}} {}\n", snapshot.request_metrics.latency_buckets.under_100ms));
    output.push_str(&format!("grpc_proxy_request_latency_bucket{{le=\"0.5\"}} {}\n", snapshot.request_metrics.latency_buckets.under_500ms));
    output.push_str(&format!("grpc_proxy_request_latency_bucket{{le=\"1.0\"}} {}\n", snapshot.request_metrics.latency_buckets.under_1s));
    output.push_str(&format!("grpc_proxy_request_latency_bucket{{le=\"+Inf\"}} {}\n", snapshot.request_metrics.latency_buckets.over_1s));
    
    // TLS metrics
    output.push_str("# HELP grpc_proxy_tls_handshakes_total Total TLS handshakes\n");
    output.push_str("# TYPE grpc_proxy_tls_handshakes_total counter\n");
    output.push_str(&format!("grpc_proxy_tls_handshakes_total {}\n", snapshot.tls_metrics.total_handshakes));
    
    output.push_str("# HELP grpc_proxy_tls_handshakes_successful_total Successful TLS handshakes\n");
    output.push_str("# TYPE grpc_proxy_tls_handshakes_successful_total counter\n");
    output.push_str(&format!("grpc_proxy_tls_handshakes_successful_total {}\n", snapshot.tls_metrics.successful_handshakes));
    
    output.push_str("# HELP grpc_proxy_tls_handshakes_failed_total Failed TLS handshakes\n");
    output.push_str("# TYPE grpc_proxy_tls_handshakes_failed_total counter\n");
    output.push_str(&format!("grpc_proxy_tls_handshakes_failed_total {}\n", snapshot.tls_metrics.failed_handshakes));
    
    // Error metrics
    output.push_str("# HELP grpc_proxy_errors_total Total errors\n");
    output.push_str("# TYPE grpc_proxy_errors_total counter\n");
    output.push_str(&format!("grpc_proxy_errors_total {}\n", snapshot.error_metrics.total_errors));
    
    for (error_type, count) in &snapshot.error_metrics.errors_by_type {
        output.push_str(&format!("grpc_proxy_errors_by_type_total{{type=\"{}\"}} {}\n", error_type, count));
    }
    
    // Upstream metrics
    for (addr, stats) in &snapshot.upstream_metrics.upstream_stats {
        let addr_label = addr.to_string().replace(':', "_");
        output.push_str(&format!("grpc_proxy_upstream_connections_successful_total{{upstream=\"{}\"}} {}\n", addr_label, stats.successful_connections));
        output.push_str(&format!("grpc_proxy_upstream_connections_failed_total{{upstream=\"{}\"}} {}\n", addr_label, stats.failed_connections));
        output.push_str(&format!("grpc_proxy_upstream_health_checks_total{{upstream=\"{}\"}} {}\n", addr_label, stats.health_checks_total));
        output.push_str(&format!("grpc_proxy_upstream_health_checks_successful_total{{upstream=\"{}\"}} {}\n", addr_label, stats.health_checks_successful));
    }
    
    output
}

/// Get current timestamp in seconds since Unix epoch
fn current_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Get uptime in seconds (placeholder implementation)
fn get_uptime_seconds() -> u64 {
    // In a real implementation, this would track actual uptime
    // For now, we'll use a placeholder
    0
}

/// Health check response
#[derive(Debug, Serialize, Deserialize)]
struct HealthResponse {
    status: String,
    timestamp: u64,
    checks: Vec<HealthCheck>,
}

impl HealthResponse {
    fn from_status(health_status: HealthStatus) -> Self {
        match health_status {
            HealthStatus::Healthy => Self {
                status: "healthy".to_string(),
                timestamp: current_timestamp(),
                checks: vec![
                    HealthCheck {
                        name: "overall".to_string(),
                        status: "pass".to_string(),
                        message: None,
                    }
                ],
            },
            HealthStatus::Degraded { issues } => Self {
                status: "degraded".to_string(),
                timestamp: current_timestamp(),
                checks: issues.into_iter().map(|issue| HealthCheck {
                    name: "overall".to_string(),
                    status: "warn".to_string(),
                    message: Some(issue),
                }).collect(),
            },
        }
    }
}

/// Individual health check
#[derive(Debug, Serialize, Deserialize)]
struct HealthCheck {
    name: String,
    status: String,
    message: Option<String>,
}

/// Readiness response
#[derive(Debug, Serialize, Deserialize)]
struct ReadinessResponse {
    status: String,
    timestamp: u64,
}

/// Liveness response
#[derive(Debug, Serialize, Deserialize)]
struct LivenessResponse {
    status: String,
    timestamp: u64,
    uptime_seconds: u64,
}

/// Detailed status response
#[derive(Debug, Serialize, Deserialize)]
struct StatusResponse {
    status: String,
    timestamp: u64,
    uptime_seconds: u64,
    version: String,
    connections: ConnectionStatus,
    requests: RequestStatus,
    upstreams: Vec<UpstreamStatus>,
    errors: ErrorStatus,
}

/// Connection status summary
#[derive(Debug, Serialize, Deserialize)]
struct ConnectionStatus {
    total: u64,
    active: u64,
    tls: u64,
    plain: u64,
}

/// Request status summary
#[derive(Debug, Serialize, Deserialize)]
struct RequestStatus {
    total: u64,
    active: u64,
    average_duration_ms: Option<u64>,
}

/// Upstream status summary
#[derive(Debug, Serialize, Deserialize)]
struct UpstreamStatus {
    address: SocketAddr,
    healthy: bool,
    total_connections: u64,
    successful_connections: u64,
    failed_connections: u64,
    consecutive_failures: u32,
}

/// Error status summary
#[derive(Debug, Serialize, Deserialize)]
struct ErrorStatus {
    total: u64,
    by_type: HashMap<String, u64>,
}

#[cfg(test)]
mod tests {
    use super::*;
    // Test imports would go here if needed

    #[tokio::test]
    async fn test_health_server_creation() {
        let bind_addr: SocketAddr = "127.0.0.1:0".parse().unwrap();
        let server = HealthServer::new(bind_addr);
        assert_eq!(server.bind_addr, bind_addr);
    }

    #[test]
    fn test_prometheus_metrics_formatting() {
        let snapshot = crate::metrics::MetricsCollector::new().get_metrics_snapshot();
        let prometheus_output = format_prometheus_metrics(&snapshot);
        
        assert!(prometheus_output.contains("grpc_proxy_connections_total"));
        assert!(prometheus_output.contains("grpc_proxy_requests_total"));
        assert!(prometheus_output.contains("grpc_proxy_errors_total"));
    }

    #[test]
    fn test_health_response_creation() {
        let healthy_response = HealthResponse::from_status(HealthStatus::Healthy);
        assert_eq!(healthy_response.status, "healthy");
        assert_eq!(healthy_response.checks.len(), 1);
        
        let degraded_response = HealthResponse::from_status(HealthStatus::Degraded {
            issues: vec!["High error rate".to_string()],
        });
        assert_eq!(degraded_response.status, "degraded");
        assert_eq!(degraded_response.checks.len(), 1);
    }

    #[test]
    fn test_current_timestamp() {
        let timestamp = current_timestamp();
        assert!(timestamp > 0);
        
        // Should be reasonably recent (within last year)
        let one_year_ago = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() - (365 * 24 * 60 * 60);
        assert!(timestamp > one_year_ago);
    }
}