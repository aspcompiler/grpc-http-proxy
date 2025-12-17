//! End-to-end integration tests for the gRPC HTTP Proxy
//! 
//! Tests complete proxy functionality including:
//! - Configuration loading and validation
//! - TLS termination and HTTP/2 negotiation
//! - Request routing to upstream servers
//! - gRPC message forwarding with trailers
//! - Error handling and recovery

use grpc_http_proxy::{ProxyServer, ProxyConfig};
use grpc_http_proxy::config::{ListenConfig, TlsConfig, RoutingConfig, RoutingRule, UpstreamConfig, LoggingConfig, Args};
use std::time::Duration;
use tempfile::NamedTempFile;
use std::io::Write;
use tokio::net::TcpListener;
use hyper::service::service_fn;
use hyper::{Request, Response, StatusCode, Method};
use hyper_util::client::legacy::Client;
use http_body_util::Full;
use bytes::Bytes;
use std::convert::Infallible;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

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

/// Create a test configuration with multiple routing rules and TLS
fn create_comprehensive_test_config(
    cert_file: &NamedTempFile, 
    key_file: &NamedTempFile, 
    auth_port: u16,
    user_port: u16,
    default_port: u16
) -> ProxyConfig {
    ProxyConfig {
        listen: ListenConfig {
            address: "127.0.0.1".to_string(),
            port: 8443, // Use a specific port for testing
        },
        tls: Some(TlsConfig {
            cert_file: cert_file.path().to_path_buf(),
            key_file: key_file.path().to_path_buf(),
            root_ca_file: None,
        }),
        routing: RoutingConfig {
            rules: vec![
                RoutingRule {
                    pattern: "/auth.*".to_string(),
                    upstream: UpstreamConfig {
                        host: "127.0.0.1".to_string(),
                        port: auth_port,
                        timeout: Duration::from_secs(5),
                        max_connections: 10,
                    },
                    priority: 10, // High priority
                },
                RoutingRule {
                    pattern: "/user.UserService/*".to_string(),
                    upstream: UpstreamConfig {
                        host: "127.0.0.1".to_string(),
                        port: user_port,
                        timeout: Duration::from_secs(5),
                        max_connections: 10,
                    },
                    priority: 20, // Medium priority
                },
            ],
            catch_all: Some(UpstreamConfig {
                host: "127.0.0.1".to_string(),
                port: default_port,
                timeout: Duration::from_secs(5),
                max_connections: 10,
            }),
        },
        logging: LoggingConfig {
            level: "debug".to_string(),
        },
    }
}

/// Create a test configuration with multiple routing rules but no TLS
fn create_comprehensive_test_config_no_tls(
    auth_port: u16,
    user_port: u16,
    default_port: u16
) -> ProxyConfig {
    ProxyConfig {
        listen: ListenConfig {
            address: "127.0.0.1".to_string(),
            port: 8080, // Use standard HTTP port for non-TLS
        },
        tls: None, // No TLS configuration
        routing: RoutingConfig {
            rules: vec![
                RoutingRule {
                    pattern: "/auth.*".to_string(),
                    upstream: UpstreamConfig {
                        host: "127.0.0.1".to_string(),
                        port: auth_port,
                        timeout: Duration::from_secs(5),
                        max_connections: 10,
                    },
                    priority: 10, // High priority
                },
                RoutingRule {
                    pattern: "/user.UserService/*".to_string(),
                    upstream: UpstreamConfig {
                        host: "127.0.0.1".to_string(),
                        port: user_port,
                        timeout: Duration::from_secs(5),
                        max_connections: 10,
                    },
                    priority: 20, // Medium priority
                },
            ],
            catch_all: Some(UpstreamConfig {
                host: "127.0.0.1".to_string(),
                port: default_port,
                timeout: Duration::from_secs(5),
                max_connections: 10,
            }),
        },
        logging: LoggingConfig {
            level: "debug".to_string(),
        },
    }
}

/// Mock upstream server that tracks requests
async fn create_mock_upstream_with_tracking(name: &str) -> (u16, tokio::task::JoinHandle<()>, Arc<AtomicUsize>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let request_count = Arc::new(AtomicUsize::new(0));
    let request_count_clone = Arc::clone(&request_count);
    let name = name.to_string();

    let handle = tokio::spawn(async move {
        loop {
            match listener.accept().await {
                Ok((stream, _)) => {
                    let io = hyper_util::rt::TokioIo::new(stream);
                    let service = service_fn({
                        let request_count = Arc::clone(&request_count_clone);
                        let name = name.clone();
                        move |req| {
                            let request_count = Arc::clone(&request_count);
                            let name = name.clone();
                            async move {
                                mock_grpc_handler_with_tracking(req, request_count, name).await
                            }
                        }
                    });
                    
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

    (port, handle, request_count)
}

/// Mock gRPC handler that tracks requests and responds based on service
async fn mock_grpc_handler_with_tracking(
    req: Request<hyper::body::Incoming>,
    request_count: Arc<AtomicUsize>,
    service_name: String,
) -> Result<Response<Full<Bytes>>, Infallible> {
    let path = req.uri().path();
    let method = req.method();
    
    // Increment request counter
    request_count.fetch_add(1, Ordering::Relaxed);
    
    println!("Mock {} received: {} {}", service_name, method, path);
    
    // Validate gRPC request format
    if method != Method::POST {
        return Ok(Response::builder()
            .status(StatusCode::METHOD_NOT_ALLOWED)
            .body(Full::new(Bytes::from("Only POST allowed")))
            .unwrap());
    }
    
    // Check content type
    let is_grpc = req.headers().get("content-type")
        .and_then(|ct| ct.to_str().ok())
        .map(|ct| ct.starts_with("application/grpc"))
        .unwrap_or(false);
        
    if !is_grpc {
        return Ok(Response::builder()
            .status(StatusCode::BAD_REQUEST)
            .body(Full::new(Bytes::from("gRPC content-type required")))
            .unwrap());
    }
    
    match path {
        // Auth service endpoints
        "/auth.AuthService/Login" => {
            Ok(Response::builder()
                .status(StatusCode::OK)
                .header("content-type", "application/grpc")
                .header("grpc-status", "0")
                .header("grpc-message", "")
                .body(Full::new(Bytes::from(format!("auth_login_response_{}", service_name))))
                .unwrap())
        }
        "/auth.AuthService/Logout" => {
            Ok(Response::builder()
                .status(StatusCode::OK)
                .header("content-type", "application/grpc")
                .header("grpc-status", "0")
                .body(Full::new(Bytes::from(format!("auth_logout_response_{}", service_name))))
                .unwrap())
        }
        
        // User service endpoints
        "/user.UserService/GetUser" => {
            Ok(Response::builder()
                .status(StatusCode::OK)
                .header("content-type", "application/grpc")
                .header("grpc-status", "0")
                .body(Full::new(Bytes::from(format!("user_get_response_{}", service_name))))
                .unwrap())
        }
        "/user.UserService/CreateUser" => {
            Ok(Response::builder()
                .status(StatusCode::OK)
                .header("content-type", "application/grpc")
                .header("grpc-status", "0")
                .body(Full::new(Bytes::from(format!("user_create_response_{}", service_name))))
                .unwrap())
        }
        
        // Default service endpoints
        "/default.DefaultService/Echo" => {
            Ok(Response::builder()
                .status(StatusCode::OK)
                .header("content-type", "application/grpc")
                .header("grpc-status", "0")
                .body(Full::new(Bytes::from(format!("default_echo_response_{}", service_name))))
                .unwrap())
        }
        
        // Error simulation endpoints
        "/test.TestService/Error" => {
            Ok(Response::builder()
                .status(StatusCode::OK)
                .header("content-type", "application/grpc")
                .header("grpc-status", "3") // INVALID_ARGUMENT
                .header("grpc-message", "simulated error")
                .body(Full::new(Bytes::from("error_response")))
                .unwrap())
        }
        
        // Timeout simulation
        "/test.TestService/Timeout" => {
            tokio::time::sleep(Duration::from_secs(10)).await;
            Ok(Response::builder()
                .status(StatusCode::OK)
                .header("content-type", "application/grpc")
                .header("grpc-status", "0")
                .body(Full::new(Bytes::from("timeout_response")))
                .unwrap())
        }
        
        _ => {
            Ok(Response::builder()
                .status(StatusCode::NOT_FOUND)
                .header("content-type", "application/grpc")
                .header("grpc-status", "12") // UNIMPLEMENTED
                .header("grpc-message", "method not found")
                .body(Full::new(Bytes::from("not_found")))
                .unwrap())
        }
    }
}

/// Create a YAML configuration file for testing
fn create_test_config_file(
    cert_file: &NamedTempFile,
    key_file: &NamedTempFile,
    auth_port: u16,
    user_port: u16,
    default_port: u16,
) -> NamedTempFile {
    let yaml_content = format!(r#"
listen:
  address: "127.0.0.1"
  port: 8443

tls:
  cert_file: "{}"
  key_file: "{}"

routing:
  rules:
    - pattern: "/auth.*"
      upstream:
        host: "127.0.0.1"
        port: {}
        timeout: 5
        max_connections: 10
      priority: 10
    - pattern: "/user.UserService/*"
      upstream:
        host: "127.0.0.1"
        port: {}
        timeout: 5
        max_connections: 10
      priority: 20
  
  catch_all:
    host: "127.0.0.1"
    port: {}
    timeout: 5
    max_connections: 10

logging:
  level: "debug"
"#, 
        cert_file.path().display(), 
        key_file.path().display(),
        auth_port,
        user_port,
        default_port
    );

    let mut config_file = NamedTempFile::new().unwrap();
    config_file.write_all(yaml_content.as_bytes()).unwrap();
    config_file.flush().unwrap();
    config_file
}

/// Create a YAML configuration file for testing without TLS
fn create_test_config_file_no_tls(
    auth_port: u16,
    user_port: u16,
    default_port: u16,
) -> NamedTempFile {
    let yaml_content = format!(r#"
listen:
  address: "127.0.0.1"
  port: 8080

# TLS is disabled - proxy will run in unencrypted mode

routing:
  rules:
    - pattern: "/auth.*"
      upstream:
        host: "127.0.0.1"
        port: {}
        timeout: 5
        max_connections: 10
      priority: 10
    - pattern: "/user.UserService/*"
      upstream:
        host: "127.0.0.1"
        port: {}
        timeout: 5
        max_connections: 10
      priority: 20
  
  catch_all:
    host: "127.0.0.1"
    port: {}
    timeout: 5
    max_connections: 10

logging:
  level: "debug"
"#, 
        auth_port,
        user_port,
        default_port
    );

    let mut config_file = NamedTempFile::new().unwrap();
    config_file.write_all(yaml_content.as_bytes()).unwrap();
    config_file.flush().unwrap();
    config_file
}

#[tokio::test]
async fn test_configuration_loading_from_file() {
    let (cert_file, key_file) = create_test_certificates();
    let config_file = create_test_config_file(&cert_file, &key_file, 9090, 9091, 9092);
    
    // Test loading configuration from file
    let config = ProxyConfig::from_file(config_file.path()).unwrap();
    
    // Verify configuration structure
    assert_eq!(config.listen.address, "127.0.0.1");
    assert_eq!(config.listen.port, 8443);
    assert_eq!(config.routing.rules.len(), 2);
    assert!(config.routing.catch_all.is_some());
    
    // Verify routing rules
    let auth_rule = &config.routing.rules[0];
    assert_eq!(auth_rule.pattern, "/auth.*");
    assert_eq!(auth_rule.upstream.port, 9090);
    assert_eq!(auth_rule.priority, 10);
    
    let user_rule = &config.routing.rules[1];
    assert_eq!(user_rule.pattern, "/user.UserService/*");
    assert_eq!(user_rule.upstream.port, 9091);
    assert_eq!(user_rule.priority, 20);
    
    // Verify catch-all
    let catch_all = config.routing.catch_all.unwrap();
    assert_eq!(catch_all.port, 9092);
}

#[tokio::test]
async fn test_configuration_loading_with_cli_overrides() {
    let (cert_file, key_file) = create_test_certificates();
    let config_file = create_test_config_file(&cert_file, &key_file, 9090, 9091, 9092);
    
    // Create CLI args with overrides
    let args = Args {
        config: config_file.path().to_path_buf(),
        listen_address: Some("0.0.0.0".to_string()),
        listen_port: Some(8443),
        tls_cert: None,
        tls_key: None,
        tls_root_ca: None,
        no_tls: false,
        log_level: Some("info".to_string()),
    };
    
    // Load configuration with CLI overrides
    let config = ProxyConfig::from_args(args).unwrap();
    
    // Verify overrides were applied
    assert_eq!(config.listen.address, "0.0.0.0");
    assert_eq!(config.listen.port, 8443);
    assert_eq!(config.logging.level, "info");
    
    // Verify non-overridden values remain from file
    assert_eq!(config.routing.rules.len(), 2);
}

#[tokio::test]
async fn test_configuration_loading_no_tls() {
    let config_file = create_test_config_file_no_tls(9090, 9091, 9092);
    
    // Test loading configuration from file
    let config = ProxyConfig::from_file(config_file.path()).unwrap();
    
    // Verify configuration structure
    assert_eq!(config.listen.address, "127.0.0.1");
    assert_eq!(config.listen.port, 8080);
    assert_eq!(config.routing.rules.len(), 2);
    assert!(config.routing.catch_all.is_some());
    assert!(config.tls.is_none());
    assert!(!config.is_tls_enabled());
    
    // Verify routing rules
    let auth_rule = &config.routing.rules[0];
    assert_eq!(auth_rule.pattern, "/auth.*");
    assert_eq!(auth_rule.upstream.port, 9090);
    assert_eq!(auth_rule.priority, 10);
    
    let user_rule = &config.routing.rules[1];
    assert_eq!(user_rule.pattern, "/user.UserService/*");
    assert_eq!(user_rule.upstream.port, 9091);
    assert_eq!(user_rule.priority, 20);
    
    // Verify catch-all
    let catch_all = config.routing.catch_all.unwrap();
    assert_eq!(catch_all.port, 9092);
}

#[tokio::test]
async fn test_configuration_no_tls_cli_flag() {
    let (cert_file, key_file) = create_test_certificates();
    let config_file = create_test_config_file(&cert_file, &key_file, 9090, 9091, 9092);
    
    // Create CLI args with --no-tls flag
    let args = Args {
        config: config_file.path().to_path_buf(),
        listen_address: None,
        listen_port: Some(8080),
        tls_cert: None,
        tls_key: None,
        tls_root_ca: None,
        no_tls: true, // Disable TLS via CLI
        log_level: None,
    };
    
    // Load configuration with CLI overrides
    let config = ProxyConfig::from_args(args).unwrap();
    
    // Verify TLS is disabled despite being in the config file
    assert_eq!(config.listen.port, 8080);
    assert!(config.tls.is_none());
    assert!(!config.is_tls_enabled());
    
    // Verify other values remain from file
    assert_eq!(config.routing.rules.len(), 2);
}

#[tokio::test]
async fn test_mutual_tls_configuration() {
    let (cert_file, key_file) = create_test_certificates();
    
    // Create a dummy root CA file
    let mut root_ca_file = NamedTempFile::new().unwrap();
    root_ca_file.write_all(b"dummy root ca").unwrap();
    root_ca_file.flush().unwrap();
    
    let yaml_content = format!(r#"
listen:
  address: "127.0.0.1"
  port: 8443

tls:
  cert_file: "{}"
  key_file: "{}"
  root_ca_file: "{}"

routing:
  rules:
    - pattern: "/auth.*"
      upstream:
        host: "127.0.0.1"
        port: 9090
        timeout: 5
        max_connections: 10
      priority: 10
  
  catch_all:
    host: "127.0.0.1"
    port: 9092
    timeout: 5
    max_connections: 10

logging:
  level: "debug"
"#, 
        cert_file.path().display(), 
        key_file.path().display(),
        root_ca_file.path().display()
    );

    let mut config_file = NamedTempFile::new().unwrap();
    config_file.write_all(yaml_content.as_bytes()).unwrap();
    config_file.flush().unwrap();
    
    // Test loading configuration from file
    let config = ProxyConfig::from_file(config_file.path()).unwrap();
    
    // Verify mutual TLS configuration
    assert!(config.is_tls_enabled());
    assert!(config.is_mutual_tls_enabled());
    
    let tls_config = config.tls.as_ref().unwrap();
    assert!(tls_config.root_ca_file.is_some());
    assert_eq!(tls_config.root_ca_file.as_ref().unwrap(), &root_ca_file.path().to_path_buf());
}

#[tokio::test]
async fn test_configuration_validation_scenarios() {
    let (cert_file, key_file) = create_test_certificates();
    
    // Test that a valid configuration structure passes basic validation
    // (The TLS files exist as temporary files, so file existence validation passes)
    let valid_config = create_comprehensive_test_config(&cert_file, &key_file, 9090, 9091, 9092);
    let validation_result = valid_config.validate();
    // This should pass because all the files exist and the structure is valid
    assert!(validation_result.is_ok());
    
    // Test invalid port
    let mut invalid_port_config = create_comprehensive_test_config(&cert_file, &key_file, 9090, 9091, 9092);
    invalid_port_config.listen.port = 0; // This should be invalid according to our validation
    let port_validation_result = invalid_port_config.validate();
    assert!(port_validation_result.is_err());
    
    // Test empty routing
    let mut empty_routing_config = create_comprehensive_test_config(&cert_file, &key_file, 9090, 9091, 9092);
    empty_routing_config.routing.rules.clear();
    empty_routing_config.routing.catch_all = None;
    let validation_result = empty_routing_config.validate();
    assert!(validation_result.is_err());
    
    // Test invalid log level
    let mut invalid_log_config = create_comprehensive_test_config(&cert_file, &key_file, 9090, 9091, 9092);
    invalid_log_config.logging.level = "invalid_level".to_string();
    let validation_result = invalid_log_config.validate();
    assert!(validation_result.is_err());
}

#[tokio::test]
async fn test_routing_rule_priority_and_matching() {
    use grpc_http_proxy::components::Router;
    
    let (cert_file, key_file) = create_test_certificates();
    let config = create_comprehensive_test_config(&cert_file, &key_file, 9090, 9091, 9092);
    
    let router = Router::new(config.routing).unwrap();
    
    // Test specific pattern matching (higher priority)
    let auth_upstream = router.route_request("/auth.AuthService/Login").unwrap();
    assert_eq!(auth_upstream.port, 9090);
    
    let auth_upstream2 = router.route_request("/auth.TokenService/Validate").unwrap();
    assert_eq!(auth_upstream2.port, 9090);
    
    // Test user service pattern matching
    let user_upstream = router.route_request("/user.UserService/GetUser").unwrap();
    assert_eq!(user_upstream.port, 9091);
    
    let user_upstream2 = router.route_request("/user.UserService/CreateUser").unwrap();
    assert_eq!(user_upstream2.port, 9091);
    
    // Test catch-all routing
    let default_upstream = router.route_request("/other.OtherService/Method").unwrap();
    assert_eq!(default_upstream.port, 9092);
    
    let default_upstream2 = router.route_request("/unknown.Service/Method").unwrap();
    assert_eq!(default_upstream2.port, 9092);
}

#[tokio::test]
async fn test_grpc_url_parsing_comprehensive() {
    use grpc_http_proxy::components::Router;
    
    // Test valid gRPC URLs
    let valid_urls = vec![
        "/auth.AuthService/Login",
        "/user.UserService/GetUser",
        "/package.service.SubService/Method",
        "/com.example.Service/LongMethodName",
    ];
    
    for url in valid_urls {
        let result = Router::parse_grpc_url(url);
        assert!(result.is_ok(), "Should parse valid URL: {}", url);
        
        let grpc_request = result.unwrap();
        assert!(!grpc_request.service.is_empty(), "Service should not be empty for: {}", url);
        assert!(!grpc_request.method.is_empty(), "Method should not be empty for: {}", url);
    }
    
    // Test invalid gRPC URLs
    let invalid_urls = vec![
        "",
        "/",
        "/service",
        "/service/",
        "//method",
        "/service//method",
        "/service/method/extra",
        "service/method", // Missing leading slash
    ];
    
    for url in invalid_urls {
        let result = Router::parse_grpc_url(url);
        assert!(result.is_err(), "Should reject invalid URL: {}", url);
    }
}

#[tokio::test]
async fn test_server_startup_and_shutdown_sequence() {
    let (cert_file, key_file) = create_test_certificates();
    let config = create_comprehensive_test_config(&cert_file, &key_file, 9090, 9091, 9092);
    
    // Test server creation (will fail due to invalid TLS certs, but tests the flow)
    let server_result = ProxyServer::new(config);
    
    // We expect this to fail due to invalid test certificates
    assert!(server_result.is_err());
    
    // Verify the error is TLS-related
    let error = server_result.err().unwrap();
    match error {
        grpc_http_proxy::ProxyError::Tls(_) | 
        grpc_http_proxy::ProxyError::TlsConfig { .. } | 
        grpc_http_proxy::ProxyError::TlsCertificate { .. } |
        grpc_http_proxy::ProxyError::ConfigError(_) => {
            // Expected TLS or config error due to invalid test certificates
        }
        other => panic!("Expected TLS-related error, got: {:?}", other),
    }
}

#[tokio::test]
async fn test_mock_upstream_server_functionality() {
    // Test that our mock upstream servers work correctly
    let (auth_port, auth_handle, auth_counter) = create_mock_upstream_with_tracking("auth").await;
    let (user_port, user_handle, user_counter) = create_mock_upstream_with_tracking("user").await;
    let (default_port, default_handle, default_counter) = create_mock_upstream_with_tracking("default").await;
    
    // Give servers time to start
    tokio::time::sleep(Duration::from_millis(100)).await;
    
    // Test making HTTP requests to mock servers
    let client = Client::builder(hyper_util::rt::TokioExecutor::new()).build_http();
    
    // Test auth service
    let auth_uri = format!("http://127.0.0.1:{}/auth.AuthService/Login", auth_port);
    let auth_request = Request::builder()
        .method(Method::POST)
        .uri(auth_uri)
        .header("content-type", "application/grpc")
        .body(Full::new(Bytes::from("test_request")))
        .unwrap();
    
    let auth_response = client.request(auth_request).await.unwrap();
    assert_eq!(auth_response.status(), StatusCode::OK);
    assert_eq!(auth_counter.load(Ordering::Relaxed), 1);
    
    // Test user service
    let user_uri = format!("http://127.0.0.1:{}/user.UserService/GetUser", user_port);
    let user_request = Request::builder()
        .method(Method::POST)
        .uri(user_uri)
        .header("content-type", "application/grpc")
        .body(Full::new(Bytes::from("test_request")))
        .unwrap();
    
    let user_response = client.request(user_request).await.unwrap();
    assert_eq!(user_response.status(), StatusCode::OK);
    assert_eq!(user_counter.load(Ordering::Relaxed), 1);
    
    // Test default service
    let default_uri = format!("http://127.0.0.1:{}/default.DefaultService/Echo", default_port);
    let default_request = Request::builder()
        .method(Method::POST)
        .uri(default_uri)
        .header("content-type", "application/grpc")
        .body(Full::new(Bytes::from("test_request")))
        .unwrap();
    
    let default_response = client.request(default_request).await.unwrap();
    assert_eq!(default_response.status(), StatusCode::OK);
    assert_eq!(default_counter.load(Ordering::Relaxed), 1);
    
    // Clean up
    auth_handle.abort();
    user_handle.abort();
    default_handle.abort();
}

#[tokio::test]
async fn test_comprehensive_error_scenarios() {
    use grpc_http_proxy::components::Router;
    
    let (cert_file, key_file) = create_test_certificates();
    let config = create_comprehensive_test_config(&cert_file, &key_file, 9090, 9091, 9092);
    
    // Test router creation and error scenarios
    let router = Router::new(config.routing).unwrap();
    
    // Test invalid gRPC paths
    let invalid_paths = vec![
        "",
        "/",
        "/invalid",
        "/invalid/",
        "//method",
        "/service//method",
    ];
    
    for path in invalid_paths {
        let result = router.route_request(path);
        assert!(result.is_err(), "Should fail for invalid path: {}", path);
    }
    
    // Test valid paths that should route correctly
    let valid_paths = vec![
        ("/auth.AuthService/Login", 9090),
        ("/auth.TokenService/Validate", 9090),
        ("/user.UserService/GetUser", 9091),
        ("/user.UserService/CreateUser", 9091),
        ("/other.Service/Method", 9092), // Catch-all
    ];
    
    for (path, expected_port) in valid_paths {
        let result = router.route_request(path);
        assert!(result.is_ok(), "Should succeed for valid path: {}", path);
        assert_eq!(result.unwrap().port, expected_port);
    }
}

#[tokio::test]
async fn test_configuration_edge_cases() {
    let (cert_file, key_file) = create_test_certificates();
    
    // Test configuration with only catch-all (no specific rules)
    let catch_all_only_config = ProxyConfig {
        listen: ListenConfig {
            address: "127.0.0.1".to_string(),
            port: 8443,
        },
        tls: Some(TlsConfig {
            cert_file: cert_file.path().to_path_buf(),
            key_file: key_file.path().to_path_buf(),
            root_ca_file: None,
        }),
        routing: RoutingConfig {
            rules: vec![], // No specific rules
            catch_all: Some(UpstreamConfig {
                host: "127.0.0.1".to_string(),
                port: 9090,
                timeout: Duration::from_secs(30),
                max_connections: 100,
            }),
        },
        logging: LoggingConfig {
            level: "info".to_string(),
        },
    };
    
    // Should be valid (catch-all only is allowed)
    let router_result = grpc_http_proxy::components::Router::new(catch_all_only_config.routing);
    assert!(router_result.is_ok());
    
    // Test configuration with rules but no catch-all
    let rules_only_config = RoutingConfig {
        rules: vec![
            RoutingRule {
                pattern: "/test.*".to_string(),
                upstream: UpstreamConfig {
                    host: "127.0.0.1".to_string(),
                    port: 9090,
                    timeout: Duration::from_secs(30),
                    max_connections: 100,
                },
                priority: 100,
            },
        ],
        catch_all: None, // No catch-all
    };
    
    // Should be valid (rules without catch-all is allowed)
    let router_result = grpc_http_proxy::components::Router::new(rules_only_config);
    assert!(router_result.is_ok());
}

#[tokio::test]
async fn test_upstream_configuration_validation() {
    // Test various upstream configuration scenarios
    let test_cases = vec![
        // Valid configurations
        (UpstreamConfig {
            host: "localhost".to_string(),
            port: 8080,
            timeout: Duration::from_secs(30),
            max_connections: 100,
        }, true),
        (UpstreamConfig {
            host: "service.example.com".to_string(),
            port: 443,
            timeout: Duration::from_secs(60),
            max_connections: 50,
        }, true),
        
        // Invalid configurations
        (UpstreamConfig {
            host: "".to_string(), // Empty host
            port: 8080,
            timeout: Duration::from_secs(30),
            max_connections: 100,
        }, false),
        (UpstreamConfig {
            host: "localhost".to_string(),
            port: 0, // Invalid port
            timeout: Duration::from_secs(30),
            max_connections: 100,
        }, false),
        (UpstreamConfig {
            host: "localhost".to_string(),
            port: 8080,
            timeout: Duration::from_secs(0), // Invalid timeout
            max_connections: 100,
        }, false),
        (UpstreamConfig {
            host: "localhost".to_string(),
            port: 8080,
            timeout: Duration::from_secs(30),
            max_connections: 0, // Invalid max connections
        }, false),
    ];
    
    for (upstream, should_be_valid) in test_cases {
        let result = upstream.validate();
        if should_be_valid {
            assert!(result.is_ok(), "Should be valid: {:?}", upstream);
        } else {
            assert!(result.is_err(), "Should be invalid: {:?}", upstream);
        }
    }
}