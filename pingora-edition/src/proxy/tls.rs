//! TLS management for the proxy

use anyhow::{anyhow, Context, Result};
use rustls::{RootCertStore, ServerConfig};
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use rustls_pemfile;
use std::fs::File;
use std::io::BufReader;
use std::sync::Arc;
use x509_parser::prelude::*;
use crate::config::TlsConfig;

/// TLS manager handles certificate loading and validation
pub struct TlsManager {
    config: Option<TlsConfig>,
    server_config: Option<Arc<ServerConfig>>,
    ca_store: Option<RootCertStore>,
}

impl TlsManager {
    /// Create a new TLS manager
    pub fn new(config: Option<TlsConfig>) -> Self {
        Self { 
            config,
            server_config: None,
            ca_store: None,
        }
    }

    /// Check if TLS is enabled
    pub fn is_enabled(&self) -> bool {
        self.config.is_some()
    }

    /// Check if mTLS is enabled
    pub fn is_mtls_enabled(&self) -> bool {
        self.config
            .as_ref()
            .map(|c| c.ca_cert_path.is_some())
            .unwrap_or(false)
    }

    /// Load and validate certificates, creating the TLS server configuration
    pub fn load_certificates(&mut self) -> Result<()> {
        let tls_config = match &self.config {
            Some(config) => config,
            None => return Ok(()), // No TLS configuration, nothing to load
        };

        // Load server certificate chain
        let cert_chain = self.load_certificate_chain(&tls_config.cert_path)
            .context("Failed to load server certificate chain")?;

        // Load private key
        let private_key = self.load_private_key(&tls_config.key_path)
            .context("Failed to load private key")?;

        // Load CA certificates for mTLS if provided
        if let Some(ca_path) = &tls_config.ca_cert_path {
            let ca_store = self.load_ca_certificates(ca_path)
                .context("Failed to load CA certificates for mTLS")?;
            self.ca_store = Some(ca_store);
        }

        // Create rustls ServerConfig with hardcoded ALPN "h2"
        let config_builder = ServerConfig::builder();

        // Configure client certificate verification for mTLS
        let server_config = if let Some(ca_store) = &self.ca_store {
            let verifier = rustls::server::WebPkiClientVerifier::builder(Arc::new(ca_store.clone()))
                .build()
                .context("Failed to create client certificate verifier")?;
            config_builder.with_client_cert_verifier(verifier)
        } else {
            config_builder.with_no_client_auth()
        };

        let mut server_config = server_config
            .with_single_cert(cert_chain, private_key)
            .context("Failed to create TLS server configuration")?;

        // Hardcode ALPN to "h2" for HTTP/2 negotiation
        server_config.alpn_protocols = vec![b"h2".to_vec()];

        self.server_config = Some(Arc::new(server_config));

        Ok(())
    }

    /// Get the rustls ServerConfig for use with Pingora
    pub fn server_config(&self) -> Option<Arc<ServerConfig>> {
        self.server_config.clone()
    }

    /// Get the CA certificate store for mTLS validation
    pub fn ca_store(&self) -> Option<&RootCertStore> {
        self.ca_store.as_ref()
    }

    /// Load certificate chain from PEM file
    fn load_certificate_chain(&self, cert_path: &std::path::Path) -> Result<Vec<CertificateDer<'static>>> {
        let cert_file = File::open(cert_path)
            .with_context(|| format!("Cannot open certificate file: {:?}", cert_path))?;
        let mut cert_reader = BufReader::new(cert_file);

        let certs = rustls_pemfile::certs(&mut cert_reader)
            .collect::<Result<Vec<_>, _>>()
            .with_context(|| format!("Failed to parse certificate PEM file: {:?}", cert_path))?;

        if certs.is_empty() {
            return Err(anyhow!("No certificates found in file: {:?}", cert_path));
        }

        // Validate certificates using x509-parser
        for (i, cert_der) in certs.iter().enumerate() {
            let (_, cert) = X509Certificate::from_der(cert_der)
                .map_err(|e| anyhow!("Failed to parse certificate {}: {}", i, e))?;

            // Check if certificate is expired
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs() as i64;

            if cert.validity().not_after.timestamp() < now {
                return Err(anyhow!(
                    "Certificate {} has expired (not_after: {})",
                    i,
                    cert.validity().not_after
                ));
            }

            if cert.validity().not_before.timestamp() > now {
                return Err(anyhow!(
                    "Certificate {} is not yet valid (not_before: {})",
                    i,
                    cert.validity().not_before
                ));
            }
        }

        // Convert to rustls CertificateDer format
        let certificates = certs.into_iter().map(CertificateDer::from).collect();
        Ok(certificates)
    }

    /// Load private key from PEM file
    fn load_private_key(&self, key_path: &std::path::Path) -> Result<PrivateKeyDer<'static>> {
        let key_file = File::open(key_path)
            .with_context(|| format!("Cannot open private key file: {:?}", key_path))?;
        let mut key_reader = BufReader::new(key_file);

        // Try different key formats
        // First try PKCS8
        if let Ok(keys) = rustls_pemfile::pkcs8_private_keys(&mut key_reader).collect::<Result<Vec<_>, _>>() {
            if let Some(key) = keys.into_iter().next() {
                return Ok(PrivateKeyDer::from(key));
            }
        }

        // Try RSA private key
        let mut key_reader = BufReader::new(File::open(key_path)?);
        if let Ok(keys) = rustls_pemfile::rsa_private_keys(&mut key_reader).collect::<Result<Vec<_>, _>>() {
            if let Some(key) = keys.into_iter().next() {
                return Ok(PrivateKeyDer::from(key));
            }
        }

        // Try EC private key
        let mut key_reader = BufReader::new(File::open(key_path)?);
        if let Ok(keys) = rustls_pemfile::ec_private_keys(&mut key_reader).collect::<Result<Vec<_>, _>>() {
            if let Some(key) = keys.into_iter().next() {
                return Ok(PrivateKeyDer::from(key));
            }
        }

        Err(anyhow!("No valid private key found in file: {:?}", key_path))
    }

    /// Load CA certificates for mTLS client verification
    fn load_ca_certificates(&self, ca_path: &std::path::Path) -> Result<RootCertStore> {
        let ca_file = File::open(ca_path)
            .with_context(|| format!("Cannot open CA certificate file: {:?}", ca_path))?;
        let mut ca_reader = BufReader::new(ca_file);

        let ca_certs = rustls_pemfile::certs(&mut ca_reader)
            .collect::<Result<Vec<_>, _>>()
            .with_context(|| format!("Failed to parse CA certificate PEM file: {:?}", ca_path))?;

        if ca_certs.is_empty() {
            return Err(anyhow!("No CA certificates found in file: {:?}", ca_path));
        }

        let mut ca_store = RootCertStore::empty();
        for ca_cert_der in ca_certs {
            // Validate CA certificate using x509-parser
            let (_, ca_cert) = X509Certificate::from_der(&ca_cert_der)
                .map_err(|e| anyhow!("Failed to parse CA certificate: {}", e))?;

            // Check if CA certificate is expired
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs() as i64;

            if ca_cert.validity().not_after.timestamp() < now {
                return Err(anyhow!(
                    "CA certificate has expired (not_after: {})",
                    ca_cert.validity().not_after
                ));
            }

            if ca_cert.validity().not_before.timestamp() > now {
                return Err(anyhow!(
                    "CA certificate is not yet valid (not_before: {})",
                    ca_cert.validity().not_before
                ));
            }

            // Add to CA store
            ca_store.add(CertificateDer::from(ca_cert_der))
                .context("Failed to add CA certificate to store")?;
        }

        Ok(ca_store)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::TlsConfig;

    use tempfile::TempDir;
    use std::fs;

    /// Test TLS manager creation without TLS config
    #[test]
    fn test_tls_manager_no_config() {
        let manager = TlsManager::new(None);
        assert!(!manager.is_enabled());
        assert!(!manager.is_mtls_enabled());
    }

    /// Test TLS manager with basic TLS config (no actual certificates)
    #[test]
    fn test_tls_manager_basic_config() {
        let temp_dir = TempDir::new().unwrap();
        let cert_path = temp_dir.path().join("cert.pem");
        let key_path = temp_dir.path().join("key.pem");

        // Create dummy files (not valid certificates, just for path testing)
        fs::write(&cert_path, "dummy cert").unwrap();
        fs::write(&key_path, "dummy key").unwrap();

        let tls_config = TlsConfig {
            cert_path,
            key_path,
            ca_cert_path: None,
        };

        let manager = TlsManager::new(Some(tls_config));
        assert!(manager.is_enabled());
        assert!(!manager.is_mtls_enabled());
    }

    /// Test TLS manager with mTLS config
    #[test]
    fn test_tls_manager_mtls_config() {
        let temp_dir = TempDir::new().unwrap();
        let cert_path = temp_dir.path().join("cert.pem");
        let key_path = temp_dir.path().join("key.pem");
        let ca_path = temp_dir.path().join("ca.pem");

        // Create dummy files
        fs::write(&cert_path, "dummy cert").unwrap();
        fs::write(&key_path, "dummy key").unwrap();
        fs::write(&ca_path, "dummy ca").unwrap();

        let tls_config = TlsConfig {
            cert_path,
            key_path,
            ca_cert_path: Some(ca_path),
        };

        let manager = TlsManager::new(Some(tls_config));
        assert!(manager.is_enabled());
        assert!(manager.is_mtls_enabled());
    }
}