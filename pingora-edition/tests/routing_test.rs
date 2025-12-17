//! Unit tests for routing functionality

use grpc_http_proxy::config::{RouteConfig, UpstreamConfig};
use grpc_http_proxy::routing::{Router, PathMatcher};
use std::net::SocketAddr;

#[test]
fn test_path_matcher_exact_match() {
    let matcher = PathMatcher::new("/api/v1/users".to_string());
    
    assert!(matcher.matches("/api/v1/users"));
    assert!(!matcher.matches("/api/v1/user"));
    assert!(!matcher.matches("/api/v1/users/123"));
    assert!(!matcher.matches("/api/v2/users"));
}

#[test]
fn test_path_matcher_wildcard_match() {
    let matcher = PathMatcher::new("/api/v1/*".to_string());
    
    assert!(matcher.matches("/api/v1/"));
    assert!(matcher.matches("/api/v1/users"));
    assert!(matcher.matches("/api/v1/users/123"));
    assert!(matcher.matches("/api/v1/posts/456/comments"));
    assert!(!matcher.matches("/api/v2/users"));
    assert!(!matcher.matches("/api/v1"));
}

#[test]
fn test_path_matcher_specificity() {
    let exact_matcher = PathMatcher::new("/api/v1/users".to_string());
    let wildcard_matcher = PathMatcher::new("/api/v1/*".to_string());
    let longer_wildcard = PathMatcher::new("/api/v1/users/*".to_string());
    
    // Exact matches should have higher specificity than wildcards
    assert!(exact_matcher.specificity() > wildcard_matcher.specificity());
    assert!(exact_matcher.specificity() > longer_wildcard.specificity());
    
    // Longer wildcard prefixes should have higher specificity than shorter ones
    assert!(longer_wildcard.specificity() > wildcard_matcher.specificity());
    
    // Test the comparison method
    assert!(exact_matcher.is_more_specific_than(&wildcard_matcher));
    assert!(longer_wildcard.is_more_specific_than(&wildcard_matcher));
}

#[test]
fn test_router_default_upstream() {
    let default_upstream = create_test_upstream("127.0.0.1:9090");
    let router = Router::new(vec![], default_upstream.clone());
    
    // With no routes configured, should always return default upstream
    assert_eq!(router.route("/any/path").address, default_upstream.address);
    assert_eq!(router.route("/").address, default_upstream.address);
    assert_eq!(router.default_upstream().address, default_upstream.address);
}

#[test]
fn test_router_exact_route_matching() {
    let default_upstream = create_test_upstream("127.0.0.1:9090");
    let api_upstream = create_test_upstream("127.0.0.1:9091");
    
    let routes = vec![
        RouteConfig {
            path_pattern: "/api/v1/users".to_string(),
            upstream: api_upstream.clone(),
            priority: None,
        }
    ];
    
    let router = Router::new(routes, default_upstream.clone());
    
    // Exact match should route to api_upstream
    assert_eq!(router.route("/api/v1/users").address, api_upstream.address);
    
    // Non-matching paths should route to default
    assert_eq!(router.route("/api/v1/posts").address, default_upstream.address);
    assert_eq!(router.route("/api/v2/users").address, default_upstream.address);
    assert_eq!(router.route("/").address, default_upstream.address);
}

#[test]
fn test_router_wildcard_route_matching() {
    let default_upstream = create_test_upstream("127.0.0.1:9090");
    let api_upstream = create_test_upstream("127.0.0.1:9091");
    
    let routes = vec![
        RouteConfig {
            path_pattern: "/api/v1/*".to_string(),
            upstream: api_upstream.clone(),
            priority: None,
        }
    ];
    
    let router = Router::new(routes, default_upstream.clone());
    
    // Wildcard matches should route to api_upstream
    assert_eq!(router.route("/api/v1/users").address, api_upstream.address);
    assert_eq!(router.route("/api/v1/posts/123").address, api_upstream.address);
    assert_eq!(router.route("/api/v1/").address, api_upstream.address);
    
    // Non-matching paths should route to default
    assert_eq!(router.route("/api/v2/users").address, default_upstream.address);
    assert_eq!(router.route("/health").address, default_upstream.address);
}

#[test]
fn test_router_priority_based_selection() {
    let default_upstream = create_test_upstream("127.0.0.1:9090");
    let high_priority_upstream = create_test_upstream("127.0.0.1:9091");
    let low_priority_upstream = create_test_upstream("127.0.0.1:9092");
    
    let routes = vec![
        RouteConfig {
            path_pattern: "/api/*".to_string(),
            upstream: low_priority_upstream.clone(),
            priority: Some(1), // Lower priority
        },
        RouteConfig {
            path_pattern: "/api/v1/*".to_string(),
            upstream: high_priority_upstream.clone(),
            priority: Some(10), // Higher priority
        }
    ];
    
    let router = Router::new(routes, default_upstream.clone());
    
    // Should match the higher priority route even though both patterns match
    assert_eq!(router.route("/api/v1/users").address, high_priority_upstream.address);
    
    // Should match the lower priority route when higher priority doesn't match
    assert_eq!(router.route("/api/v2/users").address, low_priority_upstream.address);
}

#[test]
fn test_router_specificity_based_selection() {
    let default_upstream = create_test_upstream("127.0.0.1:9090");
    let specific_upstream = create_test_upstream("127.0.0.1:9091");
    let general_upstream = create_test_upstream("127.0.0.1:9092");
    
    let routes = vec![
        RouteConfig {
            path_pattern: "/api/*".to_string(),
            upstream: general_upstream.clone(),
            priority: None,
        },
        RouteConfig {
            path_pattern: "/api/v1/users".to_string(),
            upstream: specific_upstream.clone(),
            priority: None,
        }
    ];
    
    let router = Router::new(routes, default_upstream.clone());
    
    // Should match the more specific route (exact match over wildcard)
    assert_eq!(router.route("/api/v1/users").address, specific_upstream.address);
    
    // Should match the general route when specific doesn't match
    assert_eq!(router.route("/api/v1/posts").address, general_upstream.address);
    assert_eq!(router.route("/api/v2/users").address, general_upstream.address);
}

#[test]
fn test_router_find_matching_route() {
    let default_upstream = create_test_upstream("127.0.0.1:9090");
    let api_upstream = create_test_upstream("127.0.0.1:9091");
    
    let routes = vec![
        RouteConfig {
            path_pattern: "/api/v1/*".to_string(),
            upstream: api_upstream.clone(),
            priority: Some(5),
        }
    ];
    
    let router = Router::new(routes, default_upstream);
    
    // Should find the matching route
    let result = router.find_matching_route("/api/v1/users");
    assert_eq!(result, Some(("/api/v1/*", Some(5))));
    
    // Should return None for non-matching paths
    let result = router.find_matching_route("/health");
    assert_eq!(result, None);
}

#[test]
fn test_router_routes_listing() {
    let default_upstream = create_test_upstream("127.0.0.1:9090");
    let api_upstream = create_test_upstream("127.0.0.1:9091");
    let health_upstream = create_test_upstream("127.0.0.1:9092");
    
    let routes = vec![
        RouteConfig {
            path_pattern: "/api/v1/*".to_string(),
            upstream: api_upstream,
            priority: Some(10),
        },
        RouteConfig {
            path_pattern: "/health".to_string(),
            upstream: health_upstream,
            priority: None,
        }
    ];
    
    let router = Router::new(routes, default_upstream);
    let route_list = router.routes();
    
    // Should return all routes in priority order
    assert_eq!(route_list.len(), 2);
    // Higher priority route should come first
    assert_eq!(route_list[0], ("/api/v1/*", Some(10)));
    assert_eq!(route_list[1], ("/health", None));
}

// Helper function to create test upstream configurations
fn create_test_upstream(address: &str) -> UpstreamConfig {
    UpstreamConfig {
        address: address.parse::<SocketAddr>().unwrap(),
        connection_pool_size: Some(10),
        health_check: None,
        timeout: None,
    }
}