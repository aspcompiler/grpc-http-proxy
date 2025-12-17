//! Path pattern matching utilities

/// Path matcher for route pattern matching
pub struct PathMatcher {
    pattern: String,
    is_wildcard: bool,
    prefix: String,
}

impl PathMatcher {
    /// Create a new path matcher with the given pattern
    pub fn new(pattern: String) -> Self {
        let is_wildcard = pattern.ends_with('*');
        let prefix = if is_wildcard {
            pattern.trim_end_matches('*').to_string()
        } else {
            pattern.clone()
        };

        Self { 
            pattern,
            is_wildcard,
            prefix,
        }
    }

    /// Check if the given path matches this pattern
    pub fn matches(&self, path: &str) -> bool {
        if self.is_wildcard {
            // For wildcard patterns, check if path starts with the prefix
            path.starts_with(&self.prefix)
        } else {
            // For exact patterns, do exact matching
            self.pattern == path
        }
    }

    /// Get the pattern
    pub fn pattern(&self) -> &str {
        &self.pattern
    }

    /// Get the specificity score for this pattern (higher = more specific)
    /// Used for priority-based route selection when multiple patterns match
    pub fn specificity(&self) -> u32 {
        if self.is_wildcard {
            // Wildcard patterns have lower specificity, based on prefix length
            self.prefix.len() as u32
        } else {
            // Exact patterns have higher specificity
            1000 + self.pattern.len() as u32
        }
    }

    /// Check if this pattern is more specific than another pattern
    pub fn is_more_specific_than(&self, other: &PathMatcher) -> bool {
        self.specificity() > other.specificity()
    }
}