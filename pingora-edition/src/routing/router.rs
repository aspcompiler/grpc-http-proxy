//! Router implementation for path-based routing

use crate::config::{RouteConfig, UpstreamConfig};
use crate::routing::matcher::PathMatcher;

/// Router handles path-based routing to upstream servers
pub struct Router {
    routes: Vec<RouteEntry>,
    default_upstream: UpstreamConfig,
}

/// Internal route entry with compiled pattern matcher
struct RouteEntry {
    matcher: PathMatcher,
    upstream: UpstreamConfig,
    priority: Option<u32>,
}

impl Router {
    /// Create a new router with the given routes and default upstream
    pub fn new(routes: Vec<RouteConfig>, default_upstream: UpstreamConfig) -> Self {
        // Convert RouteConfig to RouteEntry with compiled matchers
        let mut route_entries: Vec<RouteEntry> = routes
            .into_iter()
            .map(|route| RouteEntry {
                matcher: PathMatcher::new(route.path_pattern),
                upstream: route.upstream,
                priority: route.priority,
            })
            .collect();

        // Sort routes by priority (higher priority first), then by specificity
        route_entries.sort_by(|a, b| {
            match (a.priority, b.priority) {
                (Some(p1), Some(p2)) => p2.cmp(&p1), // Higher priority first
                (Some(_), None) => std::cmp::Ordering::Less, // Prioritized routes come first
                (None, Some(_)) => std::cmp::Ordering::Greater, // Non-prioritized routes come later
                (None, None) => {
                    // For routes without explicit priority, sort by specificity
                    b.matcher.specificity().cmp(&a.matcher.specificity())
                }
            }
        });

        Self {
            routes: route_entries,
            default_upstream,
        }
    }

    /// Route a request path to the appropriate upstream server
    /// Implements priority-based route selection for overlapping patterns
    /// Falls back to default upstream if no route matches
    pub fn route(&self, path: &str) -> &UpstreamConfig {
        // Find the first matching route (routes are already sorted by priority/specificity)
        for route_entry in &self.routes {
            if route_entry.matcher.matches(path) {
                return &route_entry.upstream;
            }
        }

        // No specific route matched, return default upstream
        &self.default_upstream
    }

    /// Get the default upstream server
    pub fn default_upstream(&self) -> &UpstreamConfig {
        &self.default_upstream
    }

    /// Get all configured routes (for debugging/monitoring)
    pub fn routes(&self) -> Vec<(&str, Option<u32>)> {
        self.routes
            .iter()
            .map(|entry| (entry.matcher.pattern(), entry.priority))
            .collect()
    }

    /// Find the best matching route for a given path (returns pattern and priority)
    /// This is useful for debugging and monitoring
    pub fn find_matching_route(&self, path: &str) -> Option<(&str, Option<u32>)> {
        for route_entry in &self.routes {
            if route_entry.matcher.matches(path) {
                return Some((route_entry.matcher.pattern(), route_entry.priority));
            }
        }
        None
    }
}