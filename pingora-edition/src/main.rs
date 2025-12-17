use clap::Parser;
use tracing::{info, error, warn};
use tokio::signal;

mod config;
mod error;
mod health;
mod logging;
mod metrics;
mod proxy;
mod routing;

use config::ProxyConfig;
use proxy::ProxyServer;
use error::{ProxyResult, ConfigError};
use logging::{ProxyLogger, init_logging};
use health::HealthServer;
use metrics::metrics;

#[derive(Parser)]
#[command(name = "grpc-http-proxy")]
#[command(about = "High-performance gRPC HTTP proxy built with Pingora")]
#[command(long_about = "A high-performance HTTP proxy designed specifically to handle gRPC calls. \
Built on the Pingora framework with support for HTTP/2, TLS termination with optional mTLS, \
trailer preservation, and flexible routing capabilities. The proxy terminates TLS connections \
and communicates with upstream gRPC servers in clear text.")]
#[command(version = "0.1.0")]
#[command(author = "gRPC HTTP Proxy Team")]
struct Args {
    /// Path to configuration file (YAML, JSON, or TOML format)
    /// 
    /// The configuration file defines server settings, TLS configuration,
    /// routing rules, and upstream server definitions. See documentation
    /// for complete configuration schema.
    #[arg(short, long, default_value = "config.yaml", value_name = "FILE")]
    config: String,

    /// Enable verbose logging (debug level)
    /// 
    /// When enabled, the proxy will output detailed debug information
    /// including request routing, TLS handshake details, and upstream
    /// connection status.
    #[arg(short, long)]
    verbose: bool,

    /// Enable JSON structured logging
    /// 
    /// When enabled, logs will be output in JSON format suitable for
    /// structured log processing systems like ELK stack or similar.
    #[arg(long)]
    json_logs: bool,

    /// Health check server bind address
    /// 
    /// Address and port for the health check HTTP server that provides
    /// /health, /metrics, and /status endpoints for monitoring.
    #[arg(long, default_value = "127.0.0.1:8080")]
    health_addr: String,

    /// Validate configuration file and exit without starting server
    /// 
    /// Performs comprehensive validation including:
    /// - Configuration file syntax and structure
    /// - TLS certificate validation (if enabled)
    /// - Route pattern validation
    /// - Upstream server connectivity (basic checks)
    #[arg(long)]
    validate_config: bool,

    /// Print example configuration and exit
    /// 
    /// Outputs a complete example configuration file that can be used
    /// as a starting point for your deployment.
    #[arg(long)]
    example_config: bool,
}

#[tokio::main]
async fn main() -> ProxyResult<()> {
    let args = Args::parse();

    // Handle special commands that don't require configuration
    if args.example_config {
        print_example_configuration();
        return Ok(());
    }

    // Validate command-line arguments
    if let Err(e) = validate_cli_arguments(&args) {
        error!("Invalid command-line arguments: {}", e);
        std::process::exit(1);
    }

    // Initialize structured logging
    let log_level = if args.verbose { "debug" } else { "info" };
    if let Err(e) = init_logging(log_level, args.json_logs) {
        eprintln!("Failed to initialize logging: {}", e);
        std::process::exit(1);
    }
    
    ProxyLogger::log_server_startup(
        "0.0.0.0:0".parse().unwrap(), // Placeholder, will be updated after config load
        false, // Will be updated after config load
        None,
    );
    info!("Loading configuration from: {}", args.config);

    // Load and validate configuration
    let config = match ProxyConfig::load(&args.config) {
        Ok(config) => {
            ProxyLogger::log_configuration_loaded(
                &args.config,
                config.routes.len(),
                1 + config.routes.len(), // default upstream + route upstreams
            );
            config
        }
        Err(e) => {
            ProxyLogger::log_configuration_error(&args.config, &e.to_string());
            
            // Provide helpful error context
            if !std::path::Path::new(&args.config).exists() {
                error!("Configuration file does not exist. Create a config.yaml file or specify a different path with --config");
            }
            
            std::process::exit(1);
        }
    };

    // If only validating configuration, exit after successful validation
    if args.validate_config {
        info!("Configuration validation successful");
        info!("Server would bind to: {}", config.server.bind_address);
        if let Some(tls_config) = &config.tls {
            info!("TLS enabled with certificate: {:?}", tls_config.cert_path);
            if tls_config.ca_cert_path.is_some() {
                info!("mTLS enabled with CA certificate validation");
            }
        } else {
            info!("TLS disabled - plain HTTP/2 mode");
        }
        info!("Routes configured: {}", config.routes.len());
        info!("Default upstream: {}", config.default_upstream.address);
        return Ok(());
    }

    // Log proper server startup with actual configuration
    ProxyLogger::log_server_startup(
        config.server.bind_address,
        config.tls.is_some(),
        config.server.worker_threads,
    );

    // Log configuration summary
    info!("Server configuration:");
    info!("  Bind address: {}", config.server.bind_address);
    info!("  Worker threads: {:?}", config.server.worker_threads);
    info!("  Max connections: {:?}", config.server.max_connections);
    
    if let Some(tls_config) = &config.tls {
        info!("TLS configuration:");
        info!("  Certificate: {:?}", tls_config.cert_path);
        info!("  Private key: {:?}", tls_config.key_path);
        if let Some(ca_path) = &tls_config.ca_cert_path {
            info!("  CA certificate (mTLS): {:?}", ca_path);
        }
    } else {
        info!("TLS: Disabled (plain HTTP/2 mode)");
    }

    info!("Routing configuration:");
    info!("  Routes configured: {}", config.routes.len());
    for route in &config.routes {
        info!("    {} -> {}", route.path_pattern, route.upstream.address);
    }
    info!("  Default upstream: {}", config.default_upstream.address);

    // Create and initialize the proxy server
    let proxy_server = match ProxyServer::new(config) {
        Ok(server) => {
            info!("Proxy server initialized successfully");
            server
        }
        Err(e) => {
            error!("Failed to initialize proxy server: {}", e);
            error!("Check your TLS certificates and network configuration");
            std::process::exit(1);
        }
    };

    // Parse health check address
    let health_addr: std::net::SocketAddr = args.health_addr.parse()
        .map_err(|e: std::net::AddrParseError| ConfigError::InvalidAddress {
            address: args.health_addr.clone(),
            reason: e.to_string(),
        })?;

    // Start health check server
    let mut health_server = HealthServer::new(health_addr);
    if let Err(e) = health_server.start().await {
        error!("Failed to start health check server: {}", e);
        std::process::exit(1);
    }
    info!("Health check server started on {}", health_addr);

    // Start the proxy server
    info!("Starting proxy server...");
    
    // Run both servers with graceful shutdown
    tokio::select! {
        result = proxy_server.start() => {
            match result {
                Ok(()) => {
                    info!("Proxy server started successfully");
                    // In a real implementation, this would block until the server stops
                    // For now, we'll simulate running until shutdown
                    setup_shutdown_handler().await;
                    info!("Shutdown signal received");
                }
                Err(e) => {
                    error!("Proxy server failed to start: {}", e);
                    std::process::exit(1);
                }
            }
        }
        result = health_server.run() => {
            if let Err(e) = result {
                error!("Health check server error: {}", e);
            }
        }
        _ = setup_shutdown_handler() => {
            info!("Shutdown signal received, stopping servers...");
        }
    }

    // Graceful shutdown
    info!("Initiating graceful shutdown...");
    
    // Stop health server
    health_server.stop().await;
    
    // Stop proxy server
    if let Err(e) = proxy_server.stop().await {
        error!("Error during server shutdown: {}", e);
    } else {
        info!("Server shutdown completed successfully");
    }

    // Log final metrics
    let final_metrics = metrics().get_metrics_snapshot();
    ProxyLogger::log_performance_metrics(
        final_metrics.connection_metrics.active_connections,
        final_metrics.request_metrics.active_requests,
        if final_metrics.performance_metrics.current_memory_usage > 0 {
            Some(final_metrics.performance_metrics.current_memory_usage / (1024 * 1024))
        } else {
            None
        },
        if final_metrics.performance_metrics.current_cpu_usage > 0.0 {
            Some(final_metrics.performance_metrics.current_cpu_usage)
        } else {
            None
        },
    );

    ProxyLogger::log_server_shutdown(std::time::Duration::from_secs(0)); // Placeholder uptime
    info!("gRPC HTTP Proxy stopped");
    Ok(())
}

/// Validate command-line arguments
fn validate_cli_arguments(args: &Args) -> ProxyResult<()> {
    // Validate configuration file path
    if args.config.is_empty() {
        return Err(ConfigError::ValidationFailed {
            field: "config".to_string(),
            reason: "Configuration file path cannot be empty".to_string(),
        }.into());
    }

    // Check if configuration file has a supported extension
    let config_path = std::path::Path::new(&args.config);
    if let Some(extension) = config_path.extension() {
        let ext_str = extension.to_string_lossy().to_lowercase();
        if !matches!(ext_str.as_str(), "yaml" | "yml" | "json" | "toml") {
            warn!("Configuration file extension '{}' is not explicitly supported", ext_str);
            warn!("Supported extensions: .yaml, .yml, .json, .toml");
            warn!("Will attempt to parse as YAML");
        }
    } else {
        warn!("Configuration file has no extension, will attempt to parse as YAML");
    }

    // Validate that config file exists (unless we're just validating or showing examples)
    if !args.validate_config && !args.example_config {
        if !config_path.exists() {
            let program_name = std::env::args().next().unwrap_or_else(|| "grpc-http-proxy".to_string());
            return Err(ConfigError::FileNotFound {
                path: format!(
                    "{}\n\
                    \n\
                    To create an example configuration file, run:\n\
                      {} --example-config > {}\n\
                    \n\
                    Or specify a different configuration file with:\n\
                      {} --config /path/to/your/config.yaml",
                    args.config,
                    program_name,
                    args.config,
                    program_name
                ),
            }.into());
        }

        if !config_path.is_file() {
            return Err(ConfigError::ValidationFailed {
                field: "config".to_string(),
                reason: format!("Configuration path exists but is not a file: {}", args.config),
            }.into());
        }
    }

    Ok(())
}

/// Print an example configuration file
fn print_example_configuration() {
    println!(r#"# gRPC HTTP Proxy Configuration
# This is a complete example configuration showing all available options

# Server configuration
server:
  # Address and port to bind the proxy server
  bind_address: "0.0.0.0:8443"
  
  # Optional: Number of worker threads (defaults to number of CPU cores)
  worker_threads: 4
  
  # Optional: Maximum number of concurrent connections
  max_connections: 10000

# TLS configuration (optional - remove this section for plain HTTP/2)
tls:
  # Path to server certificate file (PEM format)
  cert_path: "/path/to/server.crt"
  
  # Path to private key file (PEM format)
  key_path: "/path/to/server.key"
  
  # Optional: Path to CA certificate for mTLS client validation
  # Remove this line to disable mTLS
  ca_cert_path: "/path/to/ca.crt"

# Routing configuration
routes:
  # Route gRPC calls to /api/v1/* to a specific upstream
  - path_pattern: "/api/v1/*"
    upstream:
      address: "127.0.0.1:9001"
      connection_pool_size: 10
      timeout: "30s"
    priority: 100
  
  # Route gRPC calls to /auth/* to an authentication service
  - path_pattern: "/auth/*"
    upstream:
      address: "127.0.0.1:9002"
      connection_pool_size: 5
      timeout: "10s"
    priority: 90
  
  # Route specific service calls
  - path_pattern: "/com.example.UserService/*"
    upstream:
      address: "127.0.0.1:9003"
      connection_pool_size: 20
      timeout: "60s"
    priority: 80

# Default upstream for requests that don't match any route
default_upstream:
  address: "127.0.0.1:9000"
  connection_pool_size: 50
  timeout: "30s"

# Example configurations for different deployment scenarios:

# 1. Plain HTTP/2 proxy (no TLS):
#    Remove the entire 'tls' section above
#    Change bind_address port to 8080 or another non-TLS port

# 2. TLS without mTLS:
#    Remove the 'ca_cert_path' line from the tls section

# 3. Single upstream (no routing):
#    Remove the entire 'routes' section
#    All traffic will go to default_upstream

# 4. Development setup:
#    Use localhost addresses and self-signed certificates
#    Enable verbose logging with --verbose flag
"#);
}

/// Set up signal handlers for graceful shutdown
async fn setup_shutdown_handler() {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("Failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("Failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {
            info!("Received Ctrl+C signal");
        },
        _ = terminate => {
            info!("Received terminate signal");
        },
    }
}