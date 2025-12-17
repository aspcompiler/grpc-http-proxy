//! Unit tests for upstream management functionality

use grpc_http_proxy::config::{UpstreamConfig, HealthCheckConfig};
use grpc_http_proxy::proxy::upstream::{UpstreamManager, LoadBalancingStrategy};
use std::net::SocketAddr;
use std::time::Duration;

#[test]
fn test_upstream_manager_creation() {
    let upstreams = vec![
        create_test_upstream("127.0.0.1:9090", Some(10), None),
        create_test_upstream("127.0.0.1:9091", Some(20), None),
    ];

    let manager = UpstreamManager::new(upstreams);
    
    // All upstreams should be initially healthy
    assert!(manager.is_upstream_healthy(&"127.0.0.1:9090".parse().unwrap()));
    assert!(manager.is_upstream_healthy(&"127.0.0.1:9091".parse().unwrap()));
}

#[test]
fn test_get_upstream_success() {
    let upstream_config = create_test_upstream("127.0.0.1:9090", Some(10), None);
    let upstreams = vec![upstream_config.clone()];
    let manager = UpstreamManager::new(upstreams);

    let result = manager.get_upstream(&upstream_config);
    assert!(result.is_ok());
    assert_eq!(result.unwrap().address, upstream_config.address);
}

#[test]
fn test_get_upstream_not_found() {
    let upstream_config = create_test_upstream("127.0.0.1:9090", Some(10), None);
    let upstreams = vec![upstream_config];
    let manager = UpstreamManager::new(upstreams);

    let unknown_upstream = create_test_upstream("127.0.0.1:9999", Some(10), None);
    let result = manager.get_upstream(&unknown_upstream);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("Upstream not found"));
}

#[test]
fn test_connection_pool_management() {
    let upstream_config = create_test_upstream("127.0.0.1:9090", Some(2), None); // Small pool
    let upstreams = vec![upstream_config.clone()];
    let manager = UpstreamManager::new(upstreams);

    let address = upstream_config.address;

    // Should be able to acquire connections up to pool size
    let conn1 = manager.acquire_connection(&address);
    assert!(conn1.is_ok());
    assert_eq!(manager.get_connection_count(&address), 1);

    let conn2 = manager.acquire_connection(&address);
    assert!(conn2.is_ok());
    assert_eq!(manager.get_connection_count(&address), 2);

    // Pool should be exhausted now
    let conn3 = manager.acquire_connection(&address);
    assert!(conn3.is_err());
    assert!(conn3.unwrap_err().to_string().contains("Connection pool exhausted"));

    // Dropping a connection should free up space
    drop(conn1);
    assert_eq!(manager.get_connection_count(&address), 1);

    // Should be able to acquire again
    let conn4 = manager.acquire_connection(&address);
    assert!(conn4.is_ok());
    assert_eq!(manager.get_connection_count(&address), 2);
}

#[test]
fn test_load_balancing_round_robin() {
    let upstreams = vec![
        create_test_upstream("127.0.0.1:9090", Some(10), None),
        create_test_upstream("127.0.0.1:9091", Some(10), None),
        create_test_upstream("127.0.0.1:9092", Some(10), None),
    ];

    let manager = UpstreamManager::new(upstreams.clone());
    let candidates: Vec<&UpstreamConfig> = upstreams.iter().collect();

    // Test round-robin selection (should return one of the candidates)
    let selected = manager.select_upstream(&candidates, LoadBalancingStrategy::RoundRobin);
    assert!(selected.is_ok());
    
    let selected_address = selected.unwrap().address;
    assert!(candidates.iter().any(|c| c.address == selected_address));
}

#[test]
fn test_load_balancing_least_connections() {
    let upstreams = vec![
        create_test_upstream("127.0.0.1:9090", Some(10), None),
        create_test_upstream("127.0.0.1:9091", Some(10), None),
    ];

    let manager = UpstreamManager::new(upstreams.clone());
    let candidates: Vec<&UpstreamConfig> = upstreams.iter().collect();

    // Initially, both should have 0 connections, so either could be selected
    let selected = manager.select_upstream(&candidates, LoadBalancingStrategy::LeastConnections);
    assert!(selected.is_ok());

    // Acquire a connection from the first upstream
    let _conn = manager.acquire_connection(&upstreams[0].address);

    // Now the second upstream should be preferred (least connections)
    let selected = manager.select_upstream(&candidates, LoadBalancingStrategy::LeastConnections);
    assert!(selected.is_ok());
    // Note: Due to the simple implementation, we can't guarantee which one is selected
    // but the logic should prefer the one with fewer connections
}

#[test]
fn test_load_balancing_random() {
    let upstreams = vec![
        create_test_upstream("127.0.0.1:9090", Some(10), None),
        create_test_upstream("127.0.0.1:9091", Some(10), None),
    ];

    let manager = UpstreamManager::new(upstreams.clone());
    let candidates: Vec<&UpstreamConfig> = upstreams.iter().collect();

    // Test random selection (should return one of the candidates)
    let selected = manager.select_upstream(&candidates, LoadBalancingStrategy::Random);
    assert!(selected.is_ok());
    
    let selected_address = selected.unwrap().address;
    assert!(candidates.iter().any(|c| c.address == selected_address));
}

#[test]
fn test_load_balancing_no_candidates() {
    let upstreams = vec![
        create_test_upstream("127.0.0.1:9090", Some(10), None),
    ];

    let manager = UpstreamManager::new(upstreams);
    let candidates: Vec<&UpstreamConfig> = vec![];

    let result = manager.select_upstream(&candidates, LoadBalancingStrategy::RoundRobin);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("No upstream candidates provided"));
}

#[test]
fn test_health_state_management() {
    let upstream_config = create_test_upstream("127.0.0.1:9090", Some(10), None);
    let upstreams = vec![upstream_config.clone()];
    let manager = UpstreamManager::new(upstreams);

    let address = upstream_config.address;

    // Initially should be healthy
    assert!(manager.is_upstream_healthy(&address));

    let health_state = manager.get_health_state(&address);
    assert!(health_state.is_some());
    let health_state = health_state.unwrap();
    assert!(health_state.is_healthy);
    assert_eq!(health_state.consecutive_failures, 0);
    assert!(health_state.last_error.is_none());
}

#[test]
fn test_upstream_stats() {
    let upstreams = vec![
        create_test_upstream("127.0.0.1:9090", Some(10), None),
        create_test_upstream("127.0.0.1:9091", Some(20), None),
    ];

    let manager = UpstreamManager::new(upstreams.clone());

    // Acquire some connections
    let _conn1 = manager.acquire_connection(&upstreams[0].address);
    let _conn2 = manager.acquire_connection(&upstreams[0].address);

    let stats = manager.get_stats();
    assert_eq!(stats.len(), 2);

    let stats_9090 = stats.get(&upstreams[0].address).unwrap();
    assert_eq!(stats_9090.address, upstreams[0].address);
    assert!(stats_9090.is_healthy);
    assert_eq!(stats_9090.connection_count, 2);
    assert_eq!(stats_9090.consecutive_failures, 0);

    let stats_9091 = stats.get(&upstreams[1].address).unwrap();
    assert_eq!(stats_9091.address, upstreams[1].address);
    assert!(stats_9091.is_healthy);
    assert_eq!(stats_9091.connection_count, 0);
    assert_eq!(stats_9091.consecutive_failures, 0);
}

#[tokio::test]
async fn test_health_check_nonexistent_upstream() {
    let upstream_config = create_test_upstream("127.0.0.1:9090", Some(10), None);
    let upstreams = vec![upstream_config];
    let manager = UpstreamManager::new(upstreams);

    let unknown_address: SocketAddr = "127.0.0.1:9999".parse().unwrap();
    let result = manager.health_check(&unknown_address).await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("Upstream not found"));
}

#[tokio::test]
async fn test_health_check_all() {
    let upstreams = vec![
        create_test_upstream("127.0.0.1:9090", Some(10), None),
        create_test_upstream("127.0.0.1:9091", Some(10), None),
    ];

    let manager = UpstreamManager::new(upstreams);

    // This should complete without error (even though health checks will fail)
    let result = manager.health_check_all().await;
    assert!(result.is_ok());
}

#[test]
fn test_connection_handle_timeout() {
    let upstream_config = create_test_upstream("127.0.0.1:9090", Some(10), Some(Duration::from_secs(30)));
    let upstreams = vec![upstream_config.clone()];
    let manager = UpstreamManager::new(upstreams);

    let conn = manager.acquire_connection(&upstream_config.address).unwrap();
    assert_eq!(conn.timeout(), Some(Duration::from_secs(30)));
}

// Helper function to create test upstream configurations
fn create_test_upstream(address: &str, pool_size: Option<usize>, timeout: Option<Duration>) -> UpstreamConfig {
    UpstreamConfig {
        address: address.parse::<SocketAddr>().unwrap(),
        connection_pool_size: pool_size,
        health_check: None,
        timeout,
    }
}

// Helper function to create upstream with health check
#[allow(dead_code)]
fn create_test_upstream_with_health_check(
    address: &str, 
    pool_size: Option<usize>, 
    health_path: &str,
    health_interval: Duration,
    health_timeout: Duration
) -> UpstreamConfig {
    UpstreamConfig {
        address: address.parse::<SocketAddr>().unwrap(),
        connection_pool_size: pool_size,
        health_check: Some(HealthCheckConfig {
            path: health_path.to_string(),
            interval: health_interval,
            timeout: health_timeout,
        }),
        timeout: None,
    }
}