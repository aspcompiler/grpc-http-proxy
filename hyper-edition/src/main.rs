use grpc_http_proxy::{ProxyServer, ProxyConfig};
use grpc_http_proxy::config::Args;
use clap::Parser;
use std::process;
use tracing::{info, error};

#[tokio::main]
async fn main() {
    // Parse command-line arguments first
    let args = Args::parse();
    
    // Load configuration from file and apply CLI overrides
    let config = match ProxyConfig::from_args(args) {
        Ok(config) => config,
        Err(e) => {
            eprintln!("Configuration error: {}", e);
            eprintln!("Use --help for usage information");
            process::exit(1);
        }
    };
    
    // Initialize logging with the configured level
    if let Err(e) = init_logging(&config.logging.level) {
        eprintln!("Failed to initialize logging: {}", e);
        process::exit(1);
    }
    
    info!("Starting gRPC HTTP Proxy");
    info!("Configuration loaded successfully");
    info!("Listening on {}:{}", config.listen.address, config.listen.port);
    
    // Log TLS status
    if let Some(ref tls_config) = config.tls {
        info!("TLS certificate: {}", tls_config.cert_file.display());
        info!("TLS key: {}", tls_config.key_file.display());
    } else {
        info!("TLS disabled - running in unencrypted mode");
    }
    
    info!("Routing rules configured: {}", config.routing.rules.len());
    if config.routing.catch_all.is_some() {
        info!("Catch-all route configured");
    }
    
    // Validate configuration before starting server
    info!("Validating configuration...");
    if let Err(e) = config.validate() {
        error!("Configuration validation failed: {}", e);
        process::exit(1);
    }
    info!("Configuration validation passed");
    
    // Display startup information
    display_startup_info(&config);
    
    // Create the proxy server
    info!("Initializing proxy server components...");
    let server = match ProxyServer::new(config) {
        Ok(server) => {
            info!("Proxy server components initialized successfully");
            server
        },
        Err(e) => {
            error!("Failed to create proxy server: {}", e);
            process::exit(1);
        }
    };
    
    // Start the server and handle any runtime errors
    info!("Starting proxy server...");
    if let Err(e) = server.start().await {
        error!("Server error: {}", e);
        process::exit(1);
    }
    
    info!("Server shutdown complete");
}

/// Initialize logging with the specified level
fn init_logging(level: &str) -> Result<(), Box<dyn std::error::Error>> {
    use tracing_subscriber::{EnvFilter, fmt, layer::SubscriberExt, util::SubscriberInitExt};
    
    // Create base filter
    let mut filter_str = format!("grpc_http_proxy={}", level);
    
    // Add specific module filters for better control
    filter_str.push_str(&format!(",hyper=warn,rustls=warn,tokio=warn"));
    
    // Allow environment override
    if let Ok(env_filter) = std::env::var("RUST_LOG") {
        filter_str = env_filter;
    }
    
    let filter = EnvFilter::try_new(&filter_str)
        .map_err(|e| format!("Invalid log filter '{}': {}", filter_str, e))?;
    
    // Configure structured logging
    let fmt_layer = fmt::layer()
        .with_target(true)
        .with_thread_ids(true)
        .with_file(true)
        .with_line_number(true)
        .with_level(true)
        .with_ansi(atty::is(atty::Stream::Stdout)) // Only use colors if outputting to terminal
        .compact();
    
    // Initialize the subscriber
    tracing_subscriber::registry()
        .with(filter)
        .with(fmt_layer)
        .init();
    
    // Log initialization
    info!("Logging initialized with level: {}", level);
    info!("Log filter: {}", filter_str);
    
    Ok(())
}

/// Display startup information for the proxy server
fn display_startup_info(config: &ProxyConfig) {
    info!("=== gRPC HTTP Proxy Startup Information ===");
    info!("Version: {}", env!("CARGO_PKG_VERSION"));
    info!("Listen Address: {}:{}", config.listen.address, config.listen.port);
    
    // Display TLS information
    if let Some(ref tls_config) = config.tls {
        info!("TLS Certificate: {}", tls_config.cert_file.display());
        info!("TLS Key: {}", tls_config.key_file.display());
        
        if let Some(ref root_ca_file) = tls_config.root_ca_file {
            info!("TLS Root CA: {}", root_ca_file.display());
            info!("Mode: Encrypted with Mutual TLS (HTTPS/mTLS)");
        } else {
            info!("Mode: Encrypted (HTTPS/TLS)");
        }
    } else {
        info!("Mode: Unencrypted (HTTP)");
        info!("TLS: Disabled");
    }
    
    info!("Log Level: {}", config.logging.level);
    
    info!("Routing Configuration:");
    if config.routing.rules.is_empty() {
        info!("  No specific routing rules configured");
    } else {
        info!("  {} routing rules configured:", config.routing.rules.len());
        for (i, rule) in config.routing.rules.iter().enumerate() {
            info!("    {}. Pattern: '{}' -> {}:{} (priority: {})", 
                  i + 1, rule.pattern, rule.upstream.host, rule.upstream.port, rule.priority);
        }
    }
    
    if let Some(ref catch_all) = config.routing.catch_all {
        info!("  Catch-all route: {}:{}", catch_all.host, catch_all.port);
    } else {
        info!("  No catch-all route configured");
    }
    
    info!("=== End Startup Information ===");
}