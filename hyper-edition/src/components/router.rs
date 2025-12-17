//! Router Component
//! 
//! Parses gRPC requests and determines upstream server routing

use crate::config::{RoutingConfig, RoutingRule, UpstreamConfig};
use crate::error::ProxyError;
use std::collections::HashMap;

/// Parsed gRPC request information
#[derive(Debug, Clone, PartialEq)]
pub struct GrpcRequest {
    /// Full service name (e.g., "package.service")
    pub service: String,
    /// Method name
    pub method: String,
    /// Original path
    pub path: String,
}

/// Router for handling gRPC request routing
pub struct Router {
    /// Routing rules sorted by priority (highest first)
    rules: Vec<RoutingRule>,
    /// Catch-all upstream configuration
    catch_all: Option<UpstreamConfig>,
    /// Compiled pattern cache for performance
    pattern_cache: HashMap<String, glob::Pattern>,
}

impl Router {
    /// Create a new router from routing configuration
    pub fn new(config: RoutingConfig) -> Result<Self, ProxyError> {
        let mut rules = config.rules;
        
        // Sort rules by priority (highest first), then by specificity
        rules.sort_by(|a, b| {
            // First sort by priority (descending)
            let priority_cmp = b.priority.cmp(&a.priority);
            if priority_cmp != std::cmp::Ordering::Equal {
                return priority_cmp;
            }
            
            // Then by pattern specificity (more specific patterns first)
            // Patterns with fewer wildcards are considered more specific
            let a_wildcards = a.pattern.matches('*').count();
            let b_wildcards = b.pattern.matches('*').count();
            a_wildcards.cmp(&b_wildcards)
        });

        // Pre-compile glob patterns for performance
        let mut pattern_cache = HashMap::new();
        for rule in &rules {
            let pattern = glob::Pattern::new(&rule.pattern)
                .map_err(|e| ProxyError::RoutingError(format!("Invalid glob pattern '{}': {}", rule.pattern, e)))?;
            pattern_cache.insert(rule.pattern.clone(), pattern);
        }

        Ok(Router {
            rules,
            catch_all: config.catch_all,
            pattern_cache,
        })
    }

    /// Parse gRPC URL and extract service and method information
    /// 
    /// gRPC URLs follow the format: /{package.service}/{method}
    /// Examples:
    /// - /auth.AuthService/Login
    /// - /user.UserService/GetUser
    /// - /package.subpackage.Service/Method
    pub fn parse_grpc_url(path: &str) -> Result<GrpcRequest, ProxyError> {
        // Remove leading slash if present
        let path = path.strip_prefix('/').unwrap_or(path);
        
        // gRPC paths should have exactly one slash separating service and method
        let parts: Vec<&str> = path.split('/').collect();
        
        if parts.len() != 2 {
            return Err(ProxyError::RoutingError(format!(
                "Invalid gRPC path format '{}': expected format '{{service}}/{{method}}'", 
                path
            )));
        }

        let service = parts[0];
        let method = parts[1];

        // Validate service name format
        if service.is_empty() {
            return Err(ProxyError::RoutingError(
                "Service name cannot be empty".to_string()
            ));
        }

        // Validate method name format
        if method.is_empty() {
            return Err(ProxyError::RoutingError(
                "Method name cannot be empty".to_string()
            ));
        }

        // Service names should contain at least one dot for package.service format
        if !service.contains('.') {
            return Err(ProxyError::RoutingError(format!(
                "Invalid service name '{}': expected format 'package.service'", 
                service
            )));
        }

        // Service names should not end with a dot
        if service.ends_with('.') {
            return Err(ProxyError::RoutingError(format!(
                "Invalid service name '{}': cannot end with '.'", 
                service
            )));
        }

        // Method names should be valid identifiers (alphanumeric + underscore)
        if !method.chars().all(|c| c.is_alphanumeric() || c == '_') {
            return Err(ProxyError::RoutingError(format!(
                "Invalid method name '{}': must contain only alphanumeric characters and underscores", 
                method
            )));
        }

        Ok(GrpcRequest {
            service: service.to_string(),
            method: method.to_string(),
            path: format!("/{}", path),
        })
    }

    /// Extract routing information from HTTP/2 request path
    /// 
    /// This method validates the request path and extracts gRPC routing information
    pub fn extract_routing_info(&self, path: &str) -> Result<GrpcRequest, ProxyError> {
        // Validate that this looks like a gRPC request
        if !path.starts_with('/') {
            return Err(ProxyError::RoutingError(format!(
                "Invalid request path '{}': must start with '/'", 
                path
            )));
        }

        // Parse the gRPC URL
        let grpc_request = Self::parse_grpc_url(path)?;

        // Additional validation for HTTP/2 context
        self.validate_grpc_request(&grpc_request)?;

        Ok(grpc_request)
    }

    /// Validate gRPC request for HTTP/2 context
    fn validate_grpc_request(&self, request: &GrpcRequest) -> Result<(), ProxyError> {
        // Ensure service name follows gRPC conventions
        let service_parts: Vec<&str> = request.service.split('.').collect();
        
        // Each part of the service name should be a valid identifier
        for part in &service_parts {
            if part.is_empty() {
                return Err(ProxyError::RoutingError(format!(
                    "Invalid service name '{}': empty package or service component", 
                    request.service
                )));
            }
            
            if !part.chars().next().unwrap_or('0').is_alphabetic() {
                return Err(ProxyError::RoutingError(format!(
                    "Invalid service name '{}': package/service components must start with a letter", 
                    request.service
                )));
            }
            
            if !part.chars().all(|c| c.is_alphanumeric() || c == '_') {
                return Err(ProxyError::RoutingError(format!(
                    "Invalid service name '{}': package/service components must contain only alphanumeric characters and underscores", 
                    request.service
                )));
            }
        }

        // Method name should start with uppercase letter (gRPC convention)
        if !request.method.chars().next().unwrap_or('a').is_uppercase() {
            return Err(ProxyError::RoutingError(format!(
                "Invalid method name '{}': gRPC methods should start with uppercase letter", 
                request.method
            )));
        }

        Ok(())
    }

    /// Route a gRPC request to the appropriate upstream server
    /// 
    /// This method matches the request against routing rules and returns the
    /// corresponding upstream configuration. Rules are matched in priority order
    /// (highest priority first), with most specific patterns taking precedence.
    pub fn route_request(&self, path: &str) -> Result<&UpstreamConfig, ProxyError> {
        // First, parse and validate the gRPC request
        let grpc_request = self.extract_routing_info(path)?;

        // Try to match against routing rules in priority order
        for rule in &self.rules {
            if self.matches_pattern(&rule.pattern, &grpc_request)? {
                return Ok(&rule.upstream);
            }
        }

        // If no rule matches, use catch-all route if available
        if let Some(ref catch_all) = self.catch_all {
            return Ok(catch_all);
        }

        // No matching route found
        Err(ProxyError::RoutingError(format!(
            "No route found for path: {}", 
            path
        )))
    }

    /// Check if a gRPC request matches a routing pattern
    /// 
    /// Patterns can match against:
    /// - Full service name: "auth.AuthService"
    /// - Service with method: "auth.AuthService/Login"
    /// - Wildcard patterns: "auth.*", "*.UserService", "auth.*/Login"
    /// - Full path patterns: "/auth.AuthService/*"
    fn matches_pattern(&self, pattern: &str, request: &GrpcRequest) -> Result<bool, ProxyError> {
        // Get the compiled pattern from cache
        let glob_pattern = self.pattern_cache.get(pattern)
            .ok_or_else(|| ProxyError::RoutingError(format!("Pattern not found in cache: {}", pattern)))?;

        // Try matching against different parts of the request
        let matches = vec![
            // Match against full path: "/service/method"
            glob_pattern.matches(&request.path),
            // Match against path without leading slash: "service/method"
            glob_pattern.matches(&request.path[1..]),
            // Match against service only: "service"
            glob_pattern.matches(&request.service),
            // Match against service/method: "service/method"
            glob_pattern.matches(&format!("{}/{}", request.service, request.method)),
        ];

        Ok(matches.into_iter().any(|m| m))
    }

    /// Get all routing rules (for testing and debugging)
    pub fn get_rules(&self) -> &[RoutingRule] {
        &self.rules
    }

    /// Get catch-all upstream configuration (for testing and debugging)
    pub fn get_catch_all(&self) -> Option<&UpstreamConfig> {
        self.catch_all.as_ref()
    }

    /// Find the best matching rule for a request (returns rule index and upstream)
    /// 
    /// This method is useful for debugging and testing to see which rule matched
    pub fn find_matching_rule(&self, path: &str) -> Result<Option<(usize, &UpstreamConfig)>, ProxyError> {
        // Parse and validate the gRPC request
        let grpc_request = self.extract_routing_info(path)?;

        // Try to match against routing rules in priority order
        for (index, rule) in self.rules.iter().enumerate() {
            if self.matches_pattern(&rule.pattern, &grpc_request)? {
                return Ok(Some((index, &rule.upstream)));
            }
        }

        Ok(None)
    }

    /// Check if a pattern is valid for routing
    pub fn validate_pattern(pattern: &str) -> Result<(), ProxyError> {
        // Try to compile the pattern to check if it's valid
        glob::Pattern::new(pattern)
            .map_err(|e| ProxyError::RoutingError(format!("Invalid routing pattern '{}': {}", pattern, e)))?;

        // Additional validation for gRPC-specific patterns
        if pattern.is_empty() {
            return Err(ProxyError::RoutingError("Routing pattern cannot be empty".to_string()));
        }

        // Warn about potentially problematic patterns
        if pattern == "*" {
            // This is valid but might be too broad
        }

        if pattern.starts_with("//") {
            return Err(ProxyError::RoutingError(format!(
                "Invalid pattern '{}': should not start with '//'", 
                pattern
            )));
        }

        Ok(())
    }

    /// Load routing configuration from file and create a new router
    pub fn from_config_file(config_path: &std::path::Path) -> Result<Self, ProxyError> {
        use crate::config::ProxyConfig;
        
        let config = ProxyConfig::from_file(config_path)?;
        Self::new(config.routing)
    }

    /// Update routing configuration at runtime
    /// 
    /// This method allows hot-reloading of routing rules without restarting the server
    pub fn update_config(&mut self, config: RoutingConfig) -> Result<(), ProxyError> {
        // Validate all routing rules before updating
        for rule in &config.rules {
            rule.validate()?;
            Self::validate_pattern(&rule.pattern)?;
        }

        // Validate catch-all upstream if present
        if let Some(ref catch_all) = config.catch_all {
            catch_all.validate()?;
        }

        // Sort rules by priority and specificity
        let mut rules = config.rules;
        rules.sort_by(|a, b| {
            // First sort by priority (descending)
            let priority_cmp = b.priority.cmp(&a.priority);
            if priority_cmp != std::cmp::Ordering::Equal {
                return priority_cmp;
            }
            
            // Then by pattern specificity (more specific patterns first)
            let a_wildcards = a.pattern.matches('*').count();
            let b_wildcards = b.pattern.matches('*').count();
            a_wildcards.cmp(&b_wildcards)
        });

        // Rebuild pattern cache
        let mut pattern_cache = HashMap::new();
        for rule in &rules {
            let pattern = glob::Pattern::new(&rule.pattern)
                .map_err(|e| ProxyError::RoutingError(format!("Invalid glob pattern '{}': {}", rule.pattern, e)))?;
            pattern_cache.insert(rule.pattern.clone(), pattern);
        }

        // Update router state
        self.rules = rules;
        self.catch_all = config.catch_all;
        self.pattern_cache = pattern_cache;

        Ok(())
    }

    /// Validate the entire routing configuration
    pub fn validate_routing_config(config: &RoutingConfig) -> Result<(), ProxyError> {
        // Check that we have at least one routing rule or catch-all
        if config.rules.is_empty() && config.catch_all.is_none() {
            return Err(ProxyError::RoutingError(
                "At least one routing rule or catch-all upstream must be configured".to_string()
            ));
        }

        // Validate each routing rule
        for (index, rule) in config.rules.iter().enumerate() {
            rule.validate().map_err(|e| ProxyError::RoutingError(format!("Rule {}: {}", index, e)))?;
            Self::validate_pattern(&rule.pattern).map_err(|e| ProxyError::RoutingError(format!("Rule {}: {}", index, e)))?;
        }

        // Validate catch-all upstream if present
        if let Some(ref catch_all) = config.catch_all {
            catch_all.validate().map_err(|e| ProxyError::RoutingError(format!("Catch-all upstream: {}", e)))?;
        }

        // Check for duplicate patterns
        let mut seen_patterns = std::collections::HashSet::new();
        for rule in &config.rules {
            if !seen_patterns.insert(&rule.pattern) {
                return Err(ProxyError::RoutingError(format!(
                    "Duplicate routing pattern: {}", 
                    rule.pattern
                )));
            }
        }

        Ok(())
    }

    /// Get routing statistics for monitoring and debugging
    pub fn get_routing_stats(&self) -> RoutingStats {
        RoutingStats {
            total_rules: self.rules.len(),
            has_catch_all: self.catch_all.is_some(),
            pattern_cache_size: self.pattern_cache.len(),
            rules_by_priority: self.get_rules_by_priority(),
        }
    }

    /// Get rules grouped by priority level
    fn get_rules_by_priority(&self) -> Vec<(u32, usize)> {
        let mut priority_counts: HashMap<u32, usize> = HashMap::new();
        
        for rule in &self.rules {
            *priority_counts.entry(rule.priority).or_insert(0) += 1;
        }

        let mut result: Vec<(u32, usize)> = priority_counts.into_iter().collect();
        result.sort_by(|a, b| b.0.cmp(&a.0)); // Sort by priority descending
        result
    }

    /// Optimize routing performance by pre-computing common patterns
    /// 
    /// This method can be called periodically to optimize routing performance
    /// based on request patterns
    pub fn optimize_routing(&mut self, _request_patterns: &[String]) -> Result<(), ProxyError> {
        // For now, this is a placeholder for future optimizations
        // Potential optimizations:
        // 1. Reorder rules based on request frequency
        // 2. Create fast-path lookup tables for common patterns
        // 3. Cache compiled regex patterns for complex patterns
        
        // Currently, we rely on the priority-based sorting which is already optimal
        // for most use cases
        
        Ok(())
    }
}

/// Statistics about the routing configuration
#[derive(Debug, Clone)]
pub struct RoutingStats {
    /// Total number of routing rules
    pub total_rules: usize,
    /// Whether a catch-all route is configured
    pub has_catch_all: bool,
    /// Size of the pattern cache
    pub pattern_cache_size: usize,
    /// Number of rules at each priority level
    pub rules_by_priority: Vec<(u32, usize)>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{RoutingConfig, RoutingRule, UpstreamConfig};
    use std::time::Duration;

    fn create_test_upstream(host: &str, port: u16) -> UpstreamConfig {
        UpstreamConfig {
            host: host.to_string(),
            port,
            timeout: Duration::from_secs(30),
            max_connections: 100,
        }
    }

    fn create_test_rule(pattern: &str, host: &str, port: u16, priority: u32) -> RoutingRule {
        RoutingRule {
            pattern: pattern.to_string(),
            upstream: create_test_upstream(host, port),
            priority,
        }
    }

    #[test]
    fn test_parse_grpc_url_valid() {
        // Test valid gRPC URLs
        let test_cases = vec![
            ("/auth.AuthService/Login", "auth.AuthService", "Login"),
            ("/user.UserService/GetUser", "user.UserService", "GetUser"),
            ("/package.subpackage.Service/Method", "package.subpackage.Service", "Method"),
            ("auth.AuthService/Login", "auth.AuthService", "Login"), // Without leading slash
        ];

        for (path, expected_service, expected_method) in test_cases {
            let result = Router::parse_grpc_url(path).unwrap();
            assert_eq!(result.service, expected_service);
            assert_eq!(result.method, expected_method);
            assert_eq!(result.path, format!("/{}", path.strip_prefix('/').unwrap_or(path)));
        }
    }

    #[test]
    fn test_parse_grpc_url_invalid() {
        let invalid_cases = vec![
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

        for invalid_path in invalid_cases {
            let result = Router::parse_grpc_url(invalid_path);
            assert!(result.is_err(), "Expected error for path: {}", invalid_path);
        }
    }

    #[test]
    fn test_extract_routing_info() {
        let config = RoutingConfig {
            rules: vec![],
            catch_all: Some(create_test_upstream("default", 9090)),
        };
        let router = Router::new(config).unwrap();

        // Test valid extraction
        let result = router.extract_routing_info("/auth.AuthService/Login").unwrap();
        assert_eq!(result.service, "auth.AuthService");
        assert_eq!(result.method, "Login");
        assert_eq!(result.path, "/auth.AuthService/Login");

        // Test invalid paths
        let invalid_paths = vec![
            "auth.AuthService/Login", // Missing leading slash
            "/auth/login",            // Method should be uppercase
        ];

        for path in invalid_paths {
            assert!(router.extract_routing_info(path).is_err());
        }
    }

    #[test]
    fn test_routing_rule_matching() {
        let rules = vec![
            create_test_rule("/auth.*", "auth-service", 9001, 100),
            create_test_rule("/user.UserService/*", "user-service", 9002, 90),
            create_test_rule("*.AuthService/Login", "login-service", 9003, 80),
            create_test_rule("/payment.*", "payment-service", 9004, 70),
        ];

        let config = RoutingConfig {
            rules,
            catch_all: Some(create_test_upstream("default-service", 9000)),
        };

        let router = Router::new(config).unwrap();

        // Test specific matches
        let test_cases = vec![
            ("/auth.AuthService/Login", "auth-service", 9001),
            ("/user.UserService/GetUser", "user-service", 9002),
            ("/payment.PaymentService/ProcessPayment", "payment-service", 9004),
        ];

        for (path, expected_host, expected_port) in test_cases {
            let upstream = router.route_request(path).unwrap();
            assert_eq!(upstream.host, expected_host);
            assert_eq!(upstream.port, expected_port);
        }
    }

    #[test]
    fn test_priority_based_routing() {
        let rules = vec![
            create_test_rule("/auth.*", "auth-general", 9001, 50),      // Lower priority
            create_test_rule("/auth.AuthService/*", "auth-specific", 9002, 100), // Higher priority
        ];

        let config = RoutingConfig {
            rules,
            catch_all: None,
        };

        let router = Router::new(config).unwrap();

        // The more specific rule should match due to higher priority
        let upstream = router.route_request("/auth.AuthService/Login").unwrap();
        assert_eq!(upstream.host, "auth-specific");
        assert_eq!(upstream.port, 9002);
    }

    #[test]
    fn test_catch_all_routing() {
        let config = RoutingConfig {
            rules: vec![
                create_test_rule("/auth.*", "auth-service", 9001, 100),
            ],
            catch_all: Some(create_test_upstream("catch-all-service", 9000)),
        };

        let router = Router::new(config).unwrap();

        // Test that non-matching requests go to catch-all
        let upstream = router.route_request("/unknown.Service/Method").unwrap();
        assert_eq!(upstream.host, "catch-all-service");
        assert_eq!(upstream.port, 9000);

        // Test that matching requests still go to specific service
        let upstream = router.route_request("/auth.AuthService/Login").unwrap();
        assert_eq!(upstream.host, "auth-service");
        assert_eq!(upstream.port, 9001);
    }

    #[test]
    fn test_no_route_found() {
        let config = RoutingConfig {
            rules: vec![
                create_test_rule("/auth.*", "auth-service", 9001, 100),
            ],
            catch_all: None, // No catch-all
        };

        let router = Router::new(config).unwrap();

        // Should return error when no route matches and no catch-all
        let result = router.route_request("/unknown.Service/Method");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("No route found"));
    }

    #[test]
    fn test_pattern_validation() {
        // Valid patterns
        let valid_patterns = vec![
            "/auth.*",
            "*.AuthService/*",
            "/user.UserService/GetUser",
            "*",
        ];

        for pattern in valid_patterns {
            assert!(Router::validate_pattern(pattern).is_ok(), "Pattern should be valid: {}", pattern);
        }

        // Invalid patterns
        let invalid_patterns = vec![
            "",           // Empty pattern
            "//auth.*",   // Double slash
        ];

        for pattern in invalid_patterns {
            assert!(Router::validate_pattern(pattern).is_err(), "Pattern should be invalid: {}", pattern);
        }
    }

    #[test]
    fn test_find_matching_rule() {
        let rules = vec![
            create_test_rule("/auth.*", "auth-service", 9001, 100),
            create_test_rule("/user.*", "user-service", 9002, 90),
        ];

        let config = RoutingConfig {
            rules,
            catch_all: Some(create_test_upstream("default", 9000)),
        };

        let router = Router::new(config).unwrap();

        // Test finding specific rule
        let result = router.find_matching_rule("/auth.AuthService/Login").unwrap();
        assert!(result.is_some());
        let (index, upstream) = result.unwrap();
        assert_eq!(index, 0); // First rule should match
        assert_eq!(upstream.host, "auth-service");

        // Test no matching rule (would use catch-all in route_request)
        let result = router.find_matching_rule("/unknown.Service/Method").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_routing_stats() {
        let rules = vec![
            create_test_rule("/auth.*", "auth-service", 9001, 100),
            create_test_rule("/user.*", "user-service", 9002, 100), // Same priority
            create_test_rule("/payment.*", "payment-service", 9003, 50), // Different priority
        ];

        let config = RoutingConfig {
            rules,
            catch_all: Some(create_test_upstream("default", 9000)),
        };

        let router = Router::new(config).unwrap();
        let stats = router.get_routing_stats();

        assert_eq!(stats.total_rules, 3);
        assert!(stats.has_catch_all);
        assert_eq!(stats.pattern_cache_size, 3);
        
        // Check priority distribution
        let priority_100_count = stats.rules_by_priority.iter()
            .find(|(priority, _)| *priority == 100)
            .map(|(_, count)| *count)
            .unwrap_or(0);
        assert_eq!(priority_100_count, 2);
    }

    #[test]
    fn test_update_config() {
        let initial_config = RoutingConfig {
            rules: vec![
                create_test_rule("/auth.*", "auth-service", 9001, 100),
            ],
            catch_all: None,
        };

        let mut router = Router::new(initial_config).unwrap();

        // Update with new configuration
        let new_config = RoutingConfig {
            rules: vec![
                create_test_rule("/user.*", "user-service", 9002, 100),
                create_test_rule("/auth.*", "new-auth-service", 9003, 90),
            ],
            catch_all: Some(create_test_upstream("new-default", 9000)),
        };

        router.update_config(new_config).unwrap();

        // Test that new configuration is active
        let upstream = router.route_request("/user.UserService/GetUser").unwrap();
        assert_eq!(upstream.host, "user-service");

        let upstream = router.route_request("/auth.AuthService/Login").unwrap();
        assert_eq!(upstream.host, "new-auth-service");

        let upstream = router.route_request("/unknown.Service/Method").unwrap();
        assert_eq!(upstream.host, "new-default");
    }

    #[test]
    fn test_validate_routing_config() {
        // Valid configuration
        let valid_config = RoutingConfig {
            rules: vec![
                create_test_rule("/auth.*", "auth-service", 9001, 100),
            ],
            catch_all: Some(create_test_upstream("default", 9000)),
        };
        assert!(Router::validate_routing_config(&valid_config).is_ok());

        // Invalid: no rules and no catch-all
        let invalid_config = RoutingConfig {
            rules: vec![],
            catch_all: None,
        };
        assert!(Router::validate_routing_config(&invalid_config).is_err());

        // Invalid: duplicate patterns
        let duplicate_config = RoutingConfig {
            rules: vec![
                create_test_rule("/auth.*", "auth-service-1", 9001, 100),
                create_test_rule("/auth.*", "auth-service-2", 9002, 90),
            ],
            catch_all: None,
        };
        assert!(Router::validate_routing_config(&duplicate_config).is_err());
    }

    #[test]
    fn test_complex_pattern_matching() {
        let rules = vec![
            create_test_rule("/auth.AuthService/Login", "login-specific", 9001, 100), // Exact match
            create_test_rule("/auth.AuthService/*", "auth-service", 9002, 90),        // Service wildcard
            create_test_rule("/auth.*", "auth-general", 9003, 80),                    // Package wildcard
            create_test_rule("*", "catch-all", 9004, 10),                             // Global wildcard
        ];

        let config = RoutingConfig {
            rules,
            catch_all: None,
        };

        let router = Router::new(config).unwrap();

        // Test that most specific pattern wins
        let upstream = router.route_request("/auth.AuthService/Login").unwrap();
        assert_eq!(upstream.host, "login-specific");

        // Test service-level wildcard
        let upstream = router.route_request("/auth.AuthService/Logout").unwrap();
        assert_eq!(upstream.host, "auth-service");

        // Test package-level wildcard
        let upstream = router.route_request("/auth.OtherService/Method").unwrap();
        assert_eq!(upstream.host, "auth-general");

        // Test global wildcard
        let upstream = router.route_request("/other.Service/Method").unwrap();
        assert_eq!(upstream.host, "catch-all");
    }
}