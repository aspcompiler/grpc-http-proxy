# gRPC HTTP Proxy Design Document

## Overview

The gRPC HTTP Proxy is a high-performance Rust-based proxy server built on the Pingora framework. It serves as an intermediary between gRPC clients and gRPC servers, providing TLS termination, routing capabilities, and protocol preservation. The proxy maintains full gRPC protocol compliance while offering enterprise-grade features like mutual TLS authentication and flexible routing.

Key design principles:
- Leverage Pingora's built-in capabilities for performance and reliability
- Maintain gRPC protocol semantics, especially HTTP trailer preservation
- Support both plain HTTP/2 and TLS-terminated connections
- Provide flexible routing with path-based matching
- Enable graceful configuration management and hot reloading

## Architecture

The proxy follows a layered architecture built on Pingora's foundation:

```
┌─────────────────────────────────────────────────────────────┐
│                    gRPC Clients                            │
└─────────────────────┬───────────────────────────────────────┘
                      │ HTTP/2 + TLS (optional)
┌─────────────────────▼───────────────────────────────────────┐
│                TLS Termination Layer                       │
│  - Certificate validation (mTLS optional)                  │
│  - ALPN negotiation (h2)                                   │
│  - TLS decryption                                          │
└─────────────────────┬───────────────────────────────────────┘
                      │ HTTP/2 (decrypted)
┌─────────────────────▼───────────────────────────────────────┐
│                 Routing Layer                              │
│  - Path pattern matching                                   │
│  - Upstream server selection                               │
│  - Load balancing (if multiple upstreams)                  │
└─────────────────────┬───────────────────────────────────────┘
                      │ HTTP/2
┌─────────────────────▼───────────────────────────────────────┐
│               Protocol Preservation Layer                  │
│  - HTTP trailer forwarding                                 │
│  - gRPC status code handling                               │
│  - Streaming support                                       │
└─────────────────────┬───────────────────────────────────────┘
                      │ HTTP/2 (plain)
┌─────────────────────▼───────────────────────────────────────┐
│                 Upstream gRPC Servers                      │
└─────────────────────────────────────────────────────────────┘
```

## Components and Interfaces

### Core Components

#### 1. ProxyServer
The main server component built on Pingora's `Server` struct.
- Manages server lifecycle and configuration
- Handles graceful shutdown and reload
- Coordinates between all other components

#### 2. GrpcProxyService
Implements Pingora's `ProxyHttp` trait to handle HTTP requests.
- Processes incoming gRPC requests
- Manages request/response lifecycle
- Coordinates with routing and upstream components

#### 3. TlsManager
Handles TLS configuration and certificate management.
- Loads and validates TLS certificates and keys
- Manages CA certificates for mTLS
- Configures ALPN for HTTP/2 negotiation

#### 4. Router
Implements path-based routing logic.
- Matches request paths against configured patterns
- Selects appropriate upstream servers
- Supports default fallback routing

#### 5. UpstreamManager
Manages connections to upstream gRPC servers.
- Maintains connection pools to upstream servers
- Handles upstream health checking
- Manages load balancing across multiple upstreams

#### 6. ConfigManager
Handles configuration loading and validation.
- Parses configuration files (YAML/TOML)
- Validates configuration parameters
- Supports configuration hot-reloading

### Key Interfaces

#### ProxyService Interface
```rust
trait GrpcProxyService {
    async fn upstream_peer(&self, session: &Session, ctx: &mut Context) -> Result<Box<HttpPeer>>;
    async fn upstream_request_filter(&self, session: &Session, upstream_request: &mut RequestHeader, ctx: &mut Context) -> Result<()>;
    async fn response_filter(&self, session: &Session, upstream_response: &mut ResponseHeader, ctx: &mut Context) -> Result<()>;
    async fn upstream_response_body_filter(&self, session: &Session, body: &mut Option<Bytes>, end_of_stream: bool, ctx: &mut Context) -> Result<()>;
}
```

#### Router Interface
```rust
trait Router {
    fn route(&self, path: &str) -> Option<&UpstreamConfig>;
    fn default_upstream(&self) -> &UpstreamConfig;
}
```

#### Configuration Interface
```rust
struct ProxyConfig {
    server: ServerConfig,
    tls: Option<TlsConfig>,
    routes: Vec<RouteConfig>,
    default_upstream: UpstreamConfig,
}
```

## Data Models

### Configuration Models

#### ServerConfig
```rust
struct ServerConfig {
    bind_address: SocketAddr,
    worker_threads: Option<usize>,
    max_connections: Option<usize>,
}
```

#### TlsConfig
```rust
struct TlsConfig {
    cert_path: PathBuf,
    key_path: PathBuf,
    ca_cert_path: Option<PathBuf>, // For mTLS
    // ALPN is hardcoded to ["h2"] to prevent configuration errors
}
```

#### RouteConfig
```rust
struct RouteConfig {
    path_pattern: String,          // e.g., "/api/v1/*"
    upstream: UpstreamConfig,
    priority: Option<u32>,         // For conflict resolution
}
```

#### UpstreamConfig
```rust
struct UpstreamConfig {
    address: SocketAddr,
    connection_pool_size: Option<usize>,
    health_check: Option<HealthCheckConfig>,
    timeout: Option<Duration>,
}
```

### Runtime Models

#### ProxyContext
```rust
struct ProxyContext {
    route_match: Option<RouteConfig>,
    upstream_peer: Option<HttpPeer>,
    request_start: Instant,
    client_cert: Option<X509>,     // For mTLS
}
```

#### TrailerState
```rust
struct TrailerState {
    headers: HeaderMap,
    grpc_status: Option<i32>,
    grpc_message: Option<String>,
}
```

## Correctness Properties

*A property is a characteristic or behavior that should hold true across all valid executions of a system-essentially, a formal statement about what the system should do. Properties serve as the bridge between human-readable specifications and machine-verifiable correctness guarantees.*

Before defining the correctness properties, let me analyze the acceptance criteria for testability:

### Property Reflection

After reviewing the prework analysis, I've identified several areas where properties can be consolidated:

- Properties 1.2 and 1.5 both deal with trailer handling and can be combined into a comprehensive trailer preservation property
- Properties 3.1 and 3.2 can be combined into a single routing property that covers both default and pattern-based routing
- Properties 5.2 and 5.3 both deal with certificate validation and can be combined

### Correctness Properties

Property 1: HTTP/2 Connection Acceptance
*For any* valid HTTP/2 request sent to the proxy, the proxy should successfully accept and process the connection
**Validates: Requirements 1.1**

Property 2: Complete Trailer Preservation
*For any* gRPC response containing HTTP trailers (including status information), the proxy should preserve and forward all trailers to the client without modification
**Validates: Requirements 1.2, 1.5**

Property 3: gRPC Protocol Compliance
*For any* gRPC request forwarded through the proxy, the forwarded request should maintain complete gRPC protocol compliance
**Validates: Requirements 1.3**

Property 4: Streaming Without Buffering
*For any* gRPC streaming call (unary, server streaming, client streaming, or bidirectional), the proxy should support the streaming pattern without introducing inappropriate buffering
**Validates: Requirements 1.4**

Property 5: TLS Connection Acceptance
*For any* proxy configuration with valid TLS certificates, the proxy should accept TLS connections from clients
**Validates: Requirements 2.1**

Property 6: Client Certificate Validation
*For any* mTLS configuration with a CA certificate, the proxy should correctly validate client certificates against the provided CA
**Validates: Requirements 2.2**

Property 7: ALPN HTTP/2 Negotiation
*For any* TLS connection establishment, the proxy should negotiate HTTP/2 protocol using ALPN with the hardcoded "h2" identifier
**Validates: Requirements 2.3**

Property 8: TLS Termination and Clear Forwarding
*For any* TLS-encrypted request received by the proxy, the request should be decrypted and forwarded to the upstream server as plain HTTP/2
**Validates: Requirements 2.4**

Property 9: Comprehensive Routing Behavior
*For any* request path, the proxy should either route to the matching upstream server (if a pattern matches) or to the default upstream server (if no pattern matches), with the most specific pattern taking precedence when multiple patterns match
**Validates: Requirements 3.1, 3.2, 3.3**

Property 10: Upstream Error Handling
*For any* unreachable upstream server, the proxy should return an appropriate gRPC error status to the client
**Validates: Requirements 3.5**

Property 11: Configuration File Processing
*For any* valid configuration file provided to the proxy, the proxy should successfully read and apply the configuration
**Validates: Requirements 5.1**

Property 12: Certificate Validation
*For any* TLS or mTLS configuration, the proxy should validate that all certificate and key files are readable and cryptographically valid before starting
**Validates: Requirements 5.2, 5.3**

Property 13: Invalid Configuration Rejection
*For any* configuration containing invalid route patterns or malformed settings, the proxy should report clear configuration errors and refuse to start
**Validates: Requirements 5.4**

## Error Handling

The proxy implements comprehensive error handling at multiple layers:

### TLS Layer Errors
- Certificate validation failures (invalid certificates, expired certificates)
- Client certificate verification failures in mTLS mode
- ALPN negotiation failures
- TLS handshake timeouts

### Routing Layer Errors
- No matching route found (handled by default upstream)
- Upstream server unreachable
- Connection timeout to upstream
- Invalid route patterns in configuration

### Protocol Layer Errors
- Malformed HTTP/2 requests
- Invalid gRPC frames
- Trailer parsing errors
- Stream reset conditions

### Configuration Errors
- Missing or unreadable certificate files
- Invalid configuration file format
- Conflicting configuration parameters
- Network binding failures

Error responses maintain gRPC protocol compliance by:
- Using appropriate gRPC status codes
- Including descriptive error messages in trailers
- Maintaining HTTP/2 stream semantics
- Preserving client connection state when possible

## Testing Strategy

The testing approach combines unit testing and property-based testing to ensure comprehensive coverage:

### Unit Testing Approach
Unit tests will focus on:
- Configuration parsing and validation
- Route pattern matching logic
- Certificate loading and validation
- Error condition handling
- Integration between Pingora components

### Property-Based Testing Approach
Property-based tests will use the `proptest` crate for Rust and will be configured to run a minimum of 100 iterations per property. Each property-based test will be tagged with a comment explicitly referencing the correctness property from this design document.

Property-based tests will verify:
- HTTP/2 connection handling across various request types
- Trailer preservation for all possible trailer combinations
- gRPC protocol compliance across different message patterns
- Streaming behavior with various data sizes and timing patterns
- TLS negotiation with different certificate configurations
- Routing behavior across all possible path patterns
- Error handling with various failure scenarios
- Configuration processing with different configuration formats

Test data generators will create:
- Valid and invalid HTTP/2 requests
- Various gRPC message patterns and trailer combinations
- Different TLS certificate configurations
- Route patterns with varying complexity
- Configuration files with different parameter combinations

### Integration Testing
Integration tests will verify:
- End-to-end gRPC call flows through the proxy
- TLS termination with real certificate chains
- Multi-upstream routing scenarios
- Configuration reload behavior
- Performance characteristics under load

The testing framework will use `tokio-test` for async testing and `testcontainers` for integration testing with real gRPC servers.

## Performance Considerations

### Connection Management
- Leverage Pingora's connection pooling for upstream connections
- Implement connection keep-alive for client connections
- Use HTTP/2 multiplexing to reduce connection overhead
- Configure appropriate connection limits and timeouts

### Memory Management
- Stream processing without buffering entire messages
- Efficient trailer handling without copying
- Connection pool sizing based on expected load
- Garbage collection optimization for long-running connections

### CPU Optimization
- Minimize TLS handshake overhead through session resumption
- Efficient route matching using trie-based data structures
- Async processing to avoid blocking operations
- Worker thread configuration based on CPU cores

### Monitoring and Observability
- Metrics collection for connection counts, request rates, and error rates
- Distributed tracing support for request flow visibility
- Health check endpoints for load balancer integration
- Structured logging for debugging and analysis

## Deployment Architecture

### Single Instance Deployment
```
[gRPC Clients] → [gRPC Proxy] → [gRPC Server]
```

### Load Balanced Deployment
```
[gRPC Clients] → [Load Balancer] → [gRPC Proxy Instances] → [gRPC Servers]
```

### Service Mesh Integration
```
[gRPC Clients] → [gRPC Proxy] → [Service Discovery] → [gRPC Server Pool]
```

The proxy supports all deployment patterns through flexible configuration and Pingora's built-in load balancing capabilities.