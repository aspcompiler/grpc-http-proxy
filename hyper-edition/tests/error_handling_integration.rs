//! Integration tests for error handling scenarios
//! 
//! Tests the complete error handling flow from client requests through
//! the proxy to upstream servers, including circuit breaker behavior.

use grpc_http_proxy::{ProxyServer, ProxyConfig, ProxyError};
use grpc_http_proxy::config::{ListenConfig, TlsConfig, RoutingConfig, RoutingRule, UpstreamConfig, LoggingConfig};
use grpc_http_proxy::components::health::{HealthChecker, CircuitBreakerConfig};
use grpc_http_proxy::components::Router;
use std::time::Duration;
use std::path::PathBuf;
use tempfile::NamedTempFile;
use std::io::Write;
use tokio::net::TcpListener;
use hyper::service::service_fn;
use hyper::{Request, Response, StatusCode};
use http_body_util::Full;
use bytes::Bytes;
use std::convert::Infallible;

/// Create test TLS certificates for testing
fn create_test_certificates() -> (NamedTempFile, NamedTempFile) {
    // Create a self-signed certificate for testing
    let cert_pem = r#"-----BEGIN CERTIFICATE-----
MIIBkTCB+wIJAMlyFqk69v+9MA0GCSqGSIb3DQEBCwUAMBQxEjAQBgNVBAMMCWxv
Y2FsaG9zdDAeFw0yMzEwMDEwMDAwMDBaFw0yNDEwMDEwMDAwMDBaMBQxEjAQBgNV
BAMMCWxvY2FsaG9zdDBcMA0GCSqGSIb3DQEBAQUAA0sAMEgCQQDTgvwjlRHZ9osN
uQVWDNjYIKWOQMLjWsxqfNBNgkI7VuIeU8+7QFsqmSHtMhzM8xvPYCt0K5PzNhvQ
wV8RjU5jAgMBAAEwDQYJKoZIhvcNAQELBQADQQBKZU8rMy0p6KkQJvKpP7SJ8cQx
8rQV7oQV8rQV7oQV8rQV7oQV8rQV7oQV8rQV7oQV8rQV7oQV8rQV7oQV8rQV
-----END CERTIFICATE-----"#;

    let key_pem = r#"-----BEGIN PRIVATE KEY-----
MIIBVAIBADANBgkqhkiG9w0BAQEFAASCAT4wggE6AgEAAkEA04L8I5UR2faLDbkF
VgzY2CCljkDC41rManTQTYJCO1biHlPPu0BbKpkh7TIczPMbz2ArdCuT8zYb0MFf
EY1OYwIDAQABAkEAl4KZX9i7QV8rQV7oQV8rQV7oQV8rQV7oQV8rQV7oQV8rQV7o
QV8rQV7oQV8rQV7oQV8rQV7oQV8rQV7oQV8rQQIhAOOi+q6q6q6q6q6q6q6q6q6q
6q6q6q6q6q6q6q6q6q6q6q6qAiEA6q6q6q6q6q6q6q6q6q6q6q6q6q6q6q6q6q6q
6q6q6q6qCIQDjovquqvquqvquqvquqvquqvquqvquqvquqvquqvquqvquqvquqvq
uqvquqvquqvquqvquqvquqvquqvquqvquqvquqvquqvquqvquqvquqvquqvquqvq
uqvquqvquqvquqvquqvquqvquqvquqvquqvquqvquqvquqvquqvquqvquqvquqvq
-----END PRIVATE KEY-----"#;

    let mut cert_file = NamedTempFile::new().unwrap();
    cert_file.write_all(cert_pem.as_bytes()).unwrap();
    cert_file.flush().unwrap();

    let mut key_file = NamedTempFile::new().unwrap();
    key_file.write_all(key_pem.as_bytes()).unwrap();
    key_file.flush().unwrap();

    (cert_file, key_file)
}

/// Create a test configuration
fn create_test_config(cert_file: &NamedTempFile, key_file: &NamedTempFile, upstream_port: u16) -> ProxyConfig {
    ProxyConfig {
        listen: ListenConfig {
            address: "127.0.0.1".to_string(),
            port: 0, // Let the OS choose a port
        },
        tls: Some(TlsConfig {
            cert_file: cert_file.path().to_path_buf(),
            key_file: key_file.path().to_path_buf(),
            root_ca_file: None,
        }),
        routing: RoutingConfig {
            rules: vec![
                RoutingRule {
                    pattern: "/test.*".to_string(),
                    upstream: UpstreamConfig {
                        host: "127.0.0.1".to_string(),
                        port: upstream_port,
                        timeout: Duration::from_secs(5),
                        max_connections: 10,
                    },
                    priority: 100,
                },
                RoutingRule {
                    pattern: "/error.*".to_string(),
                    upstream: UpstreamConfig {
                        host: "127.0.0.1".to_string(),
                        port: upstream_port + 1, // Non-existent port for error testing
                        timeout: Duration::from_secs(1),
                        max_connections: 5,
                    },
                    priority: 90,
                },
            ],
            catch_all: Some(UpstreamConfig {
                host: "127.0.0.1".to_string(),
                port: upstream_port,
                timeout: Duration::from_secs(5),
                max_connections: 10,
            }),
        },
        logging: LoggingConfig {
            level: "debug".to_string(),
        },
    }
}

/// Mock upstream server for testing
async fn create_mock_upstream() -> (u16, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();

    let handle = tokio::spawn(async move {
        loop {
            match listener.accept().await {
                Ok((stream, _)) => {
                    let io = hyper_util::rt::TokioIo::new(stream);
                    let service = service_fn(mock_grpc_handler);
                    
                    tokio::spawn(async move {
                        if let Err(e) = hyper::server::conn::http1::Builder::new()
                            .serve_connection(io, service)
                            .await
                        {
                            eprintln!("Mock server connection error: {}", e);
                        }
                    });
                }
                Err(e) => {
                    eprintln!("Mock server accept error: {}", e);
                    break;
                }
            }
        }
    });

    (port, handle)
}

/// Mock gRPC handler for testing
async fn mock_grpc_handler(req: Request<hyper::body::Incoming>) -> Result<Response<Full<Bytes>>, Infallible> {
    let path = req.uri().path();
    
    match path {
        "/test.TestService/Success" => {
            Ok(Response::builder()
                .status(StatusCode::OK)
                .header("content-type", "application/grpc")
                .header("grpc-status", "0")
                .body(Full::new(Bytes::from("success")))
                .unwrap())
        }
        "/test.TestService/Error" => {
            Ok(Response::builder()
                .status(StatusCode::OK)
                .header("content-type", "application/grpc")
                .header("grpc-status", "3") // INVALID_ARGUMENT
                .header("grpc-message", "test error")
                .body(Full::new(Bytes::from("error")))
                .unwrap())
        }
        "/test.TestService/Timeout" => {
            // Simulate a slow response
            tokio::time::sleep(Duration::from_secs(10)).await;
            Ok(Response::builder()
                .status(StatusCode::OK)
                .header("content-type", "application/grpc")
                .header("grpc-status", "0")
                .body(Full::new(Bytes::from("timeout")))
                .unwrap())
        }
        _ => {
            Ok(Response::builder()
                .status(StatusCode::NOT_FOUND)
                .body(Full::new(Bytes::from("not found")))
                .unwrap())
        }
    }
}

#[tokio::test]
async fn test_proxy_error_status_code_mapping() {
    // Test that ProxyError correctly maps to HTTP status codes
    use grpc_http_proxy::error::ProxyError;
    use hyper::StatusCode;
    
    // Client errors (4xx)
    assert_eq!(
        ProxyError::invalid_grpc_request("bad request").to_status_code(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        ProxyError::no_route_found("/unknown/path").to_status_code(),
        StatusCode::NOT_FOUND
    );
    
    // Upstream errors (5xx)
    assert_eq!(
        ProxyError::upstream_timeout("localhost:9090", 5000).to_status_code(),
        StatusCode::GATEWAY_TIMEOUT
    );
    assert_eq!(
        ProxyError::circuit_breaker_open("localhost:9090").to_status_code(),
        StatusCode::SERVICE_UNAVAILABLE
    );
}

#[tokio::test]
async fn test_circuit_breaker_functionality() {
    // Test circuit breaker behavior with fast configuration
    let config = CircuitBreakerConfig {
        failure_threshold: 3,
        recovery_timeout: Duration::from_millis(100),
        success_threshold: 2,
        request_timeout: Duration::from_secs(1),
    };
    
    let health_checker = HealthChecker::with_config(config, Duration::from_secs(10));
    let upstream = UpstreamConfig {
        host: "invalid-host-for-testing".to_string(),
        port: 9999,
        timeout: Duration::from_secs(1),
        max_connections: 5,
    };
    
    // Initially should allow requests
    assert!(health_checker.should_allow_request(&upstream).await.is_ok());
    
    // Record failures to open circuit
    for i in 0..3 {
        health_checker.record_failure(&upstream).await;
        
        if i < 2 {
            // Should still allow before threshold
            assert!(health_checker.should_allow_request(&upstream).await.is_ok());
        } else {
            // Should reject after threshold
            assert!(health_checker.should_allow_request(&upstream).await.is_err());
        }
    }
    
    // Wait for recovery timeout
    tokio::time::sleep(Duration::from_millis(150)).await;
    
    // Should transition to half-open
    assert!(health_checker.should_allow_request(&upstream).await.is_ok());
    
    // Record successes to close circuit
    health_checker.record_success(&upstream).await;
    health_checker.record_success(&upstream).await;
    
    // Should be closed now
    assert!(health_checker.should_allow_request(&upstream).await.is_ok());
}

#[tokio::test]
async fn test_error_retry_logic() {
    use grpc_http_proxy::error::ProxyError;
    
    // Test retryable errors
    let retryable_errors = vec![
        ProxyError::connection_timeout("localhost:9090"),
        ProxyError::UpstreamUnavailable { address: "service:8080".to_string() },
        ProxyError::ResourceExhausted { resource: "connections".to_string() },
    ];
    
    for error in retryable_errors {
        assert!(error.is_retryable());
        
        // Test retry delays increase
        let delay0 = error.retry_delay(0).unwrap();
        let delay1 = error.retry_delay(1).unwrap();
        let delay2 = error.retry_delay(2).unwrap();
        
        assert!(delay1 >= delay0);
        assert!(delay2 >= delay1);
    }
    
    // Test non-retryable errors
    let non_retryable_errors = vec![
        ProxyError::invalid_grpc_request("bad request"),
        ProxyError::no_route_found("/unknown"),
        ProxyError::ConfigError("invalid config".to_string()),
        ProxyError::circuit_breaker_open("localhost:9090"),
    ];
    
    for error in non_retryable_errors {
        assert!(!error.is_retryable());
        assert!(error.retry_delay(0).is_none());
    }
}

#[tokio::test]
async fn test_health_check_error_scenarios() {
    // Use a shorter health check interval for testing
    let config = grpc_http_proxy::components::health::CircuitBreakerConfig {
        failure_threshold: 3,
        recovery_timeout: Duration::from_secs(5),
        success_threshold: 2,
        request_timeout: Duration::from_secs(1),
    };
    let health_checker = HealthChecker::with_config(config, Duration::from_millis(100));
    
    // Test with non-existent host
    let bad_upstream = UpstreamConfig {
        host: "non-existent-host-12345".to_string(),
        port: 9999,
        timeout: Duration::from_secs(1),
        max_connections: 5,
    };
    
    // Start health checking
    health_checker.start_health_checking(bad_upstream.clone()).await;
    
    // Give it time to perform health check (wait for at least one interval)
    tokio::time::sleep(Duration::from_millis(300)).await;
    
    // Should be unhealthy
    assert!(!health_checker.is_healthy(&bad_upstream).await);
    
    let health_status = health_checker.get_health_status(&bad_upstream).await;
    assert!(health_status.is_some());
    
    let status = health_status.unwrap();
    assert_eq!(status.status, grpc_http_proxy::components::health::HealthStatus::Unhealthy);
    assert!(status.error.is_some());
    
    // Clean up
    health_checker.stop_health_checking(&bad_upstream).await;
}

#[tokio::test]
async fn test_error_categorization() {
    use grpc_http_proxy::error::{ProxyError, ErrorCategory};
    
    let test_cases = vec![
        (ProxyError::tls_handshake("cert error"), ErrorCategory::Tls),
        (ProxyError::connection_timeout("host:port"), ErrorCategory::Network),
        (ProxyError::Http2Protocol { message: "bad frame".to_string() }, ErrorCategory::Protocol),
        (ProxyError::invalid_grpc_request("bad grpc"), ErrorCategory::Grpc),
        (ProxyError::no_route_found("/path"), ErrorCategory::Routing),
        (ProxyError::UpstreamUnavailable { address: "host:port".to_string() }, ErrorCategory::Upstream),
        (ProxyError::ConfigError("bad config".to_string()), ErrorCategory::Configuration),
        (ProxyError::UriParse { uri: "bad".to_string(), reason: "invalid".to_string() }, ErrorCategory::Parsing),
        (ProxyError::ResourceExhausted { resource: "memory".to_string() }, ErrorCategory::Resource),
        (ProxyError::Internal { message: "error".to_string() }, ErrorCategory::Internal),
    ];
    
    for (error, expected_category) in test_cases {
        assert_eq!(error.category(), expected_category);
        
        // Test string representation
        assert!(!expected_category.as_str().is_empty());
    }
}

#[tokio::test]
async fn test_server_creation_with_invalid_config() {
    // Test server creation with invalid TLS certificates
    let (cert_file, key_file) = create_test_certificates();
    let config = create_test_config(&cert_file, &key_file, 9090);
    
    // This should fail because our test certificates are invalid
    let result = ProxyServer::new(config);
    assert!(result.is_err());
    
    // The error should be related to TLS configuration
    match result.err().unwrap() {
        ProxyError::Tls(_) | ProxyError::TlsConfig { .. } | ProxyError::TlsCertificate { .. } | ProxyError::ConfigError(_) => {
            // Expected TLS or config-related error
        }
        other => panic!("Expected TLS or config error, got: {:?}", other),
    }
}

#[tokio::test]
async fn test_routing_error_scenarios() {
    use grpc_http_proxy::components::Router;
    use grpc_http_proxy::config::RoutingConfig;
    
    // Test invalid routing patterns
    let invalid_config = RoutingConfig {
        rules: vec![],
        catch_all: None,
    };
    
    // Should fail validation - no rules and no catch-all
    assert!(Router::validate_routing_config(&invalid_config).is_err());
    
    // Test invalid glob pattern
    assert!(Router::validate_pattern("[invalid").is_err());
    
    // Test valid patterns
    assert!(Router::validate_pattern("/auth.*").is_ok());
    assert!(Router::validate_pattern("*.Service/*").is_ok());
}

#[tokio::test]
async fn test_grpc_request_validation_errors() {
    use grpc_http_proxy::components::Router;
    
    let invalid_paths = vec![
        "",                           // Empty path
        "/",                          // Just slash
        "/auth",                      // Missing method
        "/auth/",                     // Empty method
        "//Login",                    // Empty service
        "/auth/Login/Extra",          // Too many parts
        "/auth./Login",               // Invalid service (ends with dot)
        "/auth/login",                // Method should start with uppercase
        "/auth.Service/Login-Method", // Invalid method name (contains dash)
        "/Service/Login",             // Service without package
    ];
    
    for invalid_path in invalid_paths {
        let result = Router::parse_grpc_url(invalid_path);
        assert!(result.is_err(), "Expected error for path: {}", invalid_path);
        
        // Verify it's a routing error
        match result.unwrap_err() {
            ProxyError::RoutingError(_) => {} // Expected
            other => panic!("Expected RoutingError for path '{}', got: {:?}", invalid_path, other),
        }
    }
}

#[tokio::test]
async fn test_connection_pool_error_handling() {
    use grpc_http_proxy::components::forwarder::ConnectionPool;
    
    let pool = ConnectionPool::new();
    let invalid_upstream = UpstreamConfig {
        host: "invalid-host-name-that-does-not-exist".to_string(),
        port: 9999,
        timeout: Duration::from_secs(1),
        max_connections: 5,
    };
    
    // Should fail to create connection to invalid host
    let result = pool.get_connection(&invalid_upstream).await;
    assert!(result.is_err());
    
    // Should be an upstream error
    match result.unwrap_err() {
        ProxyError::UpstreamError(_) => {} // Expected
        other => panic!("Expected UpstreamError, got: {:?}", other),
    }
}

#[tokio::test]
async fn test_comprehensive_error_flow() {
    // This test simulates a complete error flow from configuration to request handling
    
    // 1. Test configuration errors
    let invalid_config = ProxyConfig {
        listen: ListenConfig {
            address: "invalid-address".to_string(),
            port: 8080, // Valid port but invalid address
        },
        tls: Some(TlsConfig {
            cert_file: PathBuf::from("/non/existent/cert.pem"),
            key_file: PathBuf::from("/non/existent/key.pem"),
            root_ca_file: None,
        }),
        routing: RoutingConfig {
            rules: vec![],
            catch_all: None,
        },
        logging: LoggingConfig {
            level: "debug".to_string(),
        },
    };
    
    // Should fail to create socket address
    let addr_result = invalid_config.socket_addr();
    assert!(addr_result.is_err());
    
    // 2. Test TLS configuration errors
    let result = ProxyServer::new(invalid_config);
    assert!(result.is_err());
    
    // 3. Test routing configuration errors
    let empty_routing = RoutingConfig {
        rules: vec![],
        catch_all: None,
    };
    assert!(Router::validate_routing_config(&empty_routing).is_err());
}

#[tokio::test]
async fn test_error_helper_functions() {
    use grpc_http_proxy::error::ProxyError;
    
    // Test all error helper functions
    let tls_error = ProxyError::tls_handshake("certificate expired");
    assert!(tls_error.to_string().contains("certificate expired"));
    
    let timeout_error = ProxyError::connection_timeout("192.168.1.1:8080");
    assert!(timeout_error.to_string().contains("192.168.1.1:8080"));
    
    let upstream_timeout = ProxyError::upstream_timeout("service.com:443", 30000);
    assert!(upstream_timeout.to_string().contains("service.com:443"));
    assert!(upstream_timeout.to_string().contains("30000ms"));
    
    let no_route = ProxyError::no_route_found("/api/v1/users");
    assert!(no_route.to_string().contains("/api/v1/users"));
    
    let grpc_error = ProxyError::invalid_grpc_request("missing headers");
    assert!(grpc_error.to_string().contains("missing headers"));
    
    let grpc_status = ProxyError::grpc_status(14, "service unavailable");
    assert!(grpc_status.to_string().contains("status=14"));
    assert!(grpc_status.to_string().contains("service unavailable"));
    
    let circuit_breaker = ProxyError::circuit_breaker_open("backend:8080");
    assert!(circuit_breaker.to_string().contains("backend:8080"));
    
    let health_check = ProxyError::upstream_health_check_failed("api:443", "timeout");
    assert!(health_check.to_string().contains("api:443"));
    assert!(health_check.to_string().contains("timeout"));
}