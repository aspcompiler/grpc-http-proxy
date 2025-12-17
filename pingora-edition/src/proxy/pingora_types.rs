//! Mock Pingora types and traits for development
//! This module provides the necessary types and traits that would normally come from Pingora

use anyhow::{anyhow, Result};
use bytes::Bytes;
use http::{HeaderMap, Method, Uri, Version, HeaderName, HeaderValue};
use std::net::SocketAddr;

/// Mock Session type representing an HTTP session
#[derive(Debug)]
pub struct Session {
    pub client_addr: SocketAddr,
    pub server_addr: SocketAddr,
    pub method: Method,
    pub uri: Uri,
    pub version: Version,
    pub headers: HeaderMap,
}

impl Session {
    pub fn new(client_addr: SocketAddr, server_addr: SocketAddr) -> Self {
        Self {
            client_addr,
            server_addr,
            method: Method::GET,
            uri: Uri::from_static("/"),
            version: Version::HTTP_2,
            headers: HeaderMap::new(),
        }
    }

    pub fn client_addr(&self) -> SocketAddr {
        self.client_addr
    }

    pub fn uri(&self) -> &Uri {
        &self.uri
    }

    pub fn method(&self) -> &Method {
        &self.method
    }

    pub fn headers(&self) -> &HeaderMap {
        &self.headers
    }
}

/// Mock Context type for request processing
#[derive(Debug, Default)]
pub struct Context {
    pub route_match: Option<String>,
    pub upstream_address: Option<SocketAddr>,
    pub trailer_state: Option<TrailerState>,
    pub client_cert: Option<Vec<u8>>, // Client certificate for mTLS
    pub tls_terminated: bool, // Whether TLS was terminated for this request
}

/// Trailer state for managing HTTP trailers
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

    /// Add a header to the trailer state
    pub fn add_header(&mut self, name: &str, value: &str) {
        if let (Ok(name), Ok(value)) = (name.parse::<HeaderName>(), value.parse::<HeaderValue>()) {
            self.headers.insert(name, value);
        }
    }

    /// Add a custom trailer (non-gRPC specific)
    pub fn add_custom_trailer(&mut self, name: &str, value: &str) {
        if let (Ok(name), Ok(value)) = (name.parse::<HeaderName>(), value.parse::<HeaderValue>()) {
            self.custom_trailers.insert(name, value);
        }
    }

    /// Set gRPC status and message
    pub fn set_grpc_status(&mut self, status: i32, message: Option<String>) {
        self.grpc_status = Some(status);
        self.grpc_message = message.clone();
        
        // Also add to headers for consistency
        self.add_header("grpc-status", &status.to_string());
        if let Some(msg) = &message {
            self.add_header("grpc-message", msg);
        }
    }

    /// Extract trailers from upstream response headers
    pub fn extract_from_headers(&mut self, headers: &HeaderMap) -> Result<()> {
        for (name, value) in headers.iter() {
            let name_str = name.as_str().to_lowercase();
            
            // Handle gRPC-specific trailers
            match name_str.as_str() {
                "grpc-status" => {
                    if let Ok(status_str) = value.to_str() {
                        if let Ok(status) = status_str.parse::<i32>() {
                            self.grpc_status = Some(status);
                            self.add_header("grpc-status", status_str);
                        }
                    }
                }
                "grpc-message" => {
                    if let Ok(message) = value.to_str() {
                        self.grpc_message = Some(message.to_string());
                        self.add_header("grpc-message", message);
                    }
                }
                _ => {
                    // Preserve all other trailers
                    if let Ok(value_str) = value.to_str() {
                        self.add_custom_trailer(name.as_str(), value_str);
                    }
                }
            }
        }
        Ok(())
    }

    /// Get all trailers as a single HeaderMap
    pub fn get_all_trailers(&self) -> HeaderMap {
        let mut all_trailers = self.headers.clone();
        
        // Add custom trailers
        for (name, value) in self.custom_trailers.iter() {
            all_trailers.insert(name.clone(), value.clone());
        }
        
        // Ensure gRPC status is always present
        if self.grpc_status.is_some() && !all_trailers.contains_key("grpc-status") {
            if let Some(status) = self.grpc_status {
                if let Ok(value) = status.to_string().parse::<HeaderValue>() {
                    all_trailers.insert("grpc-status", value);
                }
            }
        }
        
        // Ensure gRPC message is present if we have one
        if self.grpc_message.is_some() && !all_trailers.contains_key("grpc-message") {
            if let Some(ref message) = self.grpc_message {
                if let Ok(value) = message.parse::<HeaderValue>() {
                    all_trailers.insert("grpc-message", value);
                }
            }
        }
        
        all_trailers
    }

    /// Check if trailer state is empty
    pub fn is_empty(&self) -> bool {
        self.headers.is_empty() && 
        self.custom_trailers.is_empty() && 
        self.grpc_status.is_none()
    }

    /// Validate that trailers are properly formatted for gRPC
    pub fn validate_grpc_trailers(&self) -> Result<()> {
        // gRPC requires a status trailer
        if self.grpc_status.is_none() {
            return Err(anyhow!("gRPC status trailer is required"));
        }

        // Validate status code is in valid range
        if let Some(status) = self.grpc_status {
            if status < 0 || status > 16 {
                return Err(anyhow!("Invalid gRPC status code: {}", status));
            }
        }

        // Validate header names and values
        for (name, value) in self.headers.iter().chain(self.custom_trailers.iter()) {
            // Header names should be lowercase
            let name_str = name.as_str();
            if name_str != name_str.to_lowercase() {
                return Err(anyhow!("Trailer header names must be lowercase: {}", name_str));
            }

            // Validate header value is valid ASCII
            if value.to_str().is_err() {
                return Err(anyhow!("Trailer header values must be valid ASCII: {}", name_str));
            }
        }

        Ok(())
    }

    /// Handle trailer parsing errors gracefully
    pub fn handle_parsing_error(&mut self, error: &str) {
        // Set error status
        self.set_grpc_status(13, Some(format!("Trailer parsing error: {}", error))); // INTERNAL error
        
        // Add error information as custom trailer
        self.add_custom_trailer("x-proxy-error", error);
    }
}

/// Mock RequestHeader type
#[derive(Debug, Clone)]
pub struct RequestHeader {
    pub method: Method,
    pub uri: Uri,
    pub version: Version,
    pub headers: HeaderMap,
}

impl RequestHeader {
    pub fn new(method: Method, uri: Uri) -> Self {
        Self {
            method,
            uri,
            version: Version::HTTP_2,
            headers: HeaderMap::new(),
        }
    }

    pub fn insert_header(&mut self, name: &str, value: &str) {
        if let (Ok(name), Ok(value)) = (name.parse::<HeaderName>(), value.parse::<HeaderValue>()) {
            self.headers.insert(name, value);
        }
    }
}

/// Mock ResponseHeader type
#[derive(Debug, Clone)]
pub struct ResponseHeader {
    pub status: u16,
    pub version: Version,
    pub headers: HeaderMap,
}

impl ResponseHeader {
    pub fn new(status: u16) -> Self {
        Self {
            status,
            version: Version::HTTP_2,
            headers: HeaderMap::new(),
        }
    }

    pub fn insert_header(&mut self, name: &str, value: &str) {
        if let (Ok(name), Ok(value)) = (name.parse::<HeaderName>(), value.parse::<HeaderValue>()) {
            self.headers.insert(name, value);
        }
    }

    pub fn status(&self) -> u16 {
        self.status
    }
}

/// Mock HttpPeer type representing an upstream server
#[derive(Debug)]
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

/// Mock ProxyHttp trait that services must implement
#[async_trait::async_trait]
pub trait ProxyHttp: Send + Sync {
    /// Select the upstream peer for this request
    async fn upstream_peer(
        &self,
        session: &Session,
        ctx: &mut Context,
    ) -> Result<Box<HttpPeer>>;

    /// Filter/modify the upstream request before sending
    async fn upstream_request_filter(
        &self,
        session: &Session,
        upstream_request: &mut RequestHeader,
        ctx: &mut Context,
    ) -> Result<()>;

    /// Filter/modify the response headers from upstream
    async fn response_filter(
        &self,
        session: &Session,
        upstream_response: &mut ResponseHeader,
        ctx: &mut Context,
    ) -> Result<()>;

    /// Filter/modify the response body and handle trailers
    async fn upstream_response_body_filter(
        &self,
        session: &Session,
        body: &mut Option<Bytes>,
        end_of_stream: bool,
        ctx: &mut Context,
    ) -> Result<()>;
}