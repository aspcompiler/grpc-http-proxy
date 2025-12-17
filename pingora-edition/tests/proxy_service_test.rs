//! Unit tests for the GrpcProxyService

use grpc_http_proxy::config::{ProxyConfig, ServerConfig, RouteConfig, UpstreamConfig};
use grpc_http_proxy::proxy::{GrpcProxyService, ProxyHttp, Session, Context};
use std::net::SocketAddr;
use http::Method;

#[tokio::test]
async fn test_grpc_proxy_service_creation() {
    let config = create_test_config();
    let service = GrpcProxyService::new(config, None);
    
    assert!(!service.is_tls_enabled());
    assert!(service.tls_config().is_none());
}

#[tokio::test]
async fn test_upstream_peer_selection_default() {
    let config = create_test_config();
    let service = GrpcProxyService::new(config, None);
    
    let session = Session {
        client_addr: "127.0.0.1:12345".parse().unwrap(),
        server_addr: "127.0.0.1:8080".parse().unwrap(),
        method: Method::POST,
        uri: "/unknown/path".parse().unwrap(),
        version: http::Version::HTTP_2,
        headers: create_grpc_headers(),
    };
    
    let mut ctx = Context::default();
    let result = service.upstream_peer(&session, &mut ctx).await;
    
    assert!(result.is_ok());
    let peer = result.unwrap();
    assert_eq!(peer.address, "127.0.0.1:9090".parse::<SocketAddr>().unwrap());
    assert_eq!(peer.scheme, "http");
}

#[tokio::test]
async fn test_upstream_peer_selection_with_route() {
    let config = create_test_config_with_routes();
    let service = GrpcProxyService::new(config, None);
    
    let session = Session {
        client_addr: "127.0.0.1:12345".parse().unwrap(),
        server_addr: "127.0.0.1:8080".parse().unwrap(),
        method: Method::POST,
        uri: "/api/v1/users".parse().unwrap(),
        version: http::Version::HTTP_2,
        headers: create_grpc_headers(),
    };
    
    let mut ctx = Context::default();
    let result = service.upstream_peer(&session, &mut ctx).await;
    
    assert!(result.is_ok());
    let peer = result.unwrap();
    assert_eq!(peer.address, "127.0.0.1:9091".parse::<SocketAddr>().unwrap());
    assert_eq!(peer.scheme, "http");
    
    // Check that route matching information is stored in context
    assert!(ctx.route_match.is_some());
    assert!(ctx.route_match.unwrap().contains("pattern"));
}

#[tokio::test]
async fn test_upstream_request_filter_grpc_validation() {
    let config = create_test_config();
    let service = GrpcProxyService::new(config, None);
    
    let session = Session {
        client_addr: "127.0.0.1:12345".parse().unwrap(),
        server_addr: "127.0.0.1:8080".parse().unwrap(),
        method: Method::POST,
        uri: "/test.Service/Method".parse().unwrap(),
        version: http::Version::HTTP_2,
        headers: create_grpc_headers(),
    };
    
    let mut upstream_request = grpc_http_proxy::proxy::RequestHeader::new(
        Method::POST,
        "/test.Service/Method".parse().unwrap()
    );
    
    let mut ctx = Context::default();
    ctx.upstream_address = Some("127.0.0.1:9090".parse().unwrap());
    
    let result = service.upstream_request_filter(&session, &mut upstream_request, &mut ctx).await;
    assert!(result.is_ok());
    
    // Check that essential headers are copied
    assert!(upstream_request.headers.contains_key("content-type"));
    assert!(upstream_request.headers.contains_key("te"));
    assert!(upstream_request.headers.contains_key("host"));
}

#[tokio::test]
async fn test_upstream_request_filter_non_grpc_rejection() {
    let config = create_test_config();
    let service = GrpcProxyService::new(config, None);
    
    // Create a non-gRPC request (GET instead of POST)
    let session = Session {
        client_addr: "127.0.0.1:12345".parse().unwrap(),
        server_addr: "127.0.0.1:8080".parse().unwrap(),
        method: Method::GET,
        uri: "/test".parse().unwrap(),
        version: http::Version::HTTP_2,
        headers: http::HeaderMap::new(), // No gRPC content-type
    };
    
    let mut upstream_request = grpc_http_proxy::proxy::RequestHeader::new(
        Method::GET,
        "/test".parse().unwrap()
    );
    
    let mut ctx = Context::default();
    
    let result = service.upstream_request_filter(&session, &mut upstream_request, &mut ctx).await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("Invalid gRPC content type"));
}

#[tokio::test]
async fn test_response_filter() {
    let config = create_test_config();
    let service = GrpcProxyService::new(config, None);
    
    let session = Session {
        client_addr: "127.0.0.1:12345".parse().unwrap(),
        server_addr: "127.0.0.1:8080".parse().unwrap(),
        method: Method::POST,
        uri: "/test.Service/Method".parse().unwrap(),
        version: http::Version::HTTP_2,
        headers: create_grpc_headers(),
    };
    
    let mut response = grpc_http_proxy::proxy::ResponseHeader::new(200);
    let mut ctx = Context::default();
    
    let result = service.response_filter(&session, &mut response, &mut ctx).await;
    assert!(result.is_ok());
    
    // Check that CORS headers are added
    assert!(response.headers.contains_key("access-control-allow-origin"));
    assert!(response.headers.contains_key("access-control-allow-methods"));
    
    // Check that trailer state is initialized
    assert!(ctx.trailer_state.is_some());
}

#[tokio::test]
async fn test_upstream_response_body_filter_end_of_stream() {
    let config = create_test_config();
    let service = GrpcProxyService::new(config, None);
    
    let session = Session {
        client_addr: "127.0.0.1:12345".parse().unwrap(),
        server_addr: "127.0.0.1:8080".parse().unwrap(),
        method: Method::POST,
        uri: "/test.Service/Method".parse().unwrap(),
        version: http::Version::HTTP_2,
        headers: create_grpc_headers(),
    };
    
    let mut body = None;
    let mut ctx = Context::default();
    ctx.trailer_state = Some(grpc_http_proxy::proxy::TrailerState::new());
    
    // Test end of stream processing
    let result = service.upstream_response_body_filter(&session, &mut body, true, &mut ctx).await;
    assert!(result.is_ok());
    
    // Check that gRPC status is set
    let trailer_state = ctx.trailer_state.unwrap();
    assert_eq!(trailer_state.grpc_status, Some(0));
    assert_eq!(trailer_state.grpc_message, Some("OK".to_string()));
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

fn create_grpc_headers() -> http::HeaderMap {
    let mut headers = http::HeaderMap::new();
    headers.insert("content-type", "application/grpc".parse().unwrap());
    headers.insert("te", "trailers".parse().unwrap());
    headers.insert("grpc-encoding", "identity".parse().unwrap());
    headers
}