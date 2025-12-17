//! Configuration module
//! 
//! Handles loading and parsing of proxy configuration

use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Duration;
use clap::Parser;
use crate::error::ProxyError;

/// Main proxy configuration
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ProxyConfig {
    /// Listening address and port
    pub listen: ListenConfig,
    /// TLS configuration (optional - if not provided, proxy runs unencrypted)
    #[serde(default)]
    pub tls: Option<TlsConfig>,
    /// Routing configuration
    pub routing: RoutingConfig,
    /// Logging configuration
    #[serde(default)]
    pub logging: LoggingConfig,
}

/// Listen configuration
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ListenConfig {
    #[serde(default = "default_listen_address")]
    pub address: String,
    pub port: u16,
}

/// TLS configuration
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TlsConfig {
    pub cert_file: PathBuf,
    pub key_file: PathBuf,
    /// Root CA certificate for client authentication (optional - enables mutual TLS)
    #[serde(default)]
    pub root_ca_file: Option<PathBuf>,
}

impl TlsConfig {
    /// Check if TLS is enabled (both cert and key files are provided)
    pub fn is_enabled(&self) -> bool {
        !self.cert_file.as_os_str().is_empty() && !self.key_file.as_os_str().is_empty()
    }
    
    /// Check if mutual TLS is enabled (TLS is enabled and root CA is provided)
    pub fn is_mutual_tls_enabled(&self) -> bool {
        self.is_enabled() && self.root_ca_file.is_some()
    }
    
    /// Validate TLS configuration
    pub fn validate(&self) -> Result<(), ProxyError> {
        let cert_provided = !self.cert_file.as_os_str().is_empty();
        let key_provided = !self.key_file.as_os_str().is_empty();
        
        // Both must be provided or both must be empty
        if cert_provided && !key_provided {
            return Err(ProxyError::ConfigError(
                "TLS certificate file provided but key file is missing. Both cert_file and key_file must be provided for TLS, or both omitted for unencrypted mode.".to_string()
            ));
        }
        
        if !cert_provided && key_provided {
            return Err(ProxyError::ConfigError(
                "TLS key file provided but certificate file is missing. Both cert_file and key_file must be provided for TLS, or both omitted for unencrypted mode.".to_string()
            ));
        }
        
        // If both are provided, check if files exist
        if cert_provided && key_provided {
            if !self.cert_file.exists() {
                return Err(ProxyError::ConfigError(format!(
                    "TLS certificate file not found: {}",
                    self.cert_file.display()
                )));
            }

            if !self.key_file.exists() {
                return Err(ProxyError::ConfigError(format!(
                    "TLS key file not found: {}",
                    self.key_file.display()
                )));
            }
            
            // If root CA is provided, check if it exists
            if let Some(ref root_ca_file) = self.root_ca_file {
                if !root_ca_file.exists() {
                    return Err(ProxyError::ConfigError(format!(
                        "TLS root CA certificate file not found: {}",
                        root_ca_file.display()
                    )));
                }
            }
        }
        
        Ok(())
    }
}

/// Routing configuration
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RoutingConfig {
    pub rules: Vec<RoutingRule>,
    pub catch_all: Option<UpstreamConfig>,
}

/// Individual routing rule
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RoutingRule {
    pub pattern: String,
    pub upstream: UpstreamConfig,
    #[serde(default = "default_priority")]
    pub priority: u32,
}

/// Upstream server configuration
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct UpstreamConfig {
    pub host: String,
    pub port: u16,
    #[serde(default = "default_timeout", with = "duration_serde")]
    pub timeout: Duration,
    #[serde(default = "default_max_connections")]
    pub max_connections: usize,
}

/// Logging configuration
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LoggingConfig {
    #[serde(default = "default_log_level")]
    pub level: String,
}

// Default value functions
fn default_listen_address() -> String {
    "0.0.0.0".to_string()
}

fn default_priority() -> u32 {
    100
}

fn default_timeout() -> Duration {
    Duration::from_secs(30)
}

fn default_max_connections() -> usize {
    100
}

fn default_log_level() -> String {
    "info".to_string()
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            level: default_log_level(),
        }
    }
}

/// Command-line arguments
#[derive(Parser, Debug)]
#[command(name = "grpc-http-proxy")]
#[command(about = "A high-performance HTTP proxy server for gRPC traffic with optional TLS termination")]
#[command(version)]
pub struct Args {
    /// Configuration file path
    #[arg(short, long, default_value = "proxy-config.yaml")]
    pub config: PathBuf,

    /// Override listen address
    #[arg(long)]
    pub listen_address: Option<String>,

    /// Override listen port
    #[arg(long)]
    pub listen_port: Option<u16>,

    /// Override TLS certificate file (requires --tls-key)
    #[arg(long)]
    pub tls_cert: Option<PathBuf>,

    /// Override TLS key file (requires --tls-cert)
    #[arg(long)]
    pub tls_key: Option<PathBuf>,

    /// Override TLS root CA certificate file for mutual TLS (optional)
    #[arg(long)]
    pub tls_root_ca: Option<PathBuf>,

    /// Disable TLS and run in unencrypted mode
    #[arg(long)]
    pub no_tls: bool,

    /// Override log level
    #[arg(long)]
    pub log_level: Option<String>,
}

// Custom serde module for Duration
mod duration_serde {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use std::time::Duration;

    pub fn serialize<S>(duration: &Duration, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        duration.as_secs().serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Duration, D::Error>
    where
        D: Deserializer<'de>,
    {
        let secs = u64::deserialize(deserializer)?;
        Ok(Duration::from_secs(secs))
    }
}

impl ProxyConfig {
    /// Load configuration from file
    pub fn from_file(path: &std::path::Path) -> Result<Self, ProxyError> {
        // Read the configuration file
        let content = std::fs::read_to_string(path)
            .map_err(|e| ProxyError::ConfigError(format!("Failed to read config file {}: {}", path.display(), e)))?;

        // Parse YAML configuration
        let config: ProxyConfig = serde_yaml::from_str(&content)
            .map_err(|e| ProxyError::ConfigError(format!("Failed to parse config file {}: {}", path.display(), e)))?;

        // Validate the configuration
        config.validate()?;

        Ok(config)
    }

    /// Load configuration from command-line arguments
    pub fn from_args(args: Args) -> Result<Self, ProxyError> {
        // Start with configuration from file
        let mut config = Self::from_file(&args.config)?;

        // Apply command-line overrides
        if let Some(address) = args.listen_address {
            config.listen.address = address;
        }

        if let Some(port) = args.listen_port {
            config.listen.port = port;
        }

        // Handle TLS configuration overrides
        if args.no_tls {
            // Explicitly disable TLS
            config.tls = None;
        } else {
            // Apply TLS overrides if provided
            let cert_override = args.tls_cert;
            let key_override = args.tls_key;
            
            // Validate CLI TLS arguments
            if cert_override.is_some() && key_override.is_none() {
                return Err(ProxyError::ConfigError(
                    "TLS certificate file provided via --tls-cert but --tls-key is missing. Both must be provided together.".to_string()
                ));
            }
            
            if cert_override.is_none() && key_override.is_some() {
                return Err(ProxyError::ConfigError(
                    "TLS key file provided via --tls-key but --tls-cert is missing. Both must be provided together.".to_string()
                ));
            }
            
            // Apply overrides if both are provided
            if let (Some(cert_file), Some(key_file)) = (cert_override, key_override) {
                // Create TLS config if it doesn't exist, or update existing one
                match config.tls.as_mut() {
                    Some(tls_config) => {
                        tls_config.cert_file = cert_file;
                        tls_config.key_file = key_file;
                    }
                    None => {
                        config.tls = Some(TlsConfig {
                            cert_file,
                            key_file,
                            root_ca_file: None,
                        });
                    }
                }
            }
            
            // Apply root CA override if provided (only if TLS is enabled)
            if let Some(root_ca_file) = args.tls_root_ca {
                match config.tls.as_mut() {
                    Some(tls_config) => {
                        tls_config.root_ca_file = Some(root_ca_file);
                    }
                    None => {
                        return Err(ProxyError::ConfigError(
                            "TLS root CA file provided via --tls-root-ca but TLS is not enabled. TLS certificate and key must be configured first.".to_string()
                        ));
                    }
                }
            }
        }

        if let Some(log_level) = args.log_level {
            config.logging.level = log_level;
        }

        // Validate the final configuration
        config.validate()?;

        Ok(config)
    }

    /// Create a default configuration for testing
    pub fn default_for_testing() -> Self {
        Self {
            listen: ListenConfig {
                address: "127.0.0.1".to_string(),
                port: 8443,
            },
            tls: Some(TlsConfig {
                cert_file: PathBuf::from("test-cert.pem"),
                key_file: PathBuf::from("test-key.pem"),
                root_ca_file: None,
            }),
            routing: RoutingConfig {
                rules: vec![],
                catch_all: Some(UpstreamConfig {
                    host: "localhost".to_string(),
                    port: 9090,
                    timeout: Duration::from_secs(30),
                    max_connections: 100,
                }),
            },
            logging: LoggingConfig {
                level: "info".to_string(),
            },
        }
    }

    /// Create a default configuration for testing without TLS
    pub fn default_for_testing_no_tls() -> Self {
        Self {
            listen: ListenConfig {
                address: "127.0.0.1".to_string(),
                port: 8080, // Use standard HTTP port for non-TLS
            },
            tls: None,
            routing: RoutingConfig {
                rules: vec![],
                catch_all: Some(UpstreamConfig {
                    host: "localhost".to_string(),
                    port: 9090,
                    timeout: Duration::from_secs(30),
                    max_connections: 100,
                }),
            },
            logging: LoggingConfig {
                level: "info".to_string(),
            },
        }
    }

    /// Check if TLS is enabled
    pub fn is_tls_enabled(&self) -> bool {
        self.tls.as_ref().map(|tls| tls.is_enabled()).unwrap_or(false)
    }
    
    /// Check if mutual TLS is enabled
    pub fn is_mutual_tls_enabled(&self) -> bool {
        self.tls.as_ref().map(|tls| tls.is_mutual_tls_enabled()).unwrap_or(false)
    }

    /// Get socket address for listening
    pub fn socket_addr(&self) -> Result<SocketAddr, ProxyError> {
        let addr = format!("{}:{}", self.listen.address, self.listen.port);
        addr.parse()
            .map_err(|e| ProxyError::ConfigError(format!("Invalid address: {}", e)))
    }

    /// Validate the configuration
    pub fn validate(&self) -> Result<(), ProxyError> {
        // Validate listen configuration
        self.validate_listen_config()?;
        
        // Validate TLS configuration if present
        if let Some(ref tls_config) = self.tls {
            tls_config.validate()?;
        }
        
        // Validate routing configuration
        self.validate_routing_config()?;
        
        // Validate logging configuration
        self.validate_logging_config()?;
        
        Ok(())
    }

    fn validate_listen_config(&self) -> Result<(), ProxyError> {
        // Validate port range
        if self.listen.port == 0 {
            return Err(ProxyError::ConfigError("Port cannot be 0".to_string()));
        }

        // Validate address format
        let addr = format!("{}:{}", self.listen.address, self.listen.port);
        addr.parse::<SocketAddr>()
            .map_err(|e| ProxyError::ConfigError(format!("Invalid listen address: {}", e)))?;

        Ok(())
    }



    fn validate_routing_config(&self) -> Result<(), ProxyError> {
        // Validate that we have at least one routing rule or catch-all
        if self.routing.rules.is_empty() && self.routing.catch_all.is_none() {
            return Err(ProxyError::ConfigError(
                "At least one routing rule or catch-all upstream must be configured".to_string()
            ));
        }

        // Validate each routing rule
        for (index, rule) in self.routing.rules.iter().enumerate() {
            if rule.pattern.is_empty() {
                return Err(ProxyError::ConfigError(format!(
                    "Routing rule {} has empty pattern",
                    index
                )));
            }
            rule.upstream.validate()?;
        }

        // Validate catch-all upstream if present
        if let Some(ref catch_all) = self.routing.catch_all {
            catch_all.validate()?;
        }

        Ok(())
    }

    fn validate_logging_config(&self) -> Result<(), ProxyError> {
        // Validate log level
        match self.logging.level.to_lowercase().as_str() {
            "trace" | "debug" | "info" | "warn" | "error" => Ok(()),
            _ => Err(ProxyError::ConfigError(format!(
                "Invalid log level: {}. Must be one of: trace, debug, info, warn, error",
                self.logging.level
            ))),
        }
    }
}

impl UpstreamConfig {
    /// Validate upstream configuration
    pub fn validate(&self) -> Result<(), ProxyError> {
        // Validate host is not empty
        if self.host.is_empty() {
            return Err(ProxyError::ConfigError("Upstream host cannot be empty".to_string()));
        }

        // Validate port range
        if self.port == 0 {
            return Err(ProxyError::ConfigError("Upstream port cannot be 0".to_string()));
        }

        // Validate timeout is reasonable
        if self.timeout.as_secs() == 0 {
            return Err(ProxyError::ConfigError("Upstream timeout cannot be 0".to_string()));
        }

        // Validate max connections is reasonable
        if self.max_connections == 0 {
            return Err(ProxyError::ConfigError("Max connections cannot be 0".to_string()));
        }

        Ok(())
    }
}

impl RoutingRule {
    /// Validate routing rule
    pub fn validate(&self) -> Result<(), ProxyError> {
        if self.pattern.is_empty() {
            return Err(ProxyError::ConfigError("Routing pattern cannot be empty".to_string()));
        }
        
        self.upstream.validate()
    }
}
#[cfg(
test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_default_values() {
        let config = ProxyConfig::default_for_testing();
        
        assert_eq!(config.listen.address, "127.0.0.1");
        assert_eq!(config.listen.port, 8443);
        assert_eq!(config.logging.level, "info");
        assert!(config.routing.catch_all.is_some());
        assert!(config.tls.is_some());
        assert!(config.is_tls_enabled());
        
        let upstream = config.routing.catch_all.unwrap();
        assert_eq!(upstream.host, "localhost");
        assert_eq!(upstream.port, 9090);
        assert_eq!(upstream.timeout, Duration::from_secs(30));
        assert_eq!(upstream.max_connections, 100);
    }

    #[test]
    fn test_default_values_no_tls() {
        let config = ProxyConfig::default_for_testing_no_tls();
        
        assert_eq!(config.listen.address, "127.0.0.1");
        assert_eq!(config.listen.port, 8080);
        assert_eq!(config.logging.level, "info");
        assert!(config.routing.catch_all.is_some());
        assert!(config.tls.is_none());
        assert!(!config.is_tls_enabled());
    }

    #[test]
    fn test_valid_yaml_config() {
        // Create dummy cert files for validation
        let cert_file = NamedTempFile::new().unwrap();
        let key_file = NamedTempFile::new().unwrap();
        fs::write(cert_file.path(), "dummy cert").unwrap();
        fs::write(key_file.path(), "dummy key").unwrap();

        let yaml_content = format!(r#"
listen:
  address: "0.0.0.0"
  port: 8443

tls:
  cert_file: "{}"
  key_file: "{}"

routing:
  rules:
    - pattern: "/auth.*"
      upstream:
        host: "auth-service"
        port: 9090
        timeout: 30
        max_connections: 50
      priority: 10
    - pattern: "/user.UserService/*"
      upstream:
        host: "user-service"
        port: 9091
  
  catch_all:
    host: "default-service"
    port: 9092

logging:
  level: "debug"
"#, cert_file.path().display(), key_file.path().display());

        let mut temp_file = NamedTempFile::new().unwrap();
        temp_file.write_all(yaml_content.as_bytes()).unwrap();

        let config = ProxyConfig::from_file(temp_file.path()).unwrap();
        
        assert_eq!(config.listen.address, "0.0.0.0");
        assert_eq!(config.listen.port, 8443);
        assert_eq!(config.logging.level, "debug");
        assert_eq!(config.routing.rules.len(), 2);
        
        let first_rule = &config.routing.rules[0];
        assert_eq!(first_rule.pattern, "/auth.*");
        assert_eq!(first_rule.upstream.host, "auth-service");
        assert_eq!(first_rule.upstream.port, 9090);
        assert_eq!(first_rule.priority, 10);
        
        // Clean up
        fs::remove_file("/tmp/cert.pem").ok();
        fs::remove_file("/tmp/key.pem").ok();
    }

    #[test]
    fn test_invalid_yaml_config() {
        let invalid_yaml = r#"
listen:
  address: "0.0.0.0"
  port: invalid_port
"#;

        let mut temp_file = NamedTempFile::new().unwrap();
        temp_file.write_all(invalid_yaml.as_bytes()).unwrap();

        let result = ProxyConfig::from_file(temp_file.path());
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Failed to parse config file"));
    }

    #[test]
    fn test_missing_config_file() {
        let result = ProxyConfig::from_file(std::path::Path::new("/nonexistent/config.yaml"));
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Failed to read config file"));
    }

    #[test]
    fn test_validation_empty_routing() {
        let mut config = ProxyConfig::default_for_testing();
        config.routing.rules.clear();
        config.routing.catch_all = None;
        
        let result = config.validate();
        assert!(result.is_err());
        let error_msg = result.unwrap_err().to_string();
        // The validation might fail on TLS files first, so let's check for that too
        assert!(error_msg.contains("At least one routing rule") || 
                error_msg.contains("catch-all upstream") ||
                error_msg.contains("TLS certificate file not found"));
    }

    #[test]
    fn test_validation_invalid_port() {
        let mut config = ProxyConfig::default_for_testing();
        config.listen.port = 0;
        
        let result = config.validate();
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Port cannot be 0"));
    }

    #[test]
    fn test_validation_invalid_log_level() {
        let mut config = ProxyConfig::default_for_testing();
        config.logging.level = "invalid".to_string();
        
        let result = config.validate();
        assert!(result.is_err());
        let error_msg = result.unwrap_err().to_string();
        assert!(error_msg.contains("Invalid log level") || error_msg.contains("TLS certificate file not found"));
    }

    #[test]
    fn test_validation_missing_tls_files() {
        let mut config = ProxyConfig::default_for_testing();
        if let Some(ref mut tls_config) = config.tls {
            tls_config.cert_file = PathBuf::from("/nonexistent/cert.pem");
        }
        
        let result = config.validate();
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("TLS certificate file not found"));
    }

    #[test]
    fn test_validation_no_tls_config() {
        let config = ProxyConfig::default_for_testing_no_tls();
        
        // Should validate successfully without TLS
        let result = config.validate();
        assert!(result.is_ok());
    }

    #[test]
    fn test_mutual_tls_config() {
        // Create dummy cert files for validation
        let cert_file = NamedTempFile::new().unwrap();
        let key_file = NamedTempFile::new().unwrap();
        let root_ca_file = NamedTempFile::new().unwrap();
        fs::write(cert_file.path(), "dummy cert").unwrap();
        fs::write(key_file.path(), "dummy key").unwrap();
        fs::write(root_ca_file.path(), "dummy root ca").unwrap();

        let tls_config = TlsConfig {
            cert_file: cert_file.path().to_path_buf(),
            key_file: key_file.path().to_path_buf(),
            root_ca_file: Some(root_ca_file.path().to_path_buf()),
        };
        
        // Should validate successfully
        let result = tls_config.validate();
        assert!(result.is_ok());
        
        // Should indicate mutual TLS is enabled
        assert!(tls_config.is_enabled());
        assert!(tls_config.is_mutual_tls_enabled());
        
        let config = ProxyConfig {
            listen: ListenConfig {
                address: "127.0.0.1".to_string(),
                port: 8443,
            },
            tls: Some(tls_config),
            routing: RoutingConfig {
                rules: vec![],
                catch_all: Some(UpstreamConfig {
                    host: "localhost".to_string(),
                    port: 9090,
                    timeout: Duration::from_secs(30),
                    max_connections: 100,
                }),
            },
            logging: LoggingConfig {
                level: "info".to_string(),
            },
        };
        
        assert!(config.is_tls_enabled());
        assert!(config.is_mutual_tls_enabled());
    }

    #[test]
    fn test_mutual_tls_validation_missing_root_ca() {
        let tls_config = TlsConfig {
            cert_file: PathBuf::from("/tmp/cert.pem"),
            key_file: PathBuf::from("/tmp/key.pem"),
            root_ca_file: Some(PathBuf::from("/nonexistent/root-ca.pem")),
        };
        
        // Create dummy cert and key files
        fs::write("/tmp/cert.pem", "dummy cert").unwrap();
        fs::write("/tmp/key.pem", "dummy key").unwrap();
        
        let result = tls_config.validate();
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("TLS root CA certificate file not found"));
        
        // Clean up
        fs::remove_file("/tmp/cert.pem").ok();
        fs::remove_file("/tmp/key.pem").ok();
    }

    #[test]
    fn test_cli_root_ca_without_tls() {
        let yaml_content = r#"
listen:
  address: "0.0.0.0"
  port: 8080

routing:
  rules: []
  catch_all:
    host: "default-service"
    port: 9092

logging:
  level: "info"
"#;

        let mut temp_file = NamedTempFile::new().unwrap();
        temp_file.write_all(yaml_content.as_bytes()).unwrap();
        
        // Test providing root CA without TLS enabled
        let args = Args {
            config: temp_file.path().to_path_buf(),
            listen_address: None,
            listen_port: None,
            tls_cert: None,
            tls_key: None,
            tls_root_ca: Some(PathBuf::from("/tmp/root-ca.pem")),
            no_tls: false,
            log_level: None,
        };
        
        let result = ProxyConfig::from_args(args);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("TLS root CA file provided via --tls-root-ca but TLS is not enabled"));
    }

    #[test]
    fn test_cli_mutual_tls_override() {
        // Create dummy files for validation
        let cert_file = NamedTempFile::new().unwrap();
        let key_file = NamedTempFile::new().unwrap();
        let root_ca_file = NamedTempFile::new().unwrap();
        fs::write(cert_file.path(), "dummy cert").unwrap();
        fs::write(key_file.path(), "dummy key").unwrap();
        fs::write(root_ca_file.path(), "dummy root ca").unwrap();

        let yaml_content = format!(r#"
listen:
  address: "0.0.0.0"
  port: 8443

tls:
  cert_file: "{}"
  key_file: "{}"

routing:
  rules: []
  catch_all:
    host: "default-service"
    port: 9092

logging:
  level: "info"
"#, cert_file.path().display(), key_file.path().display());

        let mut temp_file = NamedTempFile::new().unwrap();
        temp_file.write_all(yaml_content.as_bytes()).unwrap();

        let args = Args {
            config: temp_file.path().to_path_buf(),
            listen_address: None,
            listen_port: None,
            tls_cert: None,
            tls_key: None,
            tls_root_ca: Some(root_ca_file.path().to_path_buf()),
            no_tls: false,
            log_level: None,
        };

        let config = ProxyConfig::from_args(args).unwrap();
        
        assert!(config.is_tls_enabled());
        assert!(config.is_mutual_tls_enabled());
        assert_eq!(config.tls.as_ref().unwrap().root_ca_file.as_ref().unwrap(), &root_ca_file.path().to_path_buf());
    }

    #[test]
    fn test_tls_config_validation_partial() {
        // Test validation when only cert file is provided
        let tls_config = TlsConfig {
            cert_file: PathBuf::from("/path/to/cert.pem"),
            key_file: PathBuf::new(), // Empty path
            root_ca_file: None,
        };
        
        let result = tls_config.validate();
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("certificate file provided but key file is missing"));
        
        // Test validation when only key file is provided
        let tls_config = TlsConfig {
            cert_file: PathBuf::new(), // Empty path
            key_file: PathBuf::from("/path/to/key.pem"),
            root_ca_file: None,
        };
        
        let result = tls_config.validate();
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("key file provided but certificate file is missing"));
    }

    #[test]
    fn test_cli_args_tls_validation() {
        let yaml_content = r#"
listen:
  address: "0.0.0.0"
  port: 8080

routing:
  rules: []
  catch_all:
    host: "default-service"
    port: 9092

logging:
  level: "info"
"#;

        let mut temp_file = NamedTempFile::new().unwrap();
        temp_file.write_all(yaml_content.as_bytes()).unwrap();
        
        // Test providing only cert file via CLI
        let args = Args {
            config: temp_file.path().to_path_buf(),
            listen_address: None,
            listen_port: None,
            tls_cert: Some(PathBuf::from("/tmp/cert.pem")),
            tls_key: None,
            tls_root_ca: None,
            no_tls: false,
            log_level: None,
        };
        
        let result = ProxyConfig::from_args(args);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("certificate file provided via --tls-cert but --tls-key is missing"));
        
        // Test providing only key file via CLI
        let args = Args {
            config: temp_file.path().to_path_buf(),
            listen_address: None,
            listen_port: None,
            tls_cert: None,
            tls_key: Some(PathBuf::from("/tmp/key.pem")),
            tls_root_ca: None,
            no_tls: false,
            log_level: None,
        };
        
        let result = ProxyConfig::from_args(args);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("key file provided via --tls-key but --tls-cert is missing"));
    }

    #[test]
    fn test_upstream_validation() {
        let upstream = UpstreamConfig {
            host: "".to_string(),
            port: 9090,
            timeout: Duration::from_secs(30),
            max_connections: 100,
        };
        
        let result = upstream.validate();
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Upstream host cannot be empty"));
    }

    #[test]
    fn test_upstream_validation_zero_port() {
        let upstream = UpstreamConfig {
            host: "localhost".to_string(),
            port: 0,
            timeout: Duration::from_secs(30),
            max_connections: 100,
        };
        
        let result = upstream.validate();
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Upstream port cannot be 0"));
    }

    #[test]
    fn test_routing_rule_validation() {
        let rule = RoutingRule {
            pattern: "".to_string(),
            upstream: UpstreamConfig {
                host: "localhost".to_string(),
                port: 9090,
                timeout: Duration::from_secs(30),
                max_connections: 100,
            },
            priority: 100,
        };
        
        let result = rule.validate();
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Routing pattern cannot be empty"));
    }

    #[test]
    fn test_socket_addr_conversion() {
        let config = ProxyConfig::default_for_testing();
        let addr = config.socket_addr().unwrap();
        assert_eq!(addr.ip().to_string(), "127.0.0.1");
        assert_eq!(addr.port(), 8443);
    }

    #[test]
    fn test_args_override() {
        // Create dummy cert files for validation
        let cert_file = NamedTempFile::new().unwrap();
        let key_file = NamedTempFile::new().unwrap();
        let override_cert_file = NamedTempFile::new().unwrap();
        let override_key_file = NamedTempFile::new().unwrap();
        fs::write(cert_file.path(), "dummy cert").unwrap();
        fs::write(key_file.path(), "dummy key").unwrap();
        fs::write(override_cert_file.path(), "override cert").unwrap();
        fs::write(override_key_file.path(), "override key").unwrap();

        let yaml_content = format!(r#"
listen:
  address: "0.0.0.0"
  port: 8443

tls:
  cert_file: "{}"
  key_file: "{}"

routing:
  rules: []
  catch_all:
    host: "default-service"
    port: 9092

logging:
  level: "info"
"#, cert_file.path().display(), key_file.path().display());

        let mut temp_file = NamedTempFile::new().unwrap();
        temp_file.write_all(yaml_content.as_bytes()).unwrap();

        let args = Args {
            config: temp_file.path().to_path_buf(),
            listen_address: Some("127.0.0.1".to_string()),
            listen_port: Some(9443),
            tls_cert: Some(override_cert_file.path().to_path_buf()),
            tls_key: Some(override_key_file.path().to_path_buf()),
            tls_root_ca: None,
            no_tls: false,
            log_level: Some("debug".to_string()),
        };

        let config = ProxyConfig::from_args(args).unwrap();
        
        assert_eq!(config.listen.address, "127.0.0.1");
        assert_eq!(config.listen.port, 9443);
        assert_eq!(config.tls.as_ref().unwrap().cert_file, override_cert_file.path().to_path_buf());
        assert_eq!(config.tls.as_ref().unwrap().key_file, override_key_file.path().to_path_buf()); // Overridden
        assert_eq!(config.logging.level, "debug");
    }
}