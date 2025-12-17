//! Health checking and circuit breaker module
//! 
//! Provides upstream health monitoring and circuit breaker pattern implementation

use crate::config::UpstreamConfig;
use crate::error::ProxyError;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tokio::time::timeout;
use tracing::{debug, info, warn};

/// Health status of an upstream server
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HealthStatus {
    Healthy,
    Unhealthy,
    Unknown,
}

/// Circuit breaker state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CircuitBreakerState {
    Closed,   // Normal operation
    Open,     // Failing, rejecting requests
    HalfOpen, // Testing if service recovered
}

/// Health check result
#[derive(Debug, Clone)]
pub struct HealthCheckResult {
    pub status: HealthStatus,
    pub response_time: Duration,
    pub error: Option<String>,
    pub timestamp: Instant,
}

/// Circuit breaker configuration
#[derive(Debug, Clone)]
pub struct CircuitBreakerConfig {
    /// Number of failures before opening the circuit
    pub failure_threshold: u32,
    /// Time to wait before transitioning from Open to HalfOpen
    pub recovery_timeout: Duration,
    /// Number of successful requests needed to close the circuit from HalfOpen
    pub success_threshold: u32,
    /// Timeout for individual requests
    pub request_timeout: Duration,
}

impl Default for CircuitBreakerConfig {
    fn default() -> Self {
        Self {
            failure_threshold: 5,
            recovery_timeout: Duration::from_secs(30),
            success_threshold: 3,
            request_timeout: Duration::from_secs(5),
        }
    }
}

/// Circuit breaker state tracking
#[derive(Debug)]
struct CircuitBreakerStateTracker {
    state: CircuitBreakerState,
    failure_count: u32,
    success_count: u32,
    last_failure_time: Option<Instant>,
    last_success_time: Option<Instant>,
    config: CircuitBreakerConfig,
}

impl CircuitBreakerStateTracker {
    fn new(config: CircuitBreakerConfig) -> Self {
        Self {
            state: CircuitBreakerState::Closed,
            failure_count: 0,
            success_count: 0,
            last_failure_time: None,
            last_success_time: None,
            config,
        }
    }

    /// Record a successful request
    fn record_success(&mut self) {
        self.last_success_time = Some(Instant::now());
        
        match self.state {
            CircuitBreakerState::Closed => {
                // Reset failure count on success
                self.failure_count = 0;
            }
            CircuitBreakerState::HalfOpen => {
                self.success_count += 1;
                if self.success_count >= self.config.success_threshold {
                    // Transition to Closed
                    self.state = CircuitBreakerState::Closed;
                    self.failure_count = 0;
                    self.success_count = 0;
                    debug!("Circuit breaker transitioned to Closed state");
                }
            }
            CircuitBreakerState::Open => {
                // Should not happen, but reset if it does
                warn!("Received success while circuit breaker is Open");
            }
        }
    }

    /// Record a failed request
    fn record_failure(&mut self) {
        self.last_failure_time = Some(Instant::now());
        
        match self.state {
            CircuitBreakerState::Closed => {
                self.failure_count += 1;
                if self.failure_count >= self.config.failure_threshold {
                    // Transition to Open
                    self.state = CircuitBreakerState::Open;
                    debug!("Circuit breaker transitioned to Open state after {} failures", self.failure_count);
                }
            }
            CircuitBreakerState::HalfOpen => {
                // Transition back to Open
                self.state = CircuitBreakerState::Open;
                self.success_count = 0;
                debug!("Circuit breaker transitioned back to Open state from HalfOpen");
            }
            CircuitBreakerState::Open => {
                // Already open, just update failure time
            }
        }
    }

    /// Check if requests should be allowed
    fn should_allow_request(&mut self) -> bool {
        match self.state {
            CircuitBreakerState::Closed => true,
            CircuitBreakerState::HalfOpen => true,
            CircuitBreakerState::Open => {
                // Check if we should transition to HalfOpen
                if let Some(last_failure) = self.last_failure_time {
                    if last_failure.elapsed() >= self.config.recovery_timeout {
                        self.state = CircuitBreakerState::HalfOpen;
                        self.success_count = 0;
                        debug!("Circuit breaker transitioned to HalfOpen state");
                        return true;
                    }
                }
                false
            }
        }
    }

    /// Get current state
    fn get_state(&self) -> CircuitBreakerState {
        self.state
    }
}

/// Health checker for upstream servers
pub struct HealthChecker {
    /// Health check results for each upstream
    health_status: Arc<RwLock<HashMap<String, HealthCheckResult>>>,
    /// Circuit breaker states for each upstream
    circuit_breakers: Arc<RwLock<HashMap<String, CircuitBreakerStateTracker>>>,
    /// Default circuit breaker configuration
    default_circuit_config: CircuitBreakerConfig,
    /// Health check interval
    check_interval: Duration,
}

impl HealthChecker {
    /// Create a new health checker
    pub fn new() -> Self {
        Self {
            health_status: Arc::new(RwLock::new(HashMap::new())),
            circuit_breakers: Arc::new(RwLock::new(HashMap::new())),
            default_circuit_config: CircuitBreakerConfig::default(),
            check_interval: Duration::from_secs(30),
        }
    }

    /// Create a new health checker with custom configuration
    pub fn with_config(
        circuit_config: CircuitBreakerConfig,
        check_interval: Duration,
    ) -> Self {
        Self {
            health_status: Arc::new(RwLock::new(HashMap::new())),
            circuit_breakers: Arc::new(RwLock::new(HashMap::new())),
            default_circuit_config: circuit_config,
            check_interval,
        }
    }

    /// Get upstream key for tracking
    fn upstream_key(upstream: &UpstreamConfig) -> String {
        format!("{}:{}", upstream.host, upstream.port)
    }

    /// Start health checking for an upstream server
    pub async fn start_health_checking(&self, upstream: UpstreamConfig) {
        let key = Self::upstream_key(&upstream);
        
        // Initialize circuit breaker if not exists
        {
            let mut breakers = self.circuit_breakers.write().await;
            breakers.entry(key.clone()).or_insert_with(|| {
                CircuitBreakerStateTracker::new(self.default_circuit_config.clone())
            });
        }

        // Initialize health status
        {
            let mut status = self.health_status.write().await;
            status.entry(key.clone()).or_insert_with(|| HealthCheckResult {
                status: HealthStatus::Unknown,
                response_time: Duration::from_millis(0),
                error: None,
                timestamp: Instant::now(),
            });
        }

        // Spawn health check task
        let health_status = Arc::clone(&self.health_status);
        let check_interval = self.check_interval;
        let key_for_task = key.clone();
        
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(check_interval);
            
            loop {
                interval.tick().await;
                
                let result = Self::perform_health_check(&upstream).await;
                
                // Update health status
                {
                    let mut status = health_status.write().await;
                    status.insert(key_for_task.clone(), result.clone());
                }

                debug!("Health check for {}: {:?}", key_for_task, result.status);
                
                if result.status == HealthStatus::Unhealthy {
                    if let Some(error) = &result.error {
                        warn!("Health check failed for {}: {}", key_for_task, error);
                    }
                }
            }
        });

        info!("Started health checking for upstream: {}", key);
    }

    /// Perform a health check on an upstream server
    async fn perform_health_check(upstream: &UpstreamConfig) -> HealthCheckResult {
        let start_time = Instant::now();
        let address = format!("{}:{}", upstream.host, upstream.port);
        
        // Simple TCP connection test
        let result = timeout(
            Duration::from_secs(5),
            tokio::net::TcpStream::connect(&address)
        ).await;
        
        let response_time = start_time.elapsed();
        
        match result {
            Ok(Ok(_stream)) => {
                HealthCheckResult {
                    status: HealthStatus::Healthy,
                    response_time,
                    error: None,
                    timestamp: Instant::now(),
                }
            }
            Ok(Err(e)) => {
                HealthCheckResult {
                    status: HealthStatus::Unhealthy,
                    response_time,
                    error: Some(format!("Connection failed: {}", e)),
                    timestamp: Instant::now(),
                }
            }
            Err(_) => {
                HealthCheckResult {
                    status: HealthStatus::Unhealthy,
                    response_time,
                    error: Some("Health check timeout".to_string()),
                    timestamp: Instant::now(),
                }
            }
        }
    }

    /// Check if a request should be allowed through the circuit breaker
    pub async fn should_allow_request(&self, upstream: &UpstreamConfig) -> Result<(), ProxyError> {
        let key = Self::upstream_key(upstream);
        
        let mut breakers = self.circuit_breakers.write().await;
        let breaker = breakers.entry(key.clone()).or_insert_with(|| {
            CircuitBreakerStateTracker::new(self.default_circuit_config.clone())
        });
        
        if breaker.should_allow_request() {
            Ok(())
        } else {
            Err(ProxyError::circuit_breaker_open(key))
        }
    }

    /// Record a successful request for circuit breaker tracking
    pub async fn record_success(&self, upstream: &UpstreamConfig) {
        let key = Self::upstream_key(upstream);
        
        let mut breakers = self.circuit_breakers.write().await;
        if let Some(breaker) = breakers.get_mut(&key) {
            breaker.record_success();
        }
    }

    /// Record a failed request for circuit breaker tracking
    pub async fn record_failure(&self, upstream: &UpstreamConfig) {
        let key = Self::upstream_key(upstream);
        
        let mut breakers = self.circuit_breakers.write().await;
        if let Some(breaker) = breakers.get_mut(&key) {
            breaker.record_failure();
        }
    }

    /// Get current health status of an upstream server
    pub async fn get_health_status(&self, upstream: &UpstreamConfig) -> Option<HealthCheckResult> {
        let key = Self::upstream_key(upstream);
        let status = self.health_status.read().await;
        status.get(&key).cloned()
    }

    /// Get current circuit breaker state
    pub async fn get_circuit_breaker_state(&self, upstream: &UpstreamConfig) -> CircuitBreakerState {
        let key = Self::upstream_key(upstream);
        let breakers = self.circuit_breakers.read().await;
        breakers.get(&key)
            .map(|b| b.get_state())
            .unwrap_or(CircuitBreakerState::Closed)
    }

    /// Check if an upstream is healthy
    pub async fn is_healthy(&self, upstream: &UpstreamConfig) -> bool {
        if let Some(health) = self.get_health_status(upstream).await {
            // Consider healthy if last check was successful and recent
            health.status == HealthStatus::Healthy && 
            health.timestamp.elapsed() < Duration::from_secs(60)
        } else {
            // Unknown status, assume healthy for new upstreams
            true
        }
    }

    /// Get health statistics for monitoring
    pub async fn get_health_stats(&self) -> HealthStats {
        let status = self.health_status.read().await;
        let breakers = self.circuit_breakers.read().await;
        
        let mut healthy_count = 0;
        let mut unhealthy_count = 0;
        let mut unknown_count = 0;
        let mut circuit_breaker_open_count = 0;
        
        for result in status.values() {
            match result.status {
                HealthStatus::Healthy => healthy_count += 1,
                HealthStatus::Unhealthy => unhealthy_count += 1,
                HealthStatus::Unknown => unknown_count += 1,
            }
        }
        
        for breaker in breakers.values() {
            if breaker.get_state() == CircuitBreakerState::Open {
                circuit_breaker_open_count += 1;
            }
        }
        
        HealthStats {
            total_upstreams: status.len(),
            healthy_count,
            unhealthy_count,
            unknown_count,
            circuit_breaker_open_count,
        }
    }

    /// Stop health checking (cleanup)
    pub async fn stop_health_checking(&self, upstream: &UpstreamConfig) {
        let key = Self::upstream_key(upstream);
        
        // Remove from tracking
        {
            let mut status = self.health_status.write().await;
            status.remove(&key);
        }
        
        {
            let mut breakers = self.circuit_breakers.write().await;
            breakers.remove(&key);
        }
        
        info!("Stopped health checking for upstream: {}", key);
    }
}

/// Health statistics for monitoring
#[derive(Debug, Clone)]
pub struct HealthStats {
    pub total_upstreams: usize,
    pub healthy_count: usize,
    pub unhealthy_count: usize,
    pub unknown_count: usize,
    pub circuit_breaker_open_count: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn create_test_upstream(host: &str, port: u16) -> UpstreamConfig {
        UpstreamConfig {
            host: host.to_string(),
            port,
            timeout: Duration::from_secs(30),
            max_connections: 100,
        }
    }

    #[test]
    fn test_circuit_breaker_state_transitions() {
        let config = CircuitBreakerConfig {
            failure_threshold: 3,
            recovery_timeout: Duration::from_millis(100),
            success_threshold: 2,
            request_timeout: Duration::from_secs(5),
        };
        
        let mut breaker = CircuitBreakerStateTracker::new(config);
        
        // Initially closed
        assert_eq!(breaker.get_state(), CircuitBreakerState::Closed);
        assert!(breaker.should_allow_request());
        
        // Record failures to open circuit
        breaker.record_failure();
        assert_eq!(breaker.get_state(), CircuitBreakerState::Closed);
        
        breaker.record_failure();
        assert_eq!(breaker.get_state(), CircuitBreakerState::Closed);
        
        breaker.record_failure();
        assert_eq!(breaker.get_state(), CircuitBreakerState::Open);
        assert!(!breaker.should_allow_request());
        
        // Wait for recovery timeout
        std::thread::sleep(Duration::from_millis(150));
        
        // Should transition to half-open
        assert!(breaker.should_allow_request());
        assert_eq!(breaker.get_state(), CircuitBreakerState::HalfOpen);
        
        // Record success to close circuit
        breaker.record_success();
        assert_eq!(breaker.get_state(), CircuitBreakerState::HalfOpen);
        
        breaker.record_success();
        assert_eq!(breaker.get_state(), CircuitBreakerState::Closed);
    }

    #[test]
    fn test_circuit_breaker_half_open_failure() {
        let config = CircuitBreakerConfig {
            failure_threshold: 2,
            recovery_timeout: Duration::from_millis(50),
            success_threshold: 2,
            request_timeout: Duration::from_secs(5),
        };
        
        let mut breaker = CircuitBreakerStateTracker::new(config);
        
        // Open the circuit
        breaker.record_failure();
        breaker.record_failure();
        assert_eq!(breaker.get_state(), CircuitBreakerState::Open);
        
        // Wait and transition to half-open
        std::thread::sleep(Duration::from_millis(100));
        assert!(breaker.should_allow_request());
        assert_eq!(breaker.get_state(), CircuitBreakerState::HalfOpen);
        
        // Failure in half-open should go back to open
        breaker.record_failure();
        assert_eq!(breaker.get_state(), CircuitBreakerState::Open);
    }

    #[tokio::test]
    async fn test_health_checker_creation() {
        let checker = HealthChecker::new();
        let stats = checker.get_health_stats().await;
        
        assert_eq!(stats.total_upstreams, 0);
        assert_eq!(stats.healthy_count, 0);
        assert_eq!(stats.unhealthy_count, 0);
    }

    #[tokio::test]
    async fn test_upstream_key_generation() {
        let upstream = create_test_upstream("localhost", 9090);
        let key = HealthChecker::upstream_key(&upstream);
        assert_eq!(key, "localhost:9090");
    }

    #[tokio::test]
    async fn test_circuit_breaker_allow_request() {
        let checker = HealthChecker::new();
        let upstream = create_test_upstream("localhost", 9090);
        
        // Should allow request initially
        assert!(checker.should_allow_request(&upstream).await.is_ok());
        
        // Record failures to open circuit
        checker.record_failure(&upstream).await;
        checker.record_failure(&upstream).await;
        checker.record_failure(&upstream).await;
        checker.record_failure(&upstream).await;
        checker.record_failure(&upstream).await;
        
        // Should reject request when circuit is open
        assert!(checker.should_allow_request(&upstream).await.is_err());
        
        let state = checker.get_circuit_breaker_state(&upstream).await;
        assert_eq!(state, CircuitBreakerState::Open);
    }

    #[tokio::test]
    async fn test_health_status_tracking() {
        let checker = HealthChecker::new();
        let upstream = create_test_upstream("localhost", 9090);
        
        // Initially no health status
        assert!(checker.get_health_status(&upstream).await.is_none());
        
        // Should assume healthy for unknown upstreams
        assert!(checker.is_healthy(&upstream).await);
    }

    #[tokio::test]
    async fn test_health_stats() {
        let checker = HealthChecker::new();
        let upstream1 = create_test_upstream("localhost", 9090);
        let upstream2 = create_test_upstream("localhost", 9091);
        
        // Start health checking (this initializes the tracking)
        checker.start_health_checking(upstream1.clone()).await;
        checker.start_health_checking(upstream2.clone()).await;
        
        // Give it a moment to initialize
        tokio::time::sleep(Duration::from_millis(10)).await;
        
        let stats = checker.get_health_stats().await;
        assert_eq!(stats.total_upstreams, 2);
        
        // Clean up
        checker.stop_health_checking(&upstream1).await;
        checker.stop_health_checking(&upstream2).await;
    }

    #[tokio::test]
    async fn test_perform_health_check_invalid_address() {
        let upstream = create_test_upstream("invalid-host-that-does-not-exist", 9999);
        let result = HealthChecker::perform_health_check(&upstream).await;
        
        assert_eq!(result.status, HealthStatus::Unhealthy);
        assert!(result.error.is_some());
    }

    #[tokio::test]
    async fn test_circuit_breaker_error_scenarios() {
        let checker = HealthChecker::new();
        let upstream = create_test_upstream("localhost", 9999); // Non-existent port
        
        // Initially should allow requests
        assert!(checker.should_allow_request(&upstream).await.is_ok());
        
        // Simulate multiple failures
        for i in 0..5 {
            checker.record_failure(&upstream).await;
            
            if i < 4 {
                // Should still allow requests before threshold
                assert!(checker.should_allow_request(&upstream).await.is_ok());
            } else {
                // Should reject after threshold
                let result = checker.should_allow_request(&upstream).await;
                assert!(result.is_err());
                
                if let Err(ProxyError::CircuitBreakerOpen { address }) = result {
                    assert_eq!(address, "localhost:9999");
                } else {
                    panic!("Expected CircuitBreakerOpen error");
                }
            }
        }
        
        // Verify circuit breaker state
        let state = checker.get_circuit_breaker_state(&upstream).await;
        assert_eq!(state, CircuitBreakerState::Open);
    }

    #[tokio::test]
    async fn test_circuit_breaker_recovery() {
        let config = CircuitBreakerConfig {
            failure_threshold: 2,
            recovery_timeout: Duration::from_millis(50),
            success_threshold: 1,
            request_timeout: Duration::from_secs(5),
        };
        
        let checker = HealthChecker::with_config(config, Duration::from_secs(30));
        let upstream = create_test_upstream("localhost", 9998);
        
        // Initialize circuit breaker by checking if request is allowed
        assert!(checker.should_allow_request(&upstream).await.is_ok());
        
        // Open the circuit
        checker.record_failure(&upstream).await;
        checker.record_failure(&upstream).await;
        
        // Should be open
        assert!(checker.should_allow_request(&upstream).await.is_err());
        assert_eq!(checker.get_circuit_breaker_state(&upstream).await, CircuitBreakerState::Open);
        
        // Wait for recovery timeout
        tokio::time::sleep(Duration::from_millis(100)).await;
        
        // Should transition to half-open and allow request
        assert!(checker.should_allow_request(&upstream).await.is_ok());
        assert_eq!(checker.get_circuit_breaker_state(&upstream).await, CircuitBreakerState::HalfOpen);
        
        // Record success to close circuit
        checker.record_success(&upstream).await;
        assert_eq!(checker.get_circuit_breaker_state(&upstream).await, CircuitBreakerState::Closed);
        
        // Should allow requests normally
        assert!(checker.should_allow_request(&upstream).await.is_ok());
    }



    #[tokio::test]
    async fn test_health_status_tracking_over_time() {
        let checker = HealthChecker::new();
        let upstream = create_test_upstream("localhost", 9996);
        
        // Start health checking
        checker.start_health_checking(upstream.clone()).await;
        
        // Give it a moment to perform initial check
        tokio::time::sleep(Duration::from_millis(100)).await;
        
        // Should have health status now
        let status = checker.get_health_status(&upstream).await;
        assert!(status.is_some());
        
        let status = status.unwrap();
        // Should be unhealthy since port doesn't exist
        assert_eq!(status.status, HealthStatus::Unhealthy);
        assert!(status.error.is_some());
        
        // Stop health checking
        checker.stop_health_checking(&upstream).await;
        
        // Should no longer have health status
        let status = checker.get_health_status(&upstream).await;
        assert!(status.is_none());
    }

    #[tokio::test]
    async fn test_health_stats_with_multiple_upstreams() {
        let checker = HealthChecker::new();
        let upstream1 = create_test_upstream("localhost", 9995);
        let upstream2 = create_test_upstream("localhost", 9994);
        let upstream3 = create_test_upstream("localhost", 9993);
        
        // Start health checking for multiple upstreams
        checker.start_health_checking(upstream1.clone()).await;
        checker.start_health_checking(upstream2.clone()).await;
        checker.start_health_checking(upstream3.clone()).await;
        
        // Give time for initial health checks
        tokio::time::sleep(Duration::from_millis(100)).await;
        
        // Open circuit breaker for one upstream
        for _ in 0..5 {
            checker.record_failure(&upstream1).await;
        }
        
        let stats = checker.get_health_stats().await;
        assert_eq!(stats.total_upstreams, 3);
        assert_eq!(stats.circuit_breaker_open_count, 1);
        
        // All should be unhealthy since ports don't exist
        assert_eq!(stats.unhealthy_count, 3);
        assert_eq!(stats.healthy_count, 0);
        
        // Clean up
        checker.stop_health_checking(&upstream1).await;
        checker.stop_health_checking(&upstream2).await;
        checker.stop_health_checking(&upstream3).await;
    }

    #[test]
    fn test_health_status_enum() {
        assert_eq!(HealthStatus::Healthy, HealthStatus::Healthy);
        assert_ne!(HealthStatus::Healthy, HealthStatus::Unhealthy);
        assert_ne!(HealthStatus::Healthy, HealthStatus::Unknown);
        
        // Test debug formatting
        assert_eq!(format!("{:?}", HealthStatus::Healthy), "Healthy");
        assert_eq!(format!("{:?}", HealthStatus::Unhealthy), "Unhealthy");
        assert_eq!(format!("{:?}", HealthStatus::Unknown), "Unknown");
    }

    #[test]
    fn test_circuit_breaker_state_enum() {
        assert_eq!(CircuitBreakerState::Closed, CircuitBreakerState::Closed);
        assert_ne!(CircuitBreakerState::Closed, CircuitBreakerState::Open);
        assert_ne!(CircuitBreakerState::Closed, CircuitBreakerState::HalfOpen);
        
        // Test debug formatting
        assert_eq!(format!("{:?}", CircuitBreakerState::Closed), "Closed");
        assert_eq!(format!("{:?}", CircuitBreakerState::Open), "Open");
        assert_eq!(format!("{:?}", CircuitBreakerState::HalfOpen), "HalfOpen");
    }

    #[test]
    fn test_circuit_breaker_config_default() {
        let config = CircuitBreakerConfig::default();
        assert_eq!(config.failure_threshold, 5);
        assert_eq!(config.recovery_timeout, Duration::from_secs(30));
        assert_eq!(config.success_threshold, 3);
        assert_eq!(config.request_timeout, Duration::from_secs(5));
    }

    #[test]
    fn test_health_check_result_structure() {
        let result = HealthCheckResult {
            status: HealthStatus::Healthy,
            response_time: Duration::from_millis(150),
            error: None,
            timestamp: Instant::now(),
        };
        
        assert_eq!(result.status, HealthStatus::Healthy);
        assert_eq!(result.response_time, Duration::from_millis(150));
        assert!(result.error.is_none());
        
        let result_with_error = HealthCheckResult {
            status: HealthStatus::Unhealthy,
            response_time: Duration::from_millis(5000),
            error: Some("Connection timeout".to_string()),
            timestamp: Instant::now(),
        };
        
        assert_eq!(result_with_error.status, HealthStatus::Unhealthy);
        assert_eq!(result_with_error.error, Some("Connection timeout".to_string()));
    }

    #[test]
    fn test_health_stats_structure() {
        let stats = HealthStats {
            total_upstreams: 5,
            healthy_count: 3,
            unhealthy_count: 1,
            unknown_count: 1,
            circuit_breaker_open_count: 2,
        };
        
        assert_eq!(stats.total_upstreams, 5);
        assert_eq!(stats.healthy_count, 3);
        assert_eq!(stats.unhealthy_count, 1);
        assert_eq!(stats.unknown_count, 1);
        assert_eq!(stats.circuit_breaker_open_count, 2);
        
        // Test debug formatting
        let debug_str = format!("{:?}", stats);
        assert!(debug_str.contains("total_upstreams: 5"));
        assert!(debug_str.contains("healthy_count: 3"));
    }
}