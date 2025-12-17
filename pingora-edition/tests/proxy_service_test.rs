//! Unit tests for the GrpcProxyService

use grpc_http_proxy::config::{ProxyConfig, ServerConfig, RouteConfig, UpstreamConfig};
use grpc_http_proxy::proxy::GrpcProxyService;

#[tokio::test]
async fn test_grpc_proxy_service_creation() {
    let config = create_test_config();
    let service = GrpcProxyService::new(config, None);
    
    assert!(!service.is_tls_enabled());
    assert!(service.tls_config().is_none());
}

#[tokio::test]
async fn test_service_configuration() {
    let config = create_test_config_with_routes();
    let service = GrpcProxyService::new(config, None);
    
    // Test that service is created successfully with routes
    assert!(!service.is_tls_enabled());
    assert!(service.tls_config().is_none());
}

// Helper functions

fn create_test_config() -> ProxyConfig {
    ProxyConfig {
        server: ServerConfig {
            bind_address: "127.0.0.1:8080".parse().unwrap(),
            worker_threads: Some(1),
            max_connections: Some(100),
        },
        tls: None,
        routes: vec![],
        default_upstream: UpstreamConfig {
            address: "127.0.0.1:9090".parse().unwrap(),
            connection_pool_size: Some(10),
            health_check: None,
            timeout: None,
        },
    }
}

fn create_test_config_with_routes() -> ProxyConfig {
    ProxyConfig {
        server: ServerConfig {
            bind_address: "127.0.0.1:8080".parse().unwrap(),
            worker_threads: Some(1),
            max_connections: Some(100),
        },
        tls: None,
        routes: vec![
            RouteConfig {
                path_pattern: "/api/v1/*".to_string(),
                upstream: UpstreamConfig {
                    address: "127.0.0.1:9091".parse().unwrap(),
                    connection_pool_size: Some(10),
                    health_check: None,
                    timeout: None,
                },
                priority: Some(10),
            }
        ],
        default_upstream: UpstreamConfig {
            address: "127.0.0.1:9090".parse().unwrap(),
            connection_pool_size: Some(10),
            health_check: None,
            timeout: None,
        },
    }
}