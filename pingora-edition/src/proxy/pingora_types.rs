//! HTTP/2 server types and extensions
//! This module provides HTTP/2 server functionality using hyper

use anyhow::Result;
use bytes::Bytes;
use http::{HeaderMap, Method, Uri, Version, HeaderName, HeaderValue, Request};
use std::net::SocketAddr;

// HTTP types for our proxy
pub type RequestHeader = http::request::Parts;
pub type ResponseHeader = http::response::Parts;

/// HTTP session information
pub struct Session {
    pub client_addr: SocketAddr,
    pub server_addr: SocketAddr,
    pub request_parts: http::request::Parts,
    pub request_body: Option<hyper::body::Incoming>,
}

impl Session {
    pub fn new(client_addr: SocketAddr, server_addr: SocketAddr, request: Request<hyper::body::Incoming>) -> Self {
        let (parts, body) = request.into_parts();
        Self {
            client_addr,
            server_addr,
            request_parts: parts,
            request_body: Some(body),
        }
    }

    pub fn method(&self) -> &Method {
        &self.request_parts.method
    }

    pub fn uri(&self) -> &Uri {
        &self.request_parts.uri
    }

    pub fn version(&self) -> Version {
        self.request_parts.version
    }

    pub fn headers(&self) -> &HeaderMap {
        &self.request_parts.headers
    }

    pub async fn body_bytes(&mut self) -> Result<Bytes, Box<dyn std::error::Error + Send + Sync>> {
        use http_body_util::BodyExt;
        
        if let Some(body) = self.request_body.take() {
            // Collect all the body bytes
            let collected = body.collect().await
                .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)?;
            
            Ok(collected.to_bytes())
        } else {
            // Body already consumed
            Ok(Bytes::new())
        }
    }

    /// Take the streaming body for forwarding (supports bidirectional streaming)
    pub fn take_body(&mut self) -> Option<hyper::body::Incoming> {
        self.request_body.take()
    }
}

/// Context for request processing
#[derive(Debug, Default)]
pub struct Context {
    pub upstream_address: Option<SocketAddr>,
    pub route_match: Option<String>,
    pub tls_terminated: bool,
    pub client_cert: Option<Vec<u8>>,
    pub trailer_state: Option<TrailerState>,
}

/// HTTP peer for upstream connections
#[derive(Debug, Clone)]
pub struct HttpPeer {
    pub address: SocketAddr,
    pub scheme: String,
}

impl HttpPeer {
    pub fn new(address: SocketAddr, scheme: &str) -> Self {
        Self {
            address,
            scheme: scheme.to_string(),
        }
    }
}

/// Proxy HTTP trait for handling requests
#[async_trait::async_trait]
pub trait ProxyHttp<C> {
    async fn upstream_peer(
        &self,
        session: &mut Session,
        ctx: &mut C,
    ) -> Result<Box<HttpPeer>, Box<dyn std::error::Error + Send + Sync>>;

    async fn upstream_request_filter(
        &self,
        session: &mut Session,
        upstream_request: &mut RequestHeader,
        ctx: &mut C,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;

    async fn response_filter(
        &self,
        session: &mut Session,
        upstream_response: &mut ResponseHeader,
        ctx: &mut C,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;

    async fn upstream_response_body_filter(
        &self,
        session: &mut Session,
        body: &mut Option<Bytes>,
        end_of_stream: bool,
        ctx: &mut C,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;
}

/// Custom trailer state for gRPC trailer handling
#[derive(Debug, Clone)]
pub struct TrailerState {
    pub headers: HeaderMap,
    pub grpc_status: Option<i32>,
    pub grpc_message: Option<String>,
    pub custom_trailers: HeaderMap,
}

impl TrailerState {
    pub fn new() -> Self {
        Self {
            headers: HeaderMap::new(),
            grpc_status: None,
            grpc_message: None,
            custom_trailers: HeaderMap::new(),
        }
    }

    pub fn add_header(&mut self, name: &str, value: &str) {
        if let (Ok(name), Ok(value)) = (HeaderName::from_bytes(name.as_bytes()), HeaderValue::from_str(value)) {
            self.headers.insert(name, value);
        }
    }

    pub fn add_custom_trailer(&mut self, name: &str, value: &str) {
        if let (Ok(name), Ok(value)) = (HeaderName::from_bytes(name.as_bytes()), HeaderValue::from_str(value)) {
            self.custom_trailers.insert(name, value);
        }
    }

    pub fn set_grpc_status(&mut self, status: i32, message: Option<String>) {
        self.grpc_status = Some(status);
        self.grpc_message = message;
        
        // Add to headers for forwarding
        self.add_header("grpc-status", &status.to_string());
        if let Some(msg) = &self.grpc_message {
            let msg_clone = msg.clone();
            self.add_header("grpc-message", &msg_clone);
        }
    }

    pub fn extract_from_headers(&mut self, headers: &HeaderMap) -> Result<()> {
        // Extract gRPC status
        if let Some(status_header) = headers.get("grpc-status") {
            if let Ok(status_str) = status_header.to_str() {
                if let Ok(status) = status_str.parse::<i32>() {
                    self.grpc_status = Some(status);
                }
            }
        }

        // Extract gRPC message
        if let Some(message_header) = headers.get("grpc-message") {
            if let Ok(message_str) = message_header.to_str() {
                self.grpc_message = Some(message_str.to_string());
            }
        }

        // Copy all headers that look like trailers
        for (name, value) in headers.iter() {
            let name_str = name.as_str();
            if name_str.starts_with("grpc-") || 
               name_str.starts_with("x-") ||
               name_str == "content-type" ||
               name_str == "content-length" {
                self.headers.insert(name.clone(), value.clone());
            }
        }

        Ok(())
    }

    pub fn get_all_trailers(&self) -> HeaderMap {
        let mut all_trailers = self.headers.clone();
        
        // Add gRPC status and message if set
        if let Some(status) = self.grpc_status {
            if let Ok(value) = HeaderValue::from_str(&status.to_string()) {
                all_trailers.insert("grpc-status", value);
            }
        }
        
        if let Some(message) = &self.grpc_message {
            if let Ok(value) = HeaderValue::from_str(message) {
                all_trailers.insert("grpc-message", value);
            }
        }

        // Add custom trailers
        for (name, value) in self.custom_trailers.iter() {
            all_trailers.insert(name.clone(), value.clone());
        }

        all_trailers
    }

    pub fn is_empty(&self) -> bool {
        self.headers.is_empty() && 
        self.grpc_status.is_none() && 
        self.grpc_message.is_none() &&
        self.custom_trailers.is_empty()
    }

    pub fn validate_grpc_trailers(&self) -> Result<()> {
        // Ensure we have a gRPC status
        if self.grpc_status.is_none() {
            return Err(anyhow::anyhow!("Missing gRPC status in trailers"));
        }

        // Validate status code range
        if let Some(status) = self.grpc_status {
            if status < 0 || status > 16 {
                return Err(anyhow::anyhow!("Invalid gRPC status code: {}", status));
            }
        }

        // Validate header names and values
        for (name, value) in self.headers.iter() {
            if name.as_str().is_empty() {
                return Err(anyhow::anyhow!("Empty header name in trailers"));
            }
            
            if value.to_str().is_err() {
                return Err(anyhow::anyhow!("Invalid header value for {}", name));
            }
        }

        Ok(())
    }

    pub fn handle_parsing_error(&mut self, error: &str) {
        // Set error status and add debugging information
        self.set_grpc_status(13, Some(format!("Trailer parsing error: {}", error))); // INTERNAL
        self.add_custom_trailer("x-proxy-error", "trailer-parsing-failed");
        self.add_custom_trailer("x-proxy-error-detail", error);
    }
}

impl Default for TrailerState {
    fn default() -> Self {
        Self::new()
    }
}