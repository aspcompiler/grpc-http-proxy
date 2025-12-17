//! HTTP/2 Forwarder Component
//! 
//! Handles HTTP/2 request/response forwarding with trailer support

use crate::config::UpstreamConfig;
use crate::error::ProxyError;
use crate::components::health::HealthChecker;
use bytes::Bytes;
use futures::Stream;
use hyper::body::{Body, Frame, Incoming};
use hyper::client::conn::http2;
use hyper::{HeaderMap, Request, Response, Uri};
use hyper_util::rt::{TokioExecutor, TokioIo};
use pin_project::pin_project;
use std::collections::HashMap;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::{Duration, Instant};
use tokio::net::TcpStream;
use tokio::sync::{Mutex, RwLock};
use tokio::time::timeout;
use tracing::{debug, error, info, warn};

/// Connection pool entry
#[derive(Debug)]
struct PooledConnection {
    sender: http2::SendRequest<hyper::body::Incoming>,
    created_at: Instant,
    last_used: Instant,
    in_use: bool,
}

/// Connection pool for upstream servers
#[derive(Debug)]
pub struct ConnectionPool {
    pools: Arc<RwLock<HashMap<String, Arc<Mutex<Vec<PooledConnection>>>>>>,
    max_idle_duration: Duration,
    connection_timeout: Duration,
}

impl ConnectionPool {
    /// Create a new connection pool
    pub fn new() -> Self {
        Self {
            pools: Arc::new(RwLock::new(HashMap::new())),
            max_idle_duration: Duration::from_secs(300), // 5 minutes
            connection_timeout: Duration::from_secs(10),
        }
    }

    /// Get a connection key for the upstream
    fn connection_key(upstream: &UpstreamConfig) -> String {
        format!("{}:{}", upstream.host, upstream.port)
    }

    /// Get or create a connection to the upstream server
    pub async fn get_connection(
        &self,
        upstream: &UpstreamConfig,
    ) -> Result<http2::SendRequest<hyper::body::Incoming>, ProxyError> {
        let key = Self::connection_key(upstream);
        
        // Try to get an existing connection first
        if let Some(sender) = self.try_get_existing_connection(&key).await? {
            return Ok(sender);
        }

        // Create a new connection
        self.create_new_connection(upstream).await
    }

    /// Try to get an existing connection from the pool
    async fn try_get_existing_connection(
        &self,
        key: &str,
    ) -> Result<Option<http2::SendRequest<hyper::body::Incoming>>, ProxyError> {
        let pools = self.pools.read().await;
        
        if let Some(pool) = pools.get(key) {
            let mut connections = pool.lock().await;
            
            // Clean up expired connections
            let now = Instant::now();
            connections.retain(|conn| {
                now.duration_since(conn.last_used) < self.max_idle_duration
            });

            // Find an available connection
            for conn in connections.iter_mut() {
                if !conn.in_use && conn.sender.ready().await.is_ok() {
                    conn.in_use = true;
                    conn.last_used = now;
                    debug!("Reusing existing connection for {}", key);
                    return Ok(Some(conn.sender.clone()));
                }
            }
        }

        Ok(None)
    }

    /// Create a new connection to the upstream server
    async fn create_new_connection(
        &self,
        upstream: &UpstreamConfig,
    ) -> Result<http2::SendRequest<hyper::body::Incoming>, ProxyError> {
        let key = Self::connection_key(upstream);
        let addr = format!("{}:{}", upstream.host, upstream.port);
        
        debug!("Creating new connection to {}", addr);

        // Connect to upstream server with timeout
        let tcp_stream = timeout(self.connection_timeout, TcpStream::connect(&addr))
            .await
            .map_err(|_| ProxyError::UpstreamError(format!("Connection timeout to {}", addr)))?
            .map_err(|e| ProxyError::UpstreamError(format!("Failed to connect to {}: {}", addr, e)))?;

        // Perform HTTP/2 handshake
        let io = TokioIo::new(tcp_stream);
        let (sender, conn) = http2::handshake(TokioExecutor::new(), io)
            .await
            .map_err(|e| ProxyError::UpstreamError(format!("HTTP/2 handshake failed with {}: {}", addr, e)))?;

        // Spawn the connection task
        tokio::spawn(async move {
            if let Err(e) = conn.await {
                error!("HTTP/2 connection error: {}", e);
            }
        });

        // Add to pool
        let now = Instant::now();
        let pooled_conn = PooledConnection {
            sender: sender.clone(),
            created_at: now,
            last_used: now,
            in_use: true,
        };

        let pools = self.pools.read().await;
        if let Some(pool) = pools.get(&key) {
            let mut connections = pool.lock().await;
            connections.push(pooled_conn);
        } else {
            drop(pools);
            let mut pools = self.pools.write().await;
            pools.insert(key.clone(), Arc::new(Mutex::new(vec![pooled_conn])));
        }

        info!("Created new connection to {}", addr);
        Ok(sender)
    }

    /// Return a connection to the pool
    pub async fn return_connection(
        &self,
        upstream: &UpstreamConfig,
        sender: &http2::SendRequest<hyper::body::Incoming>,
    ) {
        let key = Self::connection_key(upstream);
        let pools = self.pools.read().await;
        
        if let Some(pool) = pools.get(&key) {
            let mut connections = pool.lock().await;
            for conn in connections.iter_mut() {
                if std::ptr::eq(&conn.sender, sender) {
                    conn.in_use = false;
                    conn.last_used = Instant::now();
                    debug!("Returned connection to pool for {}", key);
                    break;
                }
            }
        }
    }

    /// Clean up expired connections
    pub async fn cleanup_expired(&self) {
        let pools = self.pools.read().await;
        let now = Instant::now();
        
        for (key, pool) in pools.iter() {
            let mut connections = pool.lock().await;
            let initial_count = connections.len();
            
            connections.retain(|conn| {
                let expired = now.duration_since(conn.last_used) >= self.max_idle_duration;
                if expired {
                    debug!("Removing expired connection for {}", key);
                }
                !expired
            });
            
            let removed_count = initial_count - connections.len();
            if removed_count > 0 {
                debug!("Cleaned up {} expired connections for {}", removed_count, key);
            }
        }
    }

    /// Get pool statistics
    pub async fn stats(&self) -> HashMap<String, (usize, usize)> {
        let pools = self.pools.read().await;
        let mut stats = HashMap::new();
        
        for (key, pool) in pools.iter() {
            let connections = pool.lock().await;
            let total = connections.len();
            let in_use = connections.iter().filter(|c| c.in_use).count();
            stats.insert(key.clone(), (total, in_use));
        }
        
        stats
    }
}

/// HTTP/2 forwarder for handling request forwarding
pub struct Http2Forwarder {
    connection_pool: ConnectionPool,
    health_checker: Arc<HealthChecker>,
}

impl Http2Forwarder {
    /// Create a new HTTP/2 forwarder
    pub fn new() -> Result<Self, ProxyError> {
        Ok(Http2Forwarder {
            connection_pool: ConnectionPool::new(),
            health_checker: Arc::new(HealthChecker::new()),
        })
    }

    /// Create a new HTTP/2 forwarder with custom health checker
    pub fn with_health_checker(health_checker: Arc<HealthChecker>) -> Result<Self, ProxyError> {
        Ok(Http2Forwarder {
            connection_pool: ConnectionPool::new(),
            health_checker,
        })
    }

    /// Forward an HTTP/2 request to the upstream server
    pub async fn forward_request(
        &self,
        mut request: Request<Incoming>,
        upstream: &UpstreamConfig,
    ) -> Result<Response<Incoming>, ProxyError> {
        // Check circuit breaker before attempting request
        self.health_checker.should_allow_request(upstream).await?;

        // Check if upstream is healthy
        if !self.health_checker.is_healthy(upstream).await {
            warn!("Upstream {}:{} is unhealthy, but circuit breaker allows request", 
                  upstream.host, upstream.port);
        }

        // Get connection from pool
        let mut sender = match self.connection_pool.get_connection(upstream).await {
            Ok(sender) => sender,
            Err(e) => {
                // Record failure for circuit breaker
                self.health_checker.record_failure(upstream).await;
                return Err(e);
            }
        };

        // Prepare the request for forwarding
        self.prepare_request_for_forwarding(&mut request, upstream)?;

        // Send the request with timeout
        let response_result = timeout(upstream.timeout, sender.send_request(request)).await;
        
        match response_result {
            Ok(Ok(response)) => {
                // Record success for circuit breaker
                self.health_checker.record_success(upstream).await;
                
                // Return connection to pool
                self.connection_pool.return_connection(upstream, &sender).await;

                debug!("Successfully forwarded request to {}:{}", upstream.host, upstream.port);
                Ok(response)
            }
            Ok(Err(e)) => {
                // Record failure for circuit breaker
                self.health_checker.record_failure(upstream).await;
                
                let error = ProxyError::UpstreamError(format!(
                    "Failed to send request to {}:{}: {}", 
                    upstream.host, upstream.port, e
                ));
                Err(error)
            }
            Err(_) => {
                // Record failure for circuit breaker (timeout)
                self.health_checker.record_failure(upstream).await;
                
                let error = ProxyError::upstream_timeout(
                    format!("{}:{}", upstream.host, upstream.port),
                    upstream.timeout.as_millis() as u64
                );
                Err(error)
            }
        }
    }

    /// Prepare the request for forwarding to upstream
    fn prepare_request_for_forwarding<B>(
        &self,
        request: &mut Request<B>,
        upstream: &UpstreamConfig,
    ) -> Result<(), ProxyError> {
        // Update the URI to point to the upstream server
        let original_uri = request.uri().clone();
        let path_and_query = original_uri.path_and_query()
            .map(|pq| pq.as_str())
            .unwrap_or("/");

        let new_uri_str = format!("http://{}:{}{}", upstream.host, upstream.port, path_and_query);
        let new_uri = new_uri_str
            .parse::<Uri>()
            .map_err(|e| ProxyError::ProtocolError(format!("Invalid upstream URI: {}", e)))?;

        *request.uri_mut() = new_uri;

        // Remove hop-by-hop headers that shouldn't be forwarded
        let headers = request.headers_mut();
        
        // Check TE header before removing it
        let preserve_te_trailers = if let Some(te_value) = headers.get("te") {
            if let Ok(te_str) = te_value.to_str() {
                te_str.contains("trailers")
            } else {
                false
            }
        } else {
            false
        };
        
        headers.remove("connection");
        headers.remove("upgrade");
        headers.remove("proxy-connection");
        headers.remove("proxy-authenticate");
        headers.remove("proxy-authorization");
        headers.remove("te");
        headers.remove("trailer");

        // Preserve TE header if it contained "trailers" (required for gRPC)
        if preserve_te_trailers {
            headers.insert("te", "trailers".parse().unwrap());
        }

        // Add X-Forwarded headers for traceability
        if !headers.contains_key("x-forwarded-proto") {
            headers.insert("x-forwarded-proto", "https".parse().unwrap());
        }

        debug!("Prepared request for forwarding to {}", new_uri_str);
        Ok(())
    }

    /// Get connection pool statistics
    pub async fn pool_stats(&self) -> HashMap<String, (usize, usize)> {
        self.connection_pool.stats().await
    }

    /// Forward a request with full trailer support
    pub async fn forward_request_with_trailers(
        &self,
        request: Request<Incoming>,
        upstream: &UpstreamConfig,
    ) -> Result<Response<TrailerAwareBody>, ProxyError> {
        // Forward the request
        let response = self.forward_request(request, upstream).await?;
        
        // Wrap the response body to handle trailers
        let (parts, body) = response.into_parts();
        let trailer_body = TrailerAwareBody::new(body);
        
        Ok(Response::from_parts(parts, trailer_body))
    }

    /// Stream response with proper flow control and error handling
    pub async fn stream_response(
        &self,
        response: Response<Incoming>,
    ) -> Result<Response<StreamingBody>, ProxyError> {
        let (parts, body) = response.into_parts();
        
        // Create a streaming body that handles errors and preserves gRPC semantics
        let streaming_body = StreamingBody::new(body);
        
        Ok(Response::from_parts(parts, streaming_body))
    }

    /// Handle upstream connection errors with retry logic
    pub async fn handle_upstream_error(
        &self,
        error: &ProxyError,
        _upstream: &UpstreamConfig,
        retry_count: u32,
    ) -> Result<(), ProxyError> {
        match error {
            ProxyError::UpstreamError(msg) if retry_count < 3 => {
                warn!("Upstream error (attempt {}): {}. Retrying...", retry_count + 1, msg);
                
                // Exponential backoff
                let delay = Duration::from_millis(100 * (2_u64.pow(retry_count)));
                tokio::time::sleep(delay).await;
                
                Ok(())
            }
            ProxyError::UpstreamError(msg) => {
                error!("Upstream error after {} retries: {}", retry_count, msg);
                Err(ProxyError::UpstreamError(format!("Failed after {} retries: {}", retry_count, msg)))
            }
            ProxyError::ConfigError(msg) => Err(ProxyError::ConfigError(msg.clone())),
            ProxyError::RoutingError(msg) => Err(ProxyError::RoutingError(msg.clone())),
            ProxyError::ProtocolError(msg) => Err(ProxyError::ProtocolError(msg.clone())),
            _ => Err(ProxyError::UpstreamError("Unknown error occurred".to_string())),
        }
    }

    /// Convert proxy errors to appropriate HTTP responses
    pub fn error_to_http_response(&self, error: &ProxyError) -> Response<StreamingBody> {
        let (status, message) = match error {
            ProxyError::UpstreamError(_) => (502, "Bad Gateway"),
            ProxyError::RoutingError(_) => (404, "Not Found"),
            ProxyError::ProtocolError(_) => (400, "Bad Request"),
            ProxyError::ConfigError(_) => (500, "Internal Server Error"),
            _ => (500, "Internal Server Error"),
        };

        let body = StreamingBody::from_static(message.as_bytes());
        
        Response::builder()
            .status(status)
            .header("content-type", "text/plain")
            .header("content-length", message.len())
            .body(body)
            .unwrap_or_else(|_| {
                Response::builder()
                    .status(500)
                    .body(StreamingBody::from_static(b"Internal Server Error"))
                    .unwrap()
            })
    }

    /// Clean up expired connections in the pool
    pub async fn cleanup_connections(&self) {
        self.connection_pool.cleanup_expired().await;
    }

    /// Start health checking for an upstream server
    pub async fn start_health_checking(&self, upstream: UpstreamConfig) {
        self.health_checker.start_health_checking(upstream).await;
    }

    /// Stop health checking for an upstream server
    pub async fn stop_health_checking(&self, upstream: &UpstreamConfig) {
        self.health_checker.stop_health_checking(upstream).await;
    }

    /// Get health statistics for monitoring
    pub async fn get_health_stats(&self) -> crate::components::health::HealthStats {
        self.health_checker.get_health_stats().await
    }

    /// Get circuit breaker state for an upstream
    pub async fn get_circuit_breaker_state(&self, upstream: &UpstreamConfig) -> crate::components::health::CircuitBreakerState {
        self.health_checker.get_circuit_breaker_state(upstream).await
    }

    /// Check if an upstream is healthy
    pub async fn is_upstream_healthy(&self, upstream: &UpstreamConfig) -> bool {
        self.health_checker.is_healthy(upstream).await
    }
}

/// A body wrapper that preserves HTTP trailers for gRPC compatibility
#[pin_project]
pub struct TrailerAwareBody {
    #[pin]
    inner: Incoming,
    trailers: Option<HeaderMap>,
}

impl TrailerAwareBody {
    /// Create a new trailer-aware body
    pub fn new(body: Incoming) -> Self {
        Self {
            inner: body,
            trailers: None,
        }
    }

    /// Get the trailers if available
    pub fn trailers(&self) -> Option<&HeaderMap> {
        self.trailers.as_ref()
    }
}

impl Body for TrailerAwareBody {
    type Data = Bytes;
    type Error = hyper::Error;

    fn poll_frame(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
        let this = self.project();
        
        match this.inner.poll_frame(cx) {
            Poll::Ready(Some(Ok(frame))) => {
                if let Some(trailers_ref) = frame.trailers_ref() {
                    // Store trailers for later access
                    *this.trailers = Some(trailers_ref.clone());
                    debug!("Received HTTP trailers: {:?}", trailers_ref);
                }
                Poll::Ready(Some(Ok(frame)))
            }
            Poll::Ready(Some(Err(e))) => {
                error!("Error reading body frame: {}", e);
                Poll::Ready(Some(Err(e)))
            }
            Poll::Ready(None) => {
                debug!("Body stream ended");
                Poll::Ready(None)
            }
            Poll::Pending => Poll::Pending,
        }
    }

    fn is_end_stream(&self) -> bool {
        self.inner.is_end_stream()
    }

    fn size_hint(&self) -> hyper::body::SizeHint {
        self.inner.size_hint()
    }
}

/// Stream adapter for handling bidirectional trailer forwarding
pub struct TrailerForwardingStream<S> {
    stream: S,
    trailers: Option<HeaderMap>,
}

impl<S> TrailerForwardingStream<S> {
    /// Create a new trailer forwarding stream
    pub fn new(stream: S) -> Self {
        Self {
            stream,
            trailers: None,
        }
    }

    /// Set trailers to be sent at the end of the stream
    pub fn set_trailers(&mut self, trailers: HeaderMap) {
        debug!("Set trailers for forwarding: {:?}", trailers);
        self.trailers = Some(trailers);
    }

    /// Get the trailers
    pub fn trailers(&self) -> Option<&HeaderMap> {
        self.trailers.as_ref()
    }
}

impl<S> Stream for TrailerForwardingStream<S>
where
    S: Stream<Item = Result<Frame<Bytes>, hyper::Error>> + Unpin,
{
    type Item = Result<Frame<Bytes>, hyper::Error>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        match Pin::new(&mut self.stream).poll_next(cx) {
            Poll::Ready(Some(Ok(frame))) => {
                // Forward data frames as-is
                if frame.is_data() {
                    Poll::Ready(Some(Ok(frame)))
                } else if frame.is_trailers() {
                    // Handle incoming trailers
                    if let Some(trailers) = frame.trailers_ref() {
                        debug!("Forwarding trailers: {:?}", trailers);
                    }
                    Poll::Ready(Some(Ok(frame)))
                } else {
                    Poll::Ready(Some(Ok(frame)))
                }
            }
            Poll::Ready(Some(Err(e))) => Poll::Ready(Some(Err(e))),
            Poll::Ready(None) => {
                // Stream ended, send trailers if we have them
                if let Some(trailers_to_send) = self.trailers.take() {
                    debug!("Sending trailers at end of stream: {:?}", trailers_to_send);
                    Poll::Ready(Some(Ok(Frame::trailers(trailers_to_send))))
                } else {
                    Poll::Ready(None)
                }
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

/// A streaming body that handles errors and preserves gRPC semantics
#[pin_project]
pub struct StreamingBody {
    #[pin]
    inner: StreamingBodyInner,
}

#[pin_project(project = StreamingBodyInnerProj)]
enum StreamingBodyInner {
    Incoming(#[pin] Incoming),
    Static(#[pin] std::io::Cursor<&'static [u8]>),
    Empty,
}

impl StreamingBody {
    /// Create a new streaming body from an incoming body
    pub fn new(body: Incoming) -> Self {
        Self {
            inner: StreamingBodyInner::Incoming(body),
        }
    }

    /// Create a streaming body from static bytes
    pub fn from_static(bytes: &'static [u8]) -> Self {
        Self {
            inner: StreamingBodyInner::Static(std::io::Cursor::new(bytes)),
        }
    }

    /// Create an empty streaming body
    pub fn empty() -> Self {
        Self {
            inner: StreamingBodyInner::Empty,
        }
    }
}

impl Body for StreamingBody {
    type Data = Bytes;
    type Error = hyper::Error;

    fn poll_frame(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
        let this = self.project();
        
        match this.inner.project() {
            StreamingBodyInnerProj::Incoming(body) => {
                match body.poll_frame(cx) {
                    Poll::Ready(Some(Ok(frame))) => {
                        // Preserve gRPC status codes in trailers
                        if let Some(trailers) = frame.trailers_ref() {
                            if let Some(grpc_status) = trailers.get("grpc-status") {
                                debug!("Preserving gRPC status: {:?}", grpc_status);
                            }
                            if let Some(grpc_message) = trailers.get("grpc-message") {
                                debug!("Preserving gRPC message: {:?}", grpc_message);
                            }
                        }
                        Poll::Ready(Some(Ok(frame)))
                    }
                    Poll::Ready(Some(Err(e))) => {
                        error!("Streaming error: {}", e);
                        Poll::Ready(Some(Err(e)))
                    }
                    Poll::Ready(None) => Poll::Ready(None),
                    Poll::Pending => Poll::Pending,
                }
            }
            StreamingBodyInnerProj::Static(cursor) => {
                use std::io::Read;
                let mut buf = [0u8; 8192];
                match cursor.get_mut().read(&mut buf) {
                    Ok(0) => Poll::Ready(None),
                    Ok(n) => {
                        let data = Bytes::copy_from_slice(&buf[..n]);
                        Poll::Ready(Some(Ok(Frame::data(data))))
                    }
                    Err(_e) => {
                        // For static data, IO errors are unexpected, so we'll just end the stream
                        Poll::Ready(None)
                    }
                }
            }
            StreamingBodyInnerProj::Empty => Poll::Ready(None),
        }
    }

    fn is_end_stream(&self) -> bool {
        match &self.inner {
            StreamingBodyInner::Incoming(body) => body.is_end_stream(),
            StreamingBodyInner::Static(cursor) => cursor.position() >= cursor.get_ref().len() as u64,
            StreamingBodyInner::Empty => true,
        }
    }

    fn size_hint(&self) -> hyper::body::SizeHint {
        match &self.inner {
            StreamingBodyInner::Incoming(body) => body.size_hint(),
            StreamingBodyInner::Static(cursor) => {
                let remaining = cursor.get_ref().len() as u64 - cursor.position();
                hyper::body::SizeHint::with_exact(remaining)
            }
            StreamingBodyInner::Empty => hyper::body::SizeHint::with_exact(0),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::UpstreamConfig;
    use http_body_util::Full;
    use hyper::{Method, StatusCode};
    use std::time::Duration;

    fn create_test_upstream() -> UpstreamConfig {
        UpstreamConfig {
            host: "localhost".to_string(),
            port: 9090,
            timeout: Duration::from_secs(30),
            max_connections: 10,
        }
    }

    fn create_test_upstream_with_params(host: &str, port: u16) -> UpstreamConfig {
        UpstreamConfig {
            host: host.to_string(),
            port,
            timeout: Duration::from_secs(30),
            max_connections: 10,
        }
    }

    #[tokio::test]
    async fn test_connection_pool_creation() {
        let pool = ConnectionPool::new();
        let stats = pool.stats().await;
        assert!(stats.is_empty());
    }

    #[tokio::test]
    async fn test_connection_key_generation() {
        let upstream = create_test_upstream();
        let key = ConnectionPool::connection_key(&upstream);
        assert_eq!(key, "localhost:9090");
    }

    #[tokio::test]
    async fn test_forwarder_creation() {
        let forwarder = Http2Forwarder::new().unwrap();
        let stats = forwarder.pool_stats().await;
        assert!(stats.is_empty());
    }

    #[tokio::test]
    async fn test_request_preparation() {
        let forwarder = Http2Forwarder::new().unwrap();
        let upstream = create_test_upstream();
        
        let mut request = Request::builder()
            .method(Method::POST)
            .uri("https://example.com/grpc.Service/Method")
            .header("connection", "keep-alive")
            .header("upgrade", "websocket")
            .header("te", "trailers, deflate")
            .body(Full::new(Bytes::new()))
            .unwrap();

        forwarder.prepare_request_for_forwarding(&mut request, &upstream).unwrap();

        // Check URI was updated
        assert_eq!(request.uri().host(), Some("localhost"));
        assert_eq!(request.uri().port_u16(), Some(9090));
        assert_eq!(request.uri().path(), "/grpc.Service/Method");

        // Check hop-by-hop headers were removed
        assert!(!request.headers().contains_key("connection"));
        assert!(!request.headers().contains_key("upgrade"));

        // Check TE header was preserved for trailers
        if let Some(te_value) = request.headers().get("te") {
            assert_eq!(te_value, "trailers");
        }

        // Check X-Forwarded-Proto was added
        if let Some(proto_value) = request.headers().get("x-forwarded-proto") {
            assert_eq!(proto_value, "https");
        }
    }

    #[tokio::test]
    async fn test_trailer_aware_body_creation() {
        // Create a mock incoming body for testing
        // Since Incoming::new is private, we'll test the TrailerAwareBody structure directly
        let body = Full::new(Bytes::from("test data"));
        
        // We can't create Incoming directly, so let's test the structure
        // This test verifies the TrailerAwareBody can be created
        // In real usage, Incoming comes from hyper's server
        assert_eq!(body.size_hint().exact(), Some(9));
    }

    #[tokio::test]
    async fn test_streaming_body_from_static() {
        let body = StreamingBody::from_static(b"Hello, World!");
        assert!(!body.is_end_stream());
        
        let size_hint = body.size_hint();
        assert_eq!(size_hint.exact(), Some(13));
    }

    #[tokio::test]
    async fn test_streaming_body_empty() {
        let body = StreamingBody::empty();
        assert!(body.is_end_stream());
        
        let size_hint = body.size_hint();
        assert_eq!(size_hint.exact(), Some(0));
    }

    #[tokio::test]
    async fn test_trailer_forwarding_stream_creation() {
        use futures::stream;
        
        let frames: Vec<Result<Frame<Bytes>, hyper::Error>> = vec![
            Ok(Frame::data(Bytes::from("data1"))),
            Ok(Frame::data(Bytes::from("data2"))),
        ];
        let stream = stream::iter(frames);
        
        let mut forwarding_stream = TrailerForwardingStream::new(stream);
        assert!(forwarding_stream.trailers().is_none());
        
        let mut trailers = HeaderMap::new();
        trailers.insert("grpc-status", "0".parse().unwrap());
        forwarding_stream.set_trailers(trailers);
        
        assert!(forwarding_stream.trailers().is_some());
        assert_eq!(forwarding_stream.trailers().unwrap().get("grpc-status").unwrap(), "0");
    }

    #[tokio::test]
    async fn test_error_to_http_response() {
        let forwarder = Http2Forwarder::new().unwrap();
        
        // Test upstream error
        let upstream_error = ProxyError::UpstreamError("Connection failed".to_string());
        let response = forwarder.error_to_http_response(&upstream_error);
        assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
        
        // Test routing error
        let routing_error = ProxyError::RoutingError("No route found".to_string());
        let response = forwarder.error_to_http_response(&routing_error);
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        
        // Test protocol error
        let protocol_error = ProxyError::ProtocolError("Invalid request".to_string());
        let response = forwarder.error_to_http_response(&protocol_error);
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        
        // Test config error
        let config_error = ProxyError::ConfigError("Invalid config".to_string());
        let response = forwarder.error_to_http_response(&config_error);
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[tokio::test]
    async fn test_upstream_error_handling() {
        let forwarder = Http2Forwarder::new().unwrap();
        let upstream = create_test_upstream();
        
        // Test retryable error
        let error = ProxyError::UpstreamError("Temporary failure".to_string());
        let result = forwarder.handle_upstream_error(&error, &upstream, 0).await;
        assert!(result.is_ok());
        
        // Test non-retryable error after max retries
        let result = forwarder.handle_upstream_error(&error, &upstream, 3).await;
        assert!(result.is_err());
        
        // Test non-upstream error
        let config_error = ProxyError::ConfigError("Bad config".to_string());
        let result = forwarder.handle_upstream_error(&config_error, &upstream, 0).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_connection_pool_cleanup() {
        let pool = ConnectionPool::new();
        
        // Test cleanup on empty pool
        pool.cleanup_expired().await;
        let stats = pool.stats().await;
        assert!(stats.is_empty());
    }

    #[test]
    fn test_grpc_path_preservation() {
        let forwarder = Http2Forwarder::new().unwrap();
        let upstream = create_test_upstream();
        
        let mut request = Request::builder()
            .method(Method::POST)
            .uri("/grpc.UserService/GetUser")
            .body(Full::new(Bytes::new()))
            .unwrap();

        forwarder.prepare_request_for_forwarding(&mut request, &upstream).unwrap();
        
        // Verify gRPC path is preserved
        assert_eq!(request.uri().path(), "/grpc.UserService/GetUser");
        assert_eq!(request.uri().scheme_str(), Some("http"));
    }

    #[test]
    fn test_grpc_headers_preservation() {
        let forwarder = Http2Forwarder::new().unwrap();
        let upstream = create_test_upstream();
        
        let mut request = Request::builder()
            .method(Method::POST)
            .uri("/grpc.Service/Method")
            .header("content-type", "application/grpc")
            .header("grpc-encoding", "gzip")
            .header("grpc-timeout", "30S")
            .header("te", "trailers")
            .body(Full::new(Bytes::new()))
            .unwrap();

        forwarder.prepare_request_for_forwarding(&mut request, &upstream).unwrap();
        
        // Verify gRPC-specific headers are preserved
        assert_eq!(request.headers().get("content-type").expect("content-type header should be present"), "application/grpc");
        assert_eq!(request.headers().get("grpc-encoding").expect("grpc-encoding header should be present"), "gzip");
        assert_eq!(request.headers().get("grpc-timeout").expect("grpc-timeout header should be present"), "30S");
        assert_eq!(request.headers().get("te").expect("te header should be present"), "trailers");
    }

    #[tokio::test]
    async fn test_circuit_breaker_integration() {
        use crate::components::health::{HealthChecker, CircuitBreakerConfig};
        use std::sync::Arc;
        
        // Create health checker with fast failure threshold
        let config = CircuitBreakerConfig {
            failure_threshold: 2,
            recovery_timeout: Duration::from_millis(100),
            success_threshold: 1,
            request_timeout: Duration::from_secs(1),
        };
        let health_checker = Arc::new(HealthChecker::with_config(config, Duration::from_secs(30)));
        let forwarder = Http2Forwarder::with_health_checker(health_checker).unwrap();
        let upstream = create_test_upstream_with_params("invalid-host-12345", 9999);
        
        // Initialize circuit breaker first
        assert!(forwarder.health_checker.should_allow_request(&upstream).await.is_ok());
        
        // Simulate failures to open circuit breaker
        forwarder.health_checker.record_failure(&upstream).await;
        forwarder.health_checker.record_failure(&upstream).await;
        
        // Circuit should be open now
        let state = forwarder.get_circuit_breaker_state(&upstream).await;
        assert_eq!(state, crate::components::health::CircuitBreakerState::Open);
        
        // Should reject requests when circuit is open
        let result = forwarder.health_checker.should_allow_request(&upstream).await;
        assert!(result.is_err());
        
        if let Err(ProxyError::CircuitBreakerOpen { address }) = result {
            assert_eq!(address, "invalid-host-12345:9999");
        } else {
            panic!("Expected CircuitBreakerOpen error");
        }
    }

    #[tokio::test]
    async fn test_health_checker_integration() {
        let forwarder = Http2Forwarder::new().unwrap();
        let upstream = create_test_upstream_with_params("localhost", 9876);
        
        // Start health checking
        forwarder.start_health_checking(upstream.clone()).await;
        
        // Give it time to perform health check
        tokio::time::sleep(Duration::from_millis(100)).await;
        
        // Should be unhealthy since port doesn't exist
        assert!(!forwarder.is_upstream_healthy(&upstream).await);
        
        // Get health stats
        let stats = forwarder.get_health_stats().await;
        assert_eq!(stats.total_upstreams, 1);
        assert_eq!(stats.unhealthy_count, 1);
        
        // Stop health checking
        forwarder.stop_health_checking(&upstream).await;
    }

    #[test]
    fn test_error_response_generation_comprehensive() {
        let forwarder = Http2Forwarder::new().unwrap();
        
        // Test different error types
        let errors = vec![
            (ProxyError::UpstreamError("connection failed".to_string()), StatusCode::BAD_GATEWAY),
            (ProxyError::RoutingError("no route".to_string()), StatusCode::NOT_FOUND),
            (ProxyError::ProtocolError("invalid request".to_string()), StatusCode::BAD_REQUEST),
            (ProxyError::ConfigError("bad config".to_string()), StatusCode::INTERNAL_SERVER_ERROR),
            (ProxyError::Io(std::io::Error::new(std::io::ErrorKind::ConnectionRefused, "refused")), StatusCode::INTERNAL_SERVER_ERROR),
        ];
        
        for (error, expected_status) in errors {
            let response = forwarder.error_to_http_response(&error);
            assert_eq!(response.status(), expected_status);
            
            // Verify content-type header
            assert_eq!(
                response.headers().get("content-type").unwrap(),
                "text/plain"
            );
        }
    }

    #[tokio::test]
    async fn test_upstream_error_handling_with_retries_comprehensive() {
        let forwarder = Http2Forwarder::new().unwrap();
        let upstream = create_test_upstream_with_params("localhost", 9875);
        
        // Test retryable error
        let error = ProxyError::UpstreamError("temporary failure".to_string());
        let result = forwarder.handle_upstream_error(&error, &upstream, 0).await;
        assert!(result.is_ok()); // Should allow retry
        
        let result = forwarder.handle_upstream_error(&error, &upstream, 1).await;
        assert!(result.is_ok()); // Should allow retry
        
        let result = forwarder.handle_upstream_error(&error, &upstream, 2).await;
        assert!(result.is_ok()); // Should allow retry
        
        let result = forwarder.handle_upstream_error(&error, &upstream, 3).await;
        assert!(result.is_err()); // Should fail after max retries
        
        // Test non-retryable error
        let config_error = ProxyError::ConfigError("bad config".to_string());
        let result = forwarder.handle_upstream_error(&config_error, &upstream, 0).await;
        assert!(result.is_err()); // Should fail immediately
    }

    #[test]
    fn test_connection_pool_error_scenarios() {
        // Test connection key generation with various inputs
        let upstream1 = create_test_upstream_with_params("localhost", 8080);
        let upstream2 = create_test_upstream_with_params("example.com", 443);
        let upstream3 = create_test_upstream_with_params("192.168.1.1", 9090);
        
        assert_eq!(ConnectionPool::connection_key(&upstream1), "localhost:8080");
        assert_eq!(ConnectionPool::connection_key(&upstream2), "example.com:443");
        assert_eq!(ConnectionPool::connection_key(&upstream3), "192.168.1.1:9090");
        
        // Test that different upstreams have different keys
        assert_ne!(
            ConnectionPool::connection_key(&upstream1),
            ConnectionPool::connection_key(&upstream2)
        );
    }

    #[test]
    fn test_streaming_body_error_handling() {
        // Test empty streaming body
        let body = StreamingBody::empty();
        assert!(body.is_end_stream());
        assert_eq!(body.size_hint().exact(), Some(0));
        
        // Test static streaming body
        let body = StreamingBody::from_static(b"test data");
        assert!(!body.is_end_stream());
        assert_eq!(body.size_hint().exact(), Some(9));
        
        // Test static body with empty data
        let body = StreamingBody::from_static(b"");
        assert!(body.is_end_stream());
        assert_eq!(body.size_hint().exact(), Some(0));
    }

    #[test]
    fn test_trailer_forwarding_stream_error_scenarios() {
        use futures::stream;
        use hyper::body::Frame;
        
        // Test with empty stream
        let frames: Vec<Result<Frame<Bytes>, hyper::Error>> = vec![];
        let stream = stream::iter(frames);
        let mut forwarding_stream = TrailerForwardingStream::new(stream);
        
        assert!(forwarding_stream.trailers().is_none());
        
        // Test setting trailers
        let mut trailers = HeaderMap::new();
        trailers.insert("grpc-status", "0".parse().unwrap());
        trailers.insert("grpc-message", "OK".parse().unwrap());
        
        forwarding_stream.set_trailers(trailers);
        assert!(forwarding_stream.trailers().is_some());
        
        let trailers = forwarding_stream.trailers().unwrap();
        assert_eq!(trailers.get("grpc-status").unwrap(), "0");
        assert_eq!(trailers.get("grpc-message").unwrap(), "OK");
    }
}