//! Configuration management for the gRPC HTTP proxy

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Duration;

pub mod manager;

pub use manager::ConfigManager;

/// Main proxy configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyConfig {
    pub server: ServerConfig,
    pub tls: Option<TlsConfig>,
    pub routes: Vec<RouteConfig>,
    pub default_upstream: UpstreamConfig,
}

/// Server configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    pub bind_address: SocketAddr,
    pub worker_threads: Option<usize>,
    pub max_connections: Option<usize>,
}

impl ServerConfig {
    /// Validate server configuration parameters
    pub fn validate(&self) -> Result<()> {
        if let Some(threads) = self.worker_threads {
            if threads == 0 {
                return Err(anyhow!("worker_threads must be greater than 0"));
            }
            if threads > 1024 {
                return Err(anyhow!("worker_threads should not exceed 1024"));
            }
        }

        if let Some(connections) = self.max_connections {
            if connections == 0 {
                return Err(anyhow!("max_connections must be greater than 0"));
            }
        }

        Ok(())
    }
}

/// TLS configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TlsConfig {
    pub cert_path: PathBuf,
    pub key_path: PathBuf,
    pub ca_cert_path: Option<PathBuf>, // For mTLS
    // ALPN is hardcoded to ["h2"] to prevent configuration errors
}

impl TlsConfig {
    /// Validate TLS configuration parameters
    pub fn validate(&self) -> Result<()> {
        // Check if certificate file exists and is readable
        if !self.cert_path.exists() {
            return Err(anyhow!("Certificate file does not exist: {:?}", self.cert_path));
        }
        if !self.cert_path.is_file() {
            return Err(anyhow!("Certificate path is not a file: {:?}", self.cert_path));
        }

        // Check if private key file exists and is readable
        if !self.key_path.exists() {
            return Err(anyhow!("Private key file does not exist: {:?}", self.key_path));
        }
        if !self.key_path.is_file() {
            return Err(anyhow!("Private key path is not a file: {:?}", self.key_path));
        }

        // Check CA certificate if provided (for mTLS)
        if let Some(ca_path) = &self.ca_cert_path {
            if !ca_path.exists() {
                return Err(anyhow!("CA certificate file does not exist: {:?}", ca_path));
            }
            if !ca_path.is_file() {
                return Err(anyhow!("CA certificate path is not a file: {:?}", ca_path));
            }
        }

        Ok(())
    }
}

/// Route configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteConfig {
    pub path_pattern: String,          // e.g., "/api/v1/*"
    pub upstream: UpstreamConfig,
    pub priority: Option<u32>,         // For conflict resolution
}

impl RouteConfig {
    /// Validate route configuration parameters
    pub fn validate(&self) -> Result<()> {
        // Validate path pattern
        if self.path_pattern.is_empty() {
            return Err(anyhow!("path_pattern cannot be empty"));
        }

        // Path pattern should start with '/'
        if !self.path_pattern.starts_with('/') {
            return Err(anyhow!("path_pattern must start with '/': {}", self.path_pattern));
        }

        // Validate wildcard usage - only allow '*' at the end
        if let Some(star_pos) = self.path_pattern.find('*') {
            if star_pos != self.path_pattern.len() - 1 {
                return Err(anyhow!("wildcard '*' can only appear at the end of path_pattern: {}", self.path_pattern));
            }
        }

        // Validate upstream configuration
        self.upstream.validate()?;

        Ok(())
    }
}

/// Upstream server configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpstreamConfig {
    pub address: SocketAddr,
    pub connection_pool_size: Option<usize>,
    pub health_check: Option<HealthCheckConfig>,
    #[serde(with = "humantime_serde", default)]
    pub timeout: Option<Duration>,
}

impl UpstreamConfig {
    /// Validate upstream configuration parameters
    pub fn validate(&self) -> Result<()> {
        // Validate connection pool size
        if let Some(pool_size) = self.connection_pool_size {
            if pool_size == 0 {
                return Err(anyhow!("connection_pool_size must be greater than 0"));
            }
            if pool_size > 10000 {
                return Err(anyhow!("connection_pool_size should not exceed 10000"));
            }
        }

        // Validate timeout
        if let Some(timeout) = self.timeout {
            if timeout.is_zero() {
                return Err(anyhow!("timeout must be greater than 0"));
            }
            if timeout > Duration::from_secs(3600) {
                return Err(anyhow!("timeout should not exceed 1 hour"));
            }
        }

        // Validate health check configuration if present
        if let Some(health_check) = &self.health_check {
            health_check.validate()?;
        }

        Ok(())
    }
}

/// Health check configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthCheckConfig {
    pub path: String,
    #[serde(with = "humantime_serde")]
    pub interval: Duration,
    #[serde(with = "humantime_serde")]
    pub timeout: Duration,
}

impl HealthCheckConfig {
    /// Validate health check configuration parameters
    pub fn validate(&self) -> Result<()> {
        // Validate health check path
        if self.path.is_empty() {
            return Err(anyhow!("health check path cannot be empty"));
        }
        if !self.path.starts_with('/') {
            return Err(anyhow!("health check path must start with '/': {}", self.path));
        }

        // Validate interval
        if self.interval.is_zero() {
            return Err(anyhow!("health check interval must be greater than 0"));
        }
        if self.interval < Duration::from_secs(1) {
            return Err(anyhow!("health check interval should be at least 1 second"));
        }

        // Validate timeout
        if self.timeout.is_zero() {
            return Err(anyhow!("health check timeout must be greater than 0"));
        }
        if self.timeout >= self.interval {
            return Err(anyhow!("health check timeout must be less than interval"));
        }

        Ok(())
    }
}

impl ProxyConfig {
    /// Load configuration from a file
    pub fn load(path: &str) -> Result<Self> {
        // Use ConfigManager for comprehensive loading and validation
        let manager = ConfigManager::new(path.to_string())?;
        Ok(manager.config().clone())
    }

    /// Validate the configuration
    pub fn validate(&self) -> Result<()> {
        // Validate server configuration
        self.server.validate()?;

        // Validate TLS configuration if present
        if let Some(tls) = &self.tls {
            tls.validate()?;
        }

        // Validate default upstream
        self.default_upstream.validate()?;

        // Validate all route configurations
        for route in &self.routes {
            route.validate()?;
        }

        // Check for route pattern conflicts
        self.validate_route_conflicts()?;

        Ok(())
    }

    /// Validate that route patterns don't conflict
    fn validate_route_conflicts(&self) -> Result<()> {
        let mut patterns = HashSet::new();
        let mut priorities = std::collections::HashMap::new();

        for route in &self.routes {
            // Check for exact pattern duplicates
            if patterns.contains(&route.path_pattern) {
                return Err(anyhow!("Duplicate route pattern found: {}", route.path_pattern));
            }
            patterns.insert(&route.path_pattern);

            // Check for priority conflicts
            if let Some(priority) = route.priority {
                if let Some(existing_pattern) = priorities.get(&priority) {
                    return Err(anyhow!(
                        "Priority conflict: routes '{}' and '{}' both have priority {}",
                        existing_pattern, route.path_pattern, priority
                    ));
                }
                priorities.insert(priority, &route.path_pattern);
            }
        }

        // Check for overlapping patterns that could cause ambiguity
        let route_patterns: Vec<&String> = self.routes.iter().map(|r| &r.path_pattern).collect();
        for (i, pattern1) in route_patterns.iter().enumerate() {
            for pattern2 in route_patterns.iter().skip(i + 1) {
                if self.patterns_overlap(pattern1, pattern2) {
                    // Only warn if neither route has a priority set to resolve the conflict
                    let route1 = &self.routes[i];
                    let route2_idx = route_patterns.iter().position(|p| p == pattern2).unwrap();
                    let route2 = &self.routes[route2_idx];
                    
                    if route1.priority.is_none() && route2.priority.is_none() {
                        return Err(anyhow!(
                            "Overlapping route patterns without priority resolution: '{}' and '{}'",
                            pattern1, pattern2
                        ));
                    }
                }
            }
        }

        Ok(())
    }

    /// Check if two route patterns overlap
    fn patterns_overlap(&self, pattern1: &str, pattern2: &str) -> bool {
        // Simple overlap detection - this could be made more sophisticated
        let p1_prefix = pattern1.trim_end_matches('*');
        let p2_prefix = pattern2.trim_end_matches('*');

        // If one pattern is a prefix of another, they overlap
        p1_prefix.starts_with(p2_prefix) || p2_prefix.starts_with(p1_prefix)
    }
}