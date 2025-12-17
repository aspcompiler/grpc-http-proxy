# Implementation Plan

- [x] 1. Set up project structure and dependencies
  - Create new Rust project with Cargo.toml
  - Add Pingora, tokio, serde, and other core dependencies
  - Set up basic project directory structure (src/lib.rs, src/main.rs, src/config/, src/proxy/, src/routing/)
  - Configure Rust edition and basic compiler settings
  - _Requirements: All requirements depend on proper project setup_

- [x] 2. Implement configuration management
  - [x] 2.1 Create configuration data structures
    - Define ServerConfig, TlsConfig, RouteConfig, and UpstreamConfig structs
    - Implement serde serialization/deserialization for all config types
    - Add validation methods for configuration parameters
    - _Requirements: 5.1, 5.2, 5.3, 5.4_

  - [ ]* 2.2 Write property test for configuration parsing
    - **Property 11: Configuration File Processing**
    - **Validates: Requirements 5.1**

  - [ ]* 2.3 Write property test for certificate validation
    - **Property 12: Certificate Validation**
    - **Validates: Requirements 5.2, 5.3**

  - [ ]* 2.4 Write property test for invalid configuration rejection
    - **Property 13: Invalid Configuration Rejection**
    - **Validates: Requirements 5.4**

  - [x] 2.5 Implement configuration file loading and validation
    - Create ConfigManager struct with file loading capabilities
    - Implement YAML/TOML parsing with comprehensive error handling
    - Add certificate file validation (readable, valid format, not expired)
    - Add route pattern validation and conflict detection
    - _Requirements: 5.1, 5.2, 5.3, 5.4_

- [x] 3. Implement TLS management
  - [x] 3.1 Create TLS configuration and certificate loading
    - Implement TlsManager struct for certificate management
    - Add certificate and private key loading with validation
    - Implement CA certificate loading for mTLS support
    - Configure ALPN to hardcode "h2" protocol negotiation
    - _Requirements: 2.1, 2.2, 2.3_

  - [ ]* 3.2 Write property test for TLS connection acceptance
    - **Property 5: TLS Connection Acceptance**
    - **Validates: Requirements 2.1**

  - [ ]* 3.3 Write property test for client certificate validation
    - **Property 6: Client Certificate Validation**
    - **Validates: Requirements 2.2**

  - [ ]* 3.4 Write property test for ALPN negotiation
    - **Property 7: ALPN HTTP/2 Negotiation**
    - **Validates: Requirements 2.3**

  - [x] 3.5 Integrate TLS with Pingora server configuration
    - Configure Pingora's TLS settings with loaded certificates
    - Set up mTLS client certificate validation when CA is provided
    - Ensure ALPN is configured for HTTP/2 negotiation
    - _Requirements: 2.1, 2.2, 2.3_

- [x] 4. Implement routing system
  - [x] 4.1 Create routing data structures and pattern matching
    - Implement Router trait with path pattern matching logic
    - Create RouteConfig storage and lookup mechanisms
    - Implement priority-based route selection for overlapping patterns
    - Add default upstream fallback logic
    - _Requirements: 3.1, 3.2, 3.3_

  - [ ]* 4.2 Write property test for routing behavior
    - **Property 9: Comprehensive Routing Behavior**
    - **Validates: Requirements 3.1, 3.2, 3.3**

  - [x] 4.3 Implement upstream server management
    - Create UpstreamManager for connection pooling and health checking
    - Implement upstream server selection and load balancing
    - Add connection timeout and retry logic
    - _Requirements: 3.1, 3.2, 3.5_

  - [ ]* 4.4 Write property test for upstream error handling
    - **Property 10: Upstream Error Handling**
    - **Validates: Requirements 3.5**

- [x] 5. Implement core proxy service
  - [x] 5.1 Create GrpcProxyService implementing Pingora's ProxyHttp trait
    - Implement upstream_peer method for upstream server selection
    - Implement upstream_request_filter for request processing
    - Implement response_filter for response header handling
    - Implement upstream_response_body_filter for streaming and trailer handling
    - _Requirements: 1.1, 1.2, 1.3, 1.4, 1.5_

  - [ ]* 5.2 Write property test for HTTP/2 connection acceptance
    - **Property 1: HTTP/2 Connection Acceptance**
    - **Validates: Requirements 1.1**

  - [ ]* 5.3 Write property test for trailer preservation
    - **Property 2: Complete Trailer Preservation**
    - **Validates: Requirements 1.2, 1.5**

  - [ ]* 5.4 Write property test for gRPC protocol compliance
    - **Property 3: gRPC Protocol Compliance**
    - **Validates: Requirements 1.3**

  - [ ]* 5.5 Write property test for streaming without buffering
    - **Property 4: Streaming Without Buffering**
    - **Validates: Requirements 1.4**

  - [x] 5.6 Implement HTTP trailer preservation logic
    - Create TrailerState struct for managing trailer data
    - Implement trailer extraction from upstream responses
    - Ensure all trailers (including gRPC status) are forwarded to clients
    - Handle trailer parsing errors gracefully
    - _Requirements: 1.2, 1.5_

- [x] 6. Implement TLS termination and clear forwarding
  - [x] 6.1 Implement TLS termination logic in proxy service
    - Handle TLS-encrypted incoming requests
    - Decrypt requests and prepare for plain HTTP/2 forwarding
    - Maintain client certificate information for mTLS scenarios
    - _Requirements: 2.4_

  - [ ]* 6.2 Write property test for TLS termination
    - **Property 8: TLS Termination and Clear Forwarding**
    - **Validates: Requirements 2.4**

  - [x] 6.3 Ensure plain HTTP/2 support when TLS is disabled
    - Configure proxy to accept plain HTTP/2 connections
    - Handle both TLS and non-TLS scenarios in the same codebase
    - _Requirements: 2.5_

- [x] 7. Implement main server and application entry point
  - [x] 7.1 Create ProxyServer struct and main application
    - Implement server initialization with configuration loading
    - Set up Pingora server with custom GrpcProxyService
    - Add graceful shutdown handling
    - Implement basic logging and error reporting
    - _Requirements: All requirements - server orchestration_

  - [x] 7.2 Add command-line interface and configuration file specification
    - Implement CLI argument parsing for configuration file path
    - Add help text and usage information
    - Validate command-line arguments and provide clear error messages
    - _Requirements: 5.1_

- [x] 8. Checkpoint - Ensure all tests pass
  - Ensure all tests pass, ask the user if questions arise.

- [x] 9. Add comprehensive error handling and logging
  - [x] 9.1 Implement structured error types
    - Create custom error types for different failure scenarios
    - Implement proper error propagation throughout the application
    - Add context information to errors for debugging
    - _Requirements: 3.5, 5.4_

  - [x] 9.2 Add structured logging and metrics
    - Implement structured logging with appropriate log levels
    - Add request/response logging with timing information
    - Include connection and routing metrics
    - Add health check endpoint for monitoring
    - _Requirements: Operational requirements implied by all functional requirements_

- [-] 10. Create example configuration and documentation
  - [x] 10.1 Create example configuration files
    - Provide sample configuration for basic HTTP/2 proxy
    - Provide sample configuration for TLS-enabled proxy
    - Provide sample configuration for mTLS proxy with routing
    - Include comments explaining all configuration options
    - _Requirements: 5.1, 2.1, 2.2, 3.1, 3.2_

  - [ ]* 10.2 Write integration tests
    - Create end-to-end tests with real gRPC servers
    - Test TLS and mTLS scenarios with actual certificates
    - Test routing with multiple upstream servers
    - Test error scenarios and recovery

- [x] 11. Final Checkpoint - Ensure all tests pass
  - Ensure all tests pass, ask the user if questions arise.