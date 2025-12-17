//! Proxy components module
//! 
//! Contains the main components of the proxy server:
//! - TLS handler for TLS termination and ALPN negotiation
//! - Router for gRPC request routing
//! - Forwarder for HTTP/2 request forwarding
//! - Health checker for upstream monitoring and circuit breaker

pub mod tls;
pub mod router;
pub mod forwarder;
pub mod health;

pub use tls::TlsHandler;
pub use router::Router;
pub use forwarder::Http2Forwarder;
pub use health::{HealthChecker, HealthStatus, CircuitBreakerState, CircuitBreakerConfig};