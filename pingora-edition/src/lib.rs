//! gRPC HTTP Proxy
//! 
//! A high-performance HTTP proxy designed specifically to handle gRPC calls.
//! Built on the Pingora framework with support for HTTP/2, TLS termination,
//! trailer preservation, and flexible routing capabilities.

pub mod config;
pub mod error;
pub mod health;
pub mod logging;
pub mod metrics;
pub mod proxy;
pub mod routing;

pub use config::ProxyConfig;
pub use error::{ProxyError, ProxyResult, ErrorContext, ErrorContextExt};
pub use logging::{RequestContext, ProxyLogger};
pub use metrics::{metrics, MetricsCollector};
pub use health::HealthServer;