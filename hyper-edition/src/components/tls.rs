//! TLS Handler Component
//! 
//! Manages TLS termination and HTTP/2 connection establishment

use crate::error::ProxyError;
use crate::config::TlsConfig;
use rustls::ServerConfig;
use rustls_pemfile::{certs, private_key};
use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use std::sync::Arc;
use tokio::net::TcpStream;
use tokio_rustls::{TlsAcceptor, server::TlsStream};
use tracing;

/// TLS handler for managing TLS connections and ALPN negotiation
pub struct TlsHandler {
    acceptor: TlsAcceptor,
    config: TlsConfig,
}

impl TlsHandler {
    /// Create a new TLS handler with the given configuration
    pub fn new(config: TlsConfig) -> Result<Self, ProxyError> {
        let server_config = Self::build_server_config(&config)?;
        let acceptor = TlsAcceptor::from(Arc::new(server_config));
        
        Ok(TlsHandler {
            acceptor,
            config,
        })
    }

    /// Load certificates from PEM file
    fn load_certs(path: &Path) -> Result<Vec<rustls::pki_types::CertificateDer<'static>>, ProxyError> {
        let cert_file = File::open(path)
            .map_err(|e| ProxyError::ConfigError(format!("Failed to open certificate file {}: {}", path.display(), e)))?;
        let mut reader = BufReader::new(cert_file);
        
        let certs: Result<Vec<_>, _> = certs(&mut reader).collect();
        let certs = certs
            .map_err(|e| ProxyError::ConfigError(format!("Failed to parse certificates: {}", e)))?;
            
        if certs.is_empty() {
            return Err(ProxyError::ConfigError("No certificates found in file".to_string()));
        }
        
        Ok(certs)
    }

    /// Load private key from PEM file
    fn load_private_key(path: &Path) -> Result<rustls::pki_types::PrivateKeyDer<'static>, ProxyError> {
        let key_file = File::open(path)
            .map_err(|e| ProxyError::ConfigError(format!("Failed to open private key file {}: {}", path.display(), e)))?;
        let mut reader = BufReader::new(key_file);
        
        let key = private_key(&mut reader)
            .map_err(|e| ProxyError::ConfigError(format!("Failed to parse private key: {}", e)))?
            .ok_or_else(|| ProxyError::ConfigError("No private key found in file".to_string()))?;
        
        Ok(key)
    }

    /// Build rustls ServerConfig with proper cipher suites and ALPN settings
    fn build_server_config(config: &TlsConfig) -> Result<ServerConfig, ProxyError> {
        let certs = Self::load_certs(&config.cert_file)?;
        let key = Self::load_private_key(&config.key_file)?;
        
        let mut server_config = if let Some(ref root_ca_file) = config.root_ca_file {
            // Mutual TLS: require client authentication
            tracing::info!("Configuring mutual TLS with root CA: {}", root_ca_file.display());
            
            let root_ca_certs = Self::load_certs(root_ca_file)?;
            let mut root_cert_store = rustls::RootCertStore::empty();
            
            for cert in root_ca_certs {
                root_cert_store.add(cert)
                    .map_err(|e| ProxyError::ConfigError(format!("Failed to add root CA certificate: {}", e)))?;
            }
            
            let client_cert_verifier = rustls::server::WebPkiClientVerifier::builder(Arc::new(root_cert_store))
                .build()
                .map_err(|e| ProxyError::ConfigError(format!("Failed to build client certificate verifier: {}", e)))?;
            
            ServerConfig::builder()
                .with_client_cert_verifier(client_cert_verifier)
                .with_single_cert(certs, key)
                .map_err(|e| ProxyError::ConfigError(format!("Failed to build mutual TLS config: {}", e)))?
        } else {
            // Standard TLS: no client authentication
            tracing::info!("Configuring standard TLS (no client authentication)");
            
            ServerConfig::builder()
                .with_no_client_auth()
                .with_single_cert(certs, key)
                .map_err(|e| ProxyError::ConfigError(format!("Failed to build TLS config: {}", e)))?
        };
        
        // Configure ALPN to advertise HTTP/2 and HTTP/1.1 support
        server_config.alpn_protocols = vec![
            b"h2".to_vec(),        // HTTP/2
            b"http/1.1".to_vec(),  // HTTP/1.1 fallback
        ];
            
        Ok(server_config)
    }

    /// Get the negotiated ALPN protocol from a TLS connection
    pub fn get_negotiated_protocol(&self, stream: &TlsStream<TcpStream>) -> Option<String> {
        stream.get_ref().1.alpn_protocol()
            .map(|protocol| String::from_utf8_lossy(protocol).to_string())
    }

    /// Check if the negotiated protocol is HTTP/2
    pub fn is_http2_negotiated(&self, stream: &TlsStream<TcpStream>) -> bool {
        match self.get_negotiated_protocol(stream) {
            Some(protocol) => protocol == "h2",
            None => false, // Default to HTTP/1.1 if no ALPN negotiation
        }
    }

    /// Configure ALPN protocols (for testing or custom configurations)
    pub fn configure_alpn(&mut self, _protocols: Vec<String>) -> Result<(), ProxyError> {
        // This would require rebuilding the server config, which is complex
        // For now, we'll document that ALPN configuration happens at creation time
        // In a production implementation, this might involve rebuilding the acceptor
        tracing::warn!("ALPN reconfiguration not supported after initialization");
        Ok(())
    }

    /// Accept a TLS connection and perform handshake
    pub async fn accept_connection(&self, stream: TcpStream) -> Result<TlsStream<TcpStream>, ProxyError> {
        let peer_addr = stream.peer_addr().ok();
        tracing::debug!("Accepting TLS connection from {:?}", peer_addr);
        
        match self.acceptor.accept(stream).await {
            Ok(tls_stream) => {
                let negotiated_protocol = self.get_negotiated_protocol(&tls_stream);
                
                // Log security-relevant information
                tracing::info!("TLS handshake completed from {:?}, negotiated protocol: {:?}", 
                              peer_addr, negotiated_protocol);
                
                // Log additional security details
                let conn_info = self.get_connection_info(&tls_stream);
                if let Some(cipher) = &conn_info.cipher_suite {
                    tracing::debug!("TLS cipher suite for {:?}: {}", peer_addr, cipher);
                }
                if let Some(version) = &conn_info.protocol_version {
                    tracing::debug!("TLS protocol version for {:?}: {}", peer_addr, version);
                }
                
                // Security event: Successful TLS connection
                tracing::info!(
                    event = "tls_connection_established",
                    peer_addr = ?peer_addr,
                    protocol = ?negotiated_protocol,
                    cipher_suite = ?conn_info.cipher_suite,
                    tls_version = ?conn_info.protocol_version,
                    "TLS connection established"
                );
                
                Ok(tls_stream)
            }
            Err(e) => {
                // Security event: Failed TLS handshake
                tracing::warn!(
                    event = "tls_handshake_failed",
                    peer_addr = ?peer_addr,
                    error = %e,
                    "TLS handshake failed"
                );
                
                tracing::error!("TLS handshake failed from {:?}: {}", peer_addr, e);
                Err(ProxyError::Tls(format!("TLS handshake failed: {}", e)))
            }
        }
    }

    /// Handle TLS handshake errors gracefully
    pub fn handle_handshake_error(&self, error: &std::io::Error) -> ProxyError {
        tracing::error!("TLS handshake error: {}", error);
        ProxyError::Io(std::io::Error::new(error.kind(), format!("TLS handshake failed: {}", error)))
    }

    /// Get connection state information
    pub fn get_connection_info(&self, stream: &TlsStream<TcpStream>) -> ConnectionInfo {
        let (_, session) = stream.get_ref();
        
        ConnectionInfo {
            negotiated_protocol: session.alpn_protocol()
                .map(|p| String::from_utf8_lossy(p).to_string()),
            cipher_suite: session.negotiated_cipher_suite()
                .map(|cs| format!("{:?}", cs.suite())),
            protocol_version: Some(format!("{:?}", session.protocol_version())),
            peer_certificates: session.peer_certificates()
                .map(|certs| certs.len())
                .unwrap_or(0),
        }
    }
}

/// Information about an established TLS connection
#[derive(Debug, Clone)]
pub struct ConnectionInfo {
    pub negotiated_protocol: Option<String>,
    pub cipher_suite: Option<String>,
    pub protocol_version: Option<String>,
    pub peer_certificates: usize,
}