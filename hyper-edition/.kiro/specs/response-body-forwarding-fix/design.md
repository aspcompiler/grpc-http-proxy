# Response Body Forwarding Fix Design

## Overview

This design addresses the critical bug in the gRPC HTTP proxy where response bodies from upstream servers are discarded instead of being forwarded to clients. The issue occurs in the `handle_request` function where the `Incoming` body from the upstream response is replaced with an empty `Full<Bytes>` body.

## Architecture

The fix involves modifying the response handling pipeline to properly convert `Incoming` bodies to `Full<Bytes>` bodies while preserving all response data, headers, and trailers.

### Current Flow (Broken)
```
Client Request → Proxy → Upstream Server
                ↓
Client ← Empty Body ← Proxy ← Response with Body
```

### Fixed Flow
```
Client Request → Proxy → Upstream Server
                ↓
Client ← Full Body ← Proxy ← Response with Body
```

## Components and Interfaces

### 1. Response Body Converter

A new utility function to convert `Incoming` bodies to `Full<Bytes>` bodies:

```rust
async fn convert_incoming_to_full(
    incoming: hyper::body::Incoming
) -> Result<http_body_util::Full<bytes::Bytes>, hyper::Error>
```

### 2. Enhanced Handle Request Function

The `handle_request` function will be modified to:
- Properly convert response bodies from upstream
- Preserve response headers and trailers
- Handle conversion errors gracefully
- Maintain gRPC protocol semantics

### 3. Streaming Response Handler

For large responses, implement efficient streaming conversion:

```rust
async fn stream_response_body(
    response: Response<Incoming>
) -> Result<Response<Full<Bytes>>, ProxyError>
```

## Data Models

### Response Conversion Result
```rust
pub struct ConvertedResponse {
    pub response: Response<Full<Bytes>>,
    pub body_size: usize,
    pub conversion_time: Duration,
}
```

### Body Conversion Error
```rust
pub enum BodyConversionError {
    ReadError(hyper::Error),
    SizeLimit(usize),
    Timeout(Duration),
}
```

## Correctness Properties

*A property is a characteristic or behavior that should hold true across all valid executions of a system-essentially, a formal statement about what the system should do. Properties serve as the bridge between human-readable specifications and machine-verifiable correctness guarantees.*

### Property 1: Response Body Preservation
*For any* upstream response with a non-empty body, converting the response through the proxy should result in the client receiving the exact same body content
**Validates: Requirements 1.1, 1.2, 1.3**

### Property 2: Header and Trailer Preservation  
*For any* upstream response with headers and trailers, the converted response should contain all original headers and trailers
**Validates: Requirements 1.5, 3.1**

### Property 3: gRPC Status Preservation
*For any* gRPC response with status codes and messages, the converted response should preserve all gRPC status information
**Validates: Requirements 3.1, 3.2, 3.5**

### Property 4: Error Handling Consistency
*For any* body conversion error, the proxy should return an appropriate HTTP error response and log the failure
**Validates: Requirements 2.5, 4.1**

### Property 5: Memory Efficiency
*For any* response body conversion, the memory usage should not exceed reasonable bounds relative to the response size
**Validates: Requirements 2.1, 2.2**

## Error Handling

### Body Conversion Errors
- **Read Errors**: When upstream body cannot be read, return 502 Bad Gateway
- **Size Limits**: When response exceeds size limits, return 413 Payload Too Large  
- **Timeouts**: When conversion takes too long, return 504 Gateway Timeout

### Graceful Degradation
- Log all conversion errors with detailed context
- Preserve as much response metadata as possible
- Return meaningful error responses to clients

## Testing Strategy

### Unit Testing
- Test body conversion with various response sizes
- Test header and trailer preservation
- Test error handling scenarios
- Test gRPC status code preservation

### Property-Based Testing
- **Property 1**: Body content preservation across random response data
- **Property 2**: Header preservation across random header combinations
- **Property 3**: gRPC status preservation across various status codes
- **Property 4**: Error handling consistency across various error conditions
- **Property 5**: Memory usage bounds across various response sizes

### Integration Testing
- Test with real gRPC clients and servers
- Test with RouteGuide example service
- Test streaming responses
- Test large response bodies
- Test concurrent request handling

## Implementation Plan

### Phase 1: Core Body Conversion
1. Implement `convert_incoming_to_full` utility function
2. Modify `handle_request` to use proper body conversion
3. Add basic error handling and logging

### Phase 2: gRPC Protocol Support
1. Enhance trailer preservation
2. Add gRPC status code handling
3. Implement streaming response support

### Phase 3: Performance and Monitoring
1. Add response size and timing metrics
2. Implement memory usage monitoring
3. Add comprehensive error logging

### Phase 4: Testing and Validation
1. Implement property-based tests
2. Add integration tests with real gRPC services
3. Performance testing and optimization