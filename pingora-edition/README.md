# gRPC HTTP Proxy

A high-performance HTTP proxy designed specifically to handle gRPC calls, built in Rust using the Pingora framework.

## Features

- **HTTP/2 Support**: Native HTTP/2 protocol handling for gRPC compatibility
- **TLS Termination**: Optional TLS termination with mutual TLS (mTLS) support
- **Flexible Routing**: Path-based routing to different upstream gRPC servers
- **Trailer Preservation**: Complete HTTP trailer forwarding for gRPC status codes
- **Streaming Support**: Bidirectional streaming without buffering
- **High Performance**: Built on Pingora for enterprise-grade performance

## Configuration

The proxy is configured using a YAML configuration file. See `config.yaml` for an example configuration.

### Basic Configuration

```yaml
server:
  bind_address: "127.0.0.1:8080"
  worker_threads: 4

default_upstream:
  address: "127.0.0.1:9000"
```

### TLS Configuration

```yaml
tls:
  cert_path: "certs/server.crt"
  key_path: "certs/server.key"
  ca_cert_path: "certs/ca.crt"  # For mTLS
```

### Routing Configuration

```yaml
routes:
  - path_pattern: "/api/v1/*"
    upstream:
      address: "127.0.0.1:9001"
```

## Usage

```bash
# Run with default configuration
cargo run

# Run with custom configuration file
cargo run -- --config /path/to/config.yaml
```

## Building

```bash
# Build the project
cargo build --release

# Run tests
cargo test
```

## License

This project is licensed under either of

- Apache License, Version 2.0
- MIT License

at your option.