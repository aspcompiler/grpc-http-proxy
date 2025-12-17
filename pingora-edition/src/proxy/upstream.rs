//! Upstream server management

use anyhow::{anyhow, Result};
use crate::config::{UpstreamConfig, HealthCheckConfig};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};
use tokio::time::timeout;

/// Upstream manager handles connections to backend gRPC servers
pub struct UpstreamManager {
    upstreams: HashMap<SocketAddr, UpstreamEntry>,
    health_states: Arc<RwLock<HashMap<SocketAddr, HealthState>>>,
}

/// Internal upstream entry with connection pool and configuration
struct UpstreamEntry {
    config: UpstreamConfig,
    connection_pool: ConnectionPool,
}

/// Connection pool for managing connections to a single upstream
struct ConnectionPool {
    max_size: usize,
    current_connections: Arc<RwLock<usize>>,
    timeout: Option<Duration>,
}

/// Health state for an upstream server
#[derive(Debug, Clone)]
pub struct HealthState {
    pub is_healthy: bool,
    pub last_check: Instant,
    pub consecutive_failures: u32,
    pub last_error: Option<String>,
}

/// Load balancing strategy
#[derive(Debug, Clone)]
pub enum LoadBalancingStrategy {
    RoundRobin,
    LeastConnections,
    Random,
}

impl UpstreamManager {
    /// Create a new upstream manager
    pub fn new(upstreams: Vec<UpstreamConfig>) -> Self {
        let mut upstream_map = HashMap::new();
        let mut health_states = HashMap::new();

        for upstream in upstreams {
            let pool_size = upstream.connection_pool_size.unwrap_or(10);
            let entry = UpstreamEntry {
                config: upstream.clone(),
                connection_pool: ConnectionPool::new(pool_size, upstream.timeout),
            };
            
            // Initialize health state as healthy
            let health_state = HealthState {
                is_healthy: true,
                last_check: Instant::now(),
                consecutive_failures: 0,
                last_error: None,
            };

            upstream_map.insert(upstream.address, entry);
            health_states.insert(upstream.address, health_state);
        }

        Self {
            upstreams: upstream_map,
            health_states: Arc::new(RwLock::new(health_states)),
        }
    }

    /// Get an upstream server for the given configuration
    /// Returns the upstream config if available and healthy
    pub fn get_upstream(&self, config: &UpstreamConfig) -> Result<&UpstreamConfig> {
        // Check if the upstream exists
        let upstream_entry = self.upstreams.get(&config.address)
            .ok_or_else(|| anyhow!("Upstream not found: {}", config.address))?;

        // Check if the upstream is healthy
        if let Ok(health_states) = self.health_states.read() {
            if let Some(health_state) = health_states.get(&config.address) {
                if !health_state.is_healthy {
                    return Err(anyhow!(
                        "Upstream {} is unhealthy: {}",
                        config.address,
                        health_state.last_error.as_deref().unwrap_or("Unknown error")
                    ));
                }
            }
        }

        // Check connection pool availability
        if !upstream_entry.connection_pool.can_acquire_connection() {
            return Err(anyhow!(
                "Connection pool exhausted for upstream: {}",
                config.address
            ));
        }

        Ok(&upstream_entry.config)
    }

    /// Select the best upstream from multiple options using load balancing
    pub fn select_upstream<'a>(&self, candidates: &[&'a UpstreamConfig], strategy: LoadBalancingStrategy) -> Result<&'a UpstreamConfig> {
        if candidates.is_empty() {
            return Err(anyhow!("No upstream candidates provided"));
        }

        // Filter healthy upstreams
        let healthy_candidates: Vec<&UpstreamConfig> = candidates
            .iter()
            .filter(|config| self.is_upstream_healthy(&config.address))
            .copied()
            .collect();

        if healthy_candidates.is_empty() {
            return Err(anyhow!("No healthy upstream servers available"));
        }

        match strategy {
            LoadBalancingStrategy::RoundRobin => {
                // Simple round-robin based on current time
                let index = (Instant::now().elapsed().as_millis() as usize) % healthy_candidates.len();
                Ok(healthy_candidates[index])
            }
            LoadBalancingStrategy::LeastConnections => {
                // Select upstream with least connections
                let mut best_upstream = healthy_candidates[0];
                let mut min_connections = self.get_connection_count(&best_upstream.address);

                for candidate in &healthy_candidates[1..] {
                    let connections = self.get_connection_count(&candidate.address);
                    if connections < min_connections {
                        min_connections = connections;
                        best_upstream = candidate;
                    }
                }
                Ok(best_upstream)
            }
            LoadBalancingStrategy::Random => {
                // Simple random selection based on current time
                let index = (Instant::now().elapsed().as_nanos() as usize) % healthy_candidates.len();
                Ok(healthy_candidates[index])
            }
        }
    }

    /// Acquire a connection from the pool for the given upstream
    pub fn acquire_connection(&self, address: &SocketAddr) -> Result<ConnectionHandle> {
        let upstream_entry = self.upstreams.get(address)
            .ok_or_else(|| anyhow!("Upstream not found: {}", address))?;

        upstream_entry.connection_pool.acquire()
    }

    /// Check if an upstream is healthy
    pub fn is_upstream_healthy(&self, address: &SocketAddr) -> bool {
        if let Ok(health_states) = self.health_states.read() {
            if let Some(health_state) = health_states.get(address) {
                return health_state.is_healthy;
            }
        }
        false
    }

    /// Get current connection count for an upstream
    pub fn get_connection_count(&self, address: &SocketAddr) -> usize {
        if let Some(upstream_entry) = self.upstreams.get(address) {
            upstream_entry.connection_pool.current_count()
        } else {
            0
        }
    }

    /// Get health state for an upstream
    pub fn get_health_state(&self, address: &SocketAddr) -> Option<HealthState> {
        if let Ok(health_states) = self.health_states.read() {
            health_states.get(address).cloned()
        } else {
            None
        }
    }

    /// Check health of all upstream servers
    pub async fn health_check_all(&self) -> Result<()> {
        let upstreams: Vec<_> = self.upstreams.keys().cloned().collect();
        
        for address in upstreams {
            if let Some(upstream_entry) = self.upstreams.get(&address) {
                if let Some(health_config) = &upstream_entry.config.health_check {
                    let result = self.perform_health_check(&address, health_config).await;
                    self.update_health_state(&address, result).await;
                }
            }
        }
        
        Ok(())
    }

    /// Check health of a specific upstream server
    pub async fn health_check(&self, address: &SocketAddr) -> Result<()> {
        let upstream_entry = self.upstreams.get(address)
            .ok_or_else(|| anyhow!("Upstream not found: {}", address))?;

        if let Some(health_config) = &upstream_entry.config.health_check {
            let result = self.perform_health_check(address, health_config).await;
            self.update_health_state(address, result).await;
        }

        Ok(())
    }

    /// Perform actual health check against an upstream
    async fn perform_health_check(&self, address: &SocketAddr, health_config: &HealthCheckConfig) -> Result<()> {
        // Create a simple HTTP client for health checking
        let client = reqwest::Client::new();
        let url = format!("http://{}{}", address, health_config.path);

        // Perform health check with timeout
        let response = timeout(health_config.timeout, client.get(&url).send()).await
            .map_err(|_| anyhow!("Health check timeout"))?
            .map_err(|e| anyhow!("Health check request failed: {}", e))?;

        if response.status().is_success() {
            Ok(())
        } else {
            Err(anyhow!("Health check failed with status: {}", response.status()))
        }
    }

    /// Update health state for an upstream
    async fn update_health_state(&self, address: &SocketAddr, result: Result<()>) {
        if let Ok(mut health_states) = self.health_states.write() {
            if let Some(health_state) = health_states.get_mut(address) {
                health_state.last_check = Instant::now();

                match result {
                    Ok(()) => {
                        health_state.is_healthy = true;
                        health_state.consecutive_failures = 0;
                        health_state.last_error = None;
                    }
                    Err(e) => {
                        health_state.consecutive_failures += 1;
                        health_state.last_error = Some(e.to_string());
                        
                        // Mark as unhealthy after 3 consecutive failures
                        if health_state.consecutive_failures >= 3 {
                            health_state.is_healthy = false;
                        }
                    }
                }
            }
        }
    }

    /// Get statistics for all upstreams
    pub fn get_stats(&self) -> HashMap<SocketAddr, UpstreamStats> {
        let mut stats = HashMap::new();
        
        for (address, upstream_entry) in &self.upstreams {
            let health_state = self.get_health_state(address);
            let connection_count = upstream_entry.connection_pool.current_count();
            
            stats.insert(*address, UpstreamStats {
                address: *address,
                is_healthy: health_state.as_ref().map(|h| h.is_healthy).unwrap_or(false),
                connection_count,
                consecutive_failures: health_state.as_ref().map(|h| h.consecutive_failures).unwrap_or(0),
                last_check: health_state.as_ref().map(|h| h.last_check),
                last_error: health_state.and_then(|h| h.last_error),
            });
        }
        
        stats
    }
}

impl ConnectionPool {
    fn new(max_size: usize, timeout: Option<Duration>) -> Self {
        Self {
            max_size,
            current_connections: Arc::new(RwLock::new(0)),
            timeout,
        }
    }

    fn can_acquire_connection(&self) -> bool {
        if let Ok(count) = self.current_connections.read() {
            *count < self.max_size
        } else {
            false
        }
    }

    fn acquire(&self) -> Result<ConnectionHandle> {
        if let Ok(mut count) = self.current_connections.write() {
            if *count < self.max_size {
                *count += 1;
                Ok(ConnectionHandle {
                    pool: self.current_connections.clone(),
                    timeout: self.timeout,
                })
            } else {
                Err(anyhow!("Connection pool exhausted"))
            }
        } else {
            Err(anyhow!("Failed to acquire connection pool lock"))
        }
    }

    fn current_count(&self) -> usize {
        if let Ok(count) = self.current_connections.read() {
            *count
        } else {
            0
        }
    }
}

/// Handle for a connection from the pool
#[derive(Debug)]
pub struct ConnectionHandle {
    pool: Arc<RwLock<usize>>,
    timeout: Option<Duration>,
}

impl ConnectionHandle {
    /// Get the connection timeout
    pub fn timeout(&self) -> Option<Duration> {
        self.timeout
    }
}

impl Drop for ConnectionHandle {
    fn drop(&mut self) {
        // Return connection to pool
        if let Ok(mut count) = self.pool.write() {
            if *count > 0 {
                *count -= 1;
            }
        }
    }
}

/// Statistics for an upstream server
#[derive(Debug, Clone)]
pub struct UpstreamStats {
    pub address: SocketAddr,
    pub is_healthy: bool,
    pub connection_count: usize,
    pub consecutive_failures: u32,
    pub last_check: Option<Instant>,
    pub last_error: Option<String>,
}