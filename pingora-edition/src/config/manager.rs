//! Configuration manager for loading and validating proxy configuration

use crate::error::{ProxyResult, ConfigError, TlsError, ErrorContext, ErrorContextExt};
use rustls_pemfile;
use std::fs;
use std::io::BufReader;
use x509_parser::prelude::*;
use super::ProxyConfig;

/// Configuration manager handles loading, validation, and hot-reloading of configuration
pub struct ConfigManager {
    config: ProxyConfig,
    config_path: String,
}

impl ConfigManager {
    /// Create a new configuration manager
    pub fn new(config_path: String) -> ProxyResult<Self> {
        let config = Self::load_and_validate_config(&config_path)?;
        Ok(Self {
            config,
            config_path,
        })
    }

    /// Get the current configuration
    pub fn config(&self) -> &ProxyConfig {
        &self.config
    }

    /// Reload configuration from file
    pub fn reload(&mut self) -> ProxyResult<()> {
        let new_config = Self::load_and_validate_config(&self.config_path)?;
        self.config = new_config;
        Ok(())
    }

    /// Load and validate configuration from file with comprehensive error handling
    fn load_and_validate_config(config_path: &str) -> ProxyResult<ProxyConfig> {
        let context = ErrorContext::new()
            .with_context("config_path".to_string(), config_path.to_string());

        // Check if config file exists
        if !std::path::Path::new(config_path).exists() {
            return Err(ConfigError::FileNotFound {
                path: config_path.to_string(),
            }.into());
        }

        // Read configuration file
        let content = fs::read_to_string(config_path)
            .map_err(|e| ConfigError::FileReadError {
                path: config_path.to_string(),
                source: e,
            })?;

        // Parse configuration based on file extension
        let config: ProxyConfig = if config_path.ends_with(".yaml") || config_path.ends_with(".yml") {
            serde_yaml::from_str(&content)
                .map_err(|e| ConfigError::InvalidFormat {
                    path: config_path.to_string(),
                    reason: format!("YAML parsing error: {}", e),
                })?
        } else if config_path.ends_with(".toml") {
            return Err(ConfigError::InvalidFormat {
                path: config_path.to_string(),
                reason: "TOML configuration support not yet implemented".to_string(),
            }.into());
        } else if config_path.ends_with(".json") {
            serde_json::from_str(&content)
                .map_err(|e| ConfigError::InvalidFormat {
                    path: config_path.to_string(),
                    reason: format!("JSON parsing error: {}", e),
                })?
        } else {
            // Default to YAML
            serde_yaml::from_str(&content)
                .map_err(|e| ConfigError::InvalidFormat {
                    path: config_path.to_string(),
                    reason: format!("YAML parsing error (default format): {}", e),
                })?
        };

        // Validate configuration structure
        config.validate()
            .map_err(|e| ConfigError::ValidationFailed {
                field: "configuration".to_string(),
                reason: e.to_string(),
            })?;

        // Perform certificate validation if TLS is enabled
        if let Some(tls_config) = &config.tls {
            Self::validate_certificates(tls_config)
                .with_context(context)?;
        }

        Ok(config)
    }

    /// Validate TLS certificates (readable, valid format, not expired)
    fn validate_certificates(tls_config: &super::TlsConfig) -> ProxyResult<()> {
        // Validate server certificate
        Self::validate_certificate_file(&tls_config.cert_path, "server certificate")?;

        // Validate private key
        Self::validate_private_key_file(&tls_config.key_path)?;

        // Validate CA certificate if provided (for mTLS)
        if let Some(ca_path) = &tls_config.ca_cert_path {
            Self::validate_certificate_file(ca_path, "CA certificate")?;
        }

        Ok(())
    }

    /// Validate a certificate file (readable, valid format, not expired)
    fn validate_certificate_file(cert_path: &std::path::Path, cert_type: &str) -> ProxyResult<()> {
        let path_str = cert_path.to_string_lossy().to_string();

        // Read certificate file
        let cert_file = fs::File::open(cert_path)
            .map_err(|e| TlsError::CertificateReadError {
                path: path_str.clone(),
                source: e,
            })?;
        let mut cert_reader = BufReader::new(cert_file);

        // Parse PEM certificates
        let certs = rustls_pemfile::certs(&mut cert_reader)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| TlsError::InvalidCertificate {
                path: path_str.clone(),
                reason: format!("PEM parsing error: {}", e),
            })?;

        if certs.is_empty() {
            return Err(TlsError::InvalidCertificate {
                path: path_str,
                reason: format!("No certificates found in {} file", cert_type),
            }.into());
        }

        // Validate each certificate using x509-parser
        for (i, cert_der) in certs.iter().enumerate() {
            let (_, cert) = X509Certificate::from_der(cert_der)
                .map_err(|e| TlsError::InvalidCertificate {
                    path: path_str.clone(),
                    reason: format!("Failed to parse certificate {}: {}", i, e),
                })?;

            // Check if certificate is expired
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs() as i64;

            if cert.validity().not_after.timestamp() < now {
                return Err(TlsError::CertificateExpired {
                    path: path_str.clone(),
                    not_after: cert.validity().not_after.to_string(),
                }.into());
            }

            if cert.validity().not_before.timestamp() > now {
                return Err(TlsError::CertificateNotYetValid {
                    path: path_str.clone(),
                    not_before: cert.validity().not_before.to_string(),
                }.into());
            }
        }

        Ok(())
    }

    /// Validate a private key file
    fn validate_private_key_file(key_path: &std::path::Path) -> ProxyResult<()> {
        let path_str = key_path.to_string_lossy().to_string();

        // Read private key file
        let _key_file = fs::File::open(key_path)
            .map_err(|e| TlsError::PrivateKeyReadError {
                path: path_str.clone(),
                source: e,
            })?;

        // Try to parse as different key types
        let mut found_key = false;

        // Reset reader for each attempt
        let key_content = fs::read(key_path)
            .map_err(|e| TlsError::PrivateKeyReadError {
                path: path_str.clone(),
                source: e,
            })?;

        // Try RSA private key
        let mut cursor = std::io::Cursor::new(&key_content);
        if let Ok(keys) = rustls_pemfile::rsa_private_keys(&mut cursor).collect::<Result<Vec<_>, _>>() {
            if !keys.is_empty() {
                found_key = true;
            }
        }

        // Try PKCS8 private key
        if !found_key {
            let mut cursor = std::io::Cursor::new(&key_content);
            if let Ok(keys) = rustls_pemfile::pkcs8_private_keys(&mut cursor).collect::<Result<Vec<_>, _>>() {
                if !keys.is_empty() {
                    found_key = true;
                }
            }
        }

        // Try EC private key
        if !found_key {
            let mut cursor = std::io::Cursor::new(&key_content);
            if let Ok(keys) = rustls_pemfile::ec_private_keys(&mut cursor).collect::<Result<Vec<_>, _>>() {
                if !keys.is_empty() {
                    found_key = true;
                }
            }
        }

        if !found_key {
            return Err(TlsError::InvalidPrivateKey {
                path: path_str,
                reason: "No valid private key found (tried RSA, PKCS8, and EC formats)".to_string(),
            }.into());
        }

        Ok(())
    }
}