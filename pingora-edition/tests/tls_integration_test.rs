//! Integration test for TLS management

use grpc_http_proxy::config::{ProxyConfig, ServerConfig, TlsConfig, UpstreamConfig};
use grpc_http_proxy::proxy::ProxyServer;
use std::net::SocketAddr;
use std::path::PathBuf;
use tempfile::TempDir;
use std::fs;

#[tokio::test]
async fn test_tls_integration_no_tls() {
    // Create a basic configuration without TLS
    let config = ProxyConfig {
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
    };

    // Create proxy server
    let server = ProxyServer::new(config).expect("Failed to create proxy server");
    
    // This should succeed without TLS
    // Note: We don't actually start the server as it would block
    // In a real test, you might use a timeout or run in a separate task
}

#[tokio::test]
async fn test_tls_integration_with_dummy_config() {
    let temp_dir = TempDir::new().unwrap();
    let cert_path = temp_dir.path().join("cert.pem");
    let key_path = temp_dir.path().join("key.pem");

    // Create dummy certificate files (not valid, just for testing structure)
    fs::write(&cert_path, "dummy cert content").unwrap();
    fs::write(&key_path, "dummy key content").unwrap();

    let config = ProxyConfig {
        server: ServerConfig {
            bind_address: "127.0.0.1:8443".parse().unwrap(),
            worker_threads: Some(2),
            max_connections: Some(200),
        },
        tls: Some(TlsConfig {
            cert_path,
            key_path,
            ca_cert_path: None,
        }),
        routes: vec![],
        default_upstream: UpstreamConfig {
            address: "127.0.0.1:9090".parse().unwrap(),
            connection_pool_size: Some(10),
            health_check: None,
            timeout: None,
        },
    };

    // This should fail during certificate loading since we have dummy files
    // but it demonstrates the integration structure
    let result = ProxyServer::new(config);
    assert!(result.is_err(), "Should fail with invalid certificates");
}