# gRPC Streaming and Trailers Fix Design

## Overview

This design addresses the critical issue where the proxy buffers entire response bodies in memory and drops HTTP trailers, breaking gRPC bidirectional streaming. The solution involves implementing true streaming body forwarding with trailer preservation.

## Architecture

The fix involves replacing the body buffering approach with a streaming approach that preserves HTTP trailers and maintains constant memory usage.

### Current Flow (Broken for Streaming)
```
Client ← Full<Bytes> (buffered) ← Proxy ← Incoming (streaming) ← Upstream
                ↓
        Trailers Lost
```

### Fixed Flow (Streaming with Trailers)
```
Client ← Streaming Body + Trailers ← Proxy ← Incoming + Trailers ← Upstream
```

## Components and Interfaces

### 1. Streaming Body Adapter

Replace the buffering approach with a streaming adapter:

```rust
pub struct StreamingBodyAdapter {
    inner: Incoming,
}

impl Body for StreamingBodyAdapter {
    type Data = Bytes;
    type Error = hyper::Error;
    
    fn poll_frame(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>>;
}
```

### 2. Trailer-Aware Response Handler

Implement proper trailer forwarding:

```rust
async fn forward_response_with_trailers(
    response: Response<Incoming>
) -> Result<Response<StreamingBodyAdapter>, ProxyError>
```

### 3. Memory-Efficient Body Conversion

Replace the memory-buffering conversion with streaming:

```rust
fn convert_to_streaming_body(incoming: Incoming) -> StreamingBodyAdapter
```

## Data Models

### Streaming Response Wrapper
```rust
pub struct StreamingResponse {
    pub response: Response<StreamingBodyAdapter>,
    pub started_at: Instant,
}
```

### Trailer Forwarding State
```rust
pub struct TrailerState {
    pub headers_forwarded: bool,
    pub trailers_received: Option<HeaderMap>,
    pub stream_complete: bool,
}
```

## Correctness Properties

*A property is a characteristic or behavior that should hold true across all valid executions of a system-essentially, a formal statement about what the system should do. Properties serve as the bridge between human-readable specifications and machine-verifiable correctness guarantees.*

### Property 1: Streaming Preservation
*For any* streaming response from upstream, the proxy should forward data frames immediately without buffering the entire response
**Validates: Requirements 1.1, 1.2, 1.3**

### Property 2: Trailer Preservation
*For any* response with HTTP trailers, all trailers should be forwarded to the client in the correct order
**Validates: Requirements 2.1, 2.2, 2.4, 2.5**

### Property 3: Memory Efficiency
*For any* response size, the proxy memory usage should remain constant and not grow with response size
**Validates: Requirements 3.1, 3.2, 3.3**

### Property 4: gRPC Status Preservation
*For any* gRPC response with status trailers, the grpc-status and grpc-message should be preserved exactly
**Validates: Requirements 2.2, 2.3**

### Property 5: Flow Control Preservation
*For any* streaming response, HTTP/2 flow control and backpressure should be maintained end-to-end
**Validates: Requirements 1.5, 3.4**

## Error Handling

### Streaming Errors
- **Stream Interruption**: Handle upstream stream failures gracefully
- **Trailer Loss**: Detect and log when trailers are not received
- **Flow Control Issues**: Handle backpressure and flow control errors

### Graceful Degradation
- Continue streaming even if trailer forwarding fails
- Log all streaming issues with detailed context
- Maintain connection stability during streaming errors

## Testing Strategy

### Unit Testing
- Test streaming body adapter functionality
- Test trailer preservation logic
- Test memory usage patterns
- Test error handling in streaming scenarios

### Property-Based Testing
- **Property 1**: Streaming data preservation across various response patterns
- **Property 2**: Trailer preservation across different trailer combinations
- **Property 3**: Memory usage bounds across various response sizes
- **Property 4**: gRPC status preservation across different status codes
- **Property 5**: Flow control behavior across different streaming patterns

### Integration Testing
- Test with real gRPC bidirectional streaming
- Test RouteGuide streaming methods
- Test large response streaming
- Test concurrent streaming calls
- Test trailer timing and ordering

## Implementation Plan

### Phase 1: Streaming Body Adapter
1. Create `StreamingBodyAdapter` that wraps `Incoming`
2. Implement `Body` trait with proper frame forwarding
3. Replace buffering conversion with streaming conversion

### Phase 2: Trailer Preservation
1. Enhance frame forwarding to preserve trailers
2. Add trailer logging and debugging
3. Test trailer forwarding with gRPC calls

### Phase 3: Memory Optimization
1. Verify constant memory usage
2. Add memory usage monitoring
3. Performance testing and optimization

### Phase 4: Integration and Validation
1. Test with RouteGuide bidirectional streaming
2. Validate all gRPC streaming patterns work
3. Performance and memory usage validation

## Key Changes Required

### 1. Replace Body Buffering
```rust
// OLD (buffers entire body):
let body_bytes = incoming.collect().await?.to_bytes();
Ok(Full::new(body_bytes))

// NEW (streaming):
Ok(StreamingBodyAdapter::new(incoming))
```

### 2. Update Response Type
```rust
// OLD:
Result<Response<Full<Bytes>>, ProxyError>

// NEW:
Result<Response<StreamingBodyAdapter>, ProxyError>
```

### 3. Preserve Trailers in Streaming
```rust
impl Body for StreamingBodyAdapter {
    fn poll_frame(&mut self, cx: &mut Context<'_>) -> Poll<Option<Result<Frame<Bytes>, Error>>> {
        match self.inner.poll_frame(cx) {
            Poll::Ready(Some(Ok(frame))) => {
                if frame.is_trailers() {
                    // Log and preserve trailers
                    debug!("Forwarding HTTP trailers: {:?}", frame.trailers_ref());
                }
                Poll::Ready(Some(Ok(frame)))
            }
            // ... handle other cases
        }
    }
}
```

This approach will fix the bidirectional streaming issues and ensure proper gRPC trailer handling without memory bloat.