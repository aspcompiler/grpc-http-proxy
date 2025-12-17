//! Routing implementation for path-based request forwarding

pub mod router;
pub mod matcher;

pub use router::Router;
pub use matcher::PathMatcher;