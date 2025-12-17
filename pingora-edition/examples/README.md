# gRPC HTTP Proxy Configuration Examples

This directory contains example configuration files for the gRPC HTTP Proxy, demonstrating different deployment scenarios and configuration options.

## Configuration Files

### 1. `basic-http2-proxy.yaml`
A simple HTTP/2 proxy configuration without TLS encryption.

**Use Case:** Development environments, internal networks, or scenarios where TLS termination is handled by external load balancers.

**Key Features:**
- Plain HTTP/2 connections (h2c protocol)
- Basic routing with path patterns
- Health checking for upstream servers
- Connection pooling and timeout configuration

### 2. `tls-enabled-proxy.yaml`
An HTTP/2 proxy with TLS termination for secure client connections.

**Use Case:** Production deployments requiring encrypted client connections with server-only authentication.

**Key Features:**
- TLS termination with server certificate
- Automatic HTTP/2 negotiation via ALPN
- Advanced routing with priority-based conflict resolution
- Comprehensive health checking

### 3. `mtls-proxy-with-routing.yaml`
A fully-featured proxy with mutual TLS authentication and sophisticated routing.

**Use Case:** High-security environments requiring client certificate authentication and complex service routing.

**Key Features:**
- Mutual TLS (mTLS) with client certificate validation
- Complex routing patterns for enterprise service architectures
- Priority-based route resolution
- Comprehensive health monitoring

## Configuration Reference

### Server Configuration

```yaml
server:
  bind_address: "0.0.0.0:8080"    # Address and port to bind
  worker_threads: 4               # Number of worker threads (optional)
  max_connections: 1000           # Maximum concurrent connections (optional)
```

**Parameters:**
- `bind_address`: Socket address to bind the proxy server
- `worker_threads`: Number of worker threads (defaults to CPU cores)
- `max_connections`: Maximum concurrent connections (system default if omitted)

### TLS Configuration

```yaml
tls:
  cert_path: "certs/server.crt"     # Server certificate file (PEM format)
  key_path: "certs/server.key"      # Server private key file (PEM format)
  ca_cert_path: "certs/ca.crt"      # CA certificate for mTLS (optional)
```

**Parameters:**
- `cert_path`: Path to server certificate in PEM format
- `key_path`: Path to server private key in PEM format
- `ca_cert_path`: Path to CA certificate for client validation (enables mTLS)

**Notes:**
- ALPN is automatically configured with "h2" for HTTP/2 negotiation
- Supports RSA, ECDSA, and Ed25519 private keys
- Certificate validation includes expiration checking

### Route Configuration

```yaml
routes:
  - path_pattern: "/api/v1/*"       # Path pattern to match
    priority: 10                    # Priority for conflict resolution (optional)
    upstream:
      address: "127.0.0.1:9001"     # Upstream server address
      connection_pool_size: 20      # Connection pool size (optional)
      timeout: "30s"                # Request timeout (optional)
      health_check:                 # Health check configuration (optional)
        path: "/grpc.health.v1.Health/Check"
        interval: "10s"
        timeout: "5s"
```

**Parameters:**
- `path_pattern`: URL path pattern to match (supports wildcards with `*`)
- `priority`: Numeric priority for route resolution (higher = higher priority)
- `upstream.address`: Socket address of the upstream gRPC server
- `upstream.connection_pool_size`: Number of connections to maintain
- `upstream.timeout`: Request timeout duration
- `upstream.health_check`: Optional health check configuration

**Path Pattern Rules:**
- Must start with `/`
- Wildcards (`*`) only allowed at the end
- More specific patterns take precedence
- Priority values resolve conflicts between overlapping patterns

### Default Upstream Configuration

```yaml
default_upstream:
  address: "127.0.0.1:9000"         # Default upstream server
  connection_pool_size: 50          # Connection pool size (optional)
  timeout: "60s"                    # Request timeout (optional)
  health_check:                     # Health check configuration (optional)
    path: "/grpc.health.v1.Health/Check"
    interval: "30s"
    timeout: "10s"
```

**Purpose:** Handles all requests that don't match any route patterns.

### Health Check Configuration

```yaml
health_check:
  path: "/grpc.health.v1.Health/Check"    # Health check endpoint
  interval: "10s"                         # Check interval
  timeout: "5s"                           # Check timeout
```

**Parameters:**
- `path`: gRPC health check service endpoint
- `interval`: Time between health checks
- `timeout`: Timeout for each health check request

**Requirements:**
- Health check timeout must be less than interval
- Upstream servers should implement gRPC Health Checking Protocol

## Certificate Management

### Generating Certificates for Development

#### Self-Signed Certificate (TLS only)
```bash
# Generate private key
openssl genrsa -out certs/server.key 2048

# Generate self-signed certificate
openssl req -new -x509 -days 365 -key certs/server.key -out certs/server.crt
```

#### Certificate Authority and mTLS Certificates
```bash
# 1. Create Certificate Authority
openssl genrsa -out certs/ca.key 4096
openssl req -new -x509 -days 3650 -key certs/ca.key -out certs/ca.crt

# 2. Generate server certificate
openssl genrsa -out certs/server.key 2048
openssl req -new -key certs/server.key -out certs/server.csr
openssl x509 -req -days 365 -in certs/server.csr -CA certs/ca.crt -CAkey certs/ca.key -CAcreateserial -out certs/server.crt

# 3. Generate client certificate
openssl genrsa -out certs/client.key 2048
openssl req -new -key certs/client.key -out certs/client.csr
openssl x509 -req -days 365 -in certs/client.csr -CA certs/ca.crt -CAkey certs/ca.key -CAcreateserial -out certs/client.crt

# 4. Set appropriate permissions
chmod 600 certs/*.key
chmod 644 certs/*.crt
```

### Production Certificate Considerations

1. **Use certificates from trusted CAs** for production deployments
2. **Monitor certificate expiration** and implement automatic renewal
3. **Implement certificate rotation** procedures
4. **Use strong key sizes** (2048-bit RSA minimum, 256-bit ECDSA recommended)
5. **Secure private key storage** with appropriate file permissions
6. **Consider certificate revocation** mechanisms for compromised certificates

## Usage Examples

### Starting the Proxy

```bash
# Basic HTTP/2 proxy
./grpc-http-proxy --config examples/basic-http2-proxy.yaml

# TLS-enabled proxy
./grpc-http-proxy --config examples/tls-enabled-proxy.yaml

# mTLS proxy with routing
./grpc-http-proxy --config examples/mtls-proxy-with-routing.yaml
```

### Client Connection Examples

#### Go Client Examples

**Plain HTTP/2 (h2c):**
```go
conn, err := grpc.Dial("localhost:8080", grpc.WithInsecure())
```

**TLS with server authentication:**
```go
creds := credentials.NewTLS(&tls.Config{ServerName: "your-server.com"})
conn, err := grpc.Dial("localhost:8443", grpc.WithTransportCredentials(creds))
```

**mTLS with client certificate:**
```go
cert, err := tls.LoadX509KeyPair("certs/client.crt", "certs/client.key")
if err != nil {
    log.Fatal(err)
}

creds := credentials.NewTLS(&tls.Config{
    Certificates: []tls.Certificate{cert},
    ServerName:   "your-server.com",
})
conn, err := grpc.Dial("localhost:443", grpc.WithTransportCredentials(creds))
```

#### Python Client Examples

**Plain HTTP/2:**
```python
import grpc

channel = grpc.insecure_channel('localhost:8080')
```

**TLS with server authentication:**
```python
import grpc

credentials = grpc.ssl_channel_credentials()
channel = grpc.secure_channel('localhost:8443', credentials)
```

**mTLS with client certificate:**
```python
import grpc

with open('certs/client.crt', 'rb') as f:
    client_cert = f.read()
with open('certs/client.key', 'rb') as f:
    client_key = f.read()
with open('certs/ca.crt', 'rb') as f:
    ca_cert = f.read()

credentials = grpc.ssl_channel_credentials(
    root_certificates=ca_cert,
    private_key=client_key,
    certificate_chain=client_cert
)
channel = grpc.secure_channel('localhost:443', credentials)
```

## Configuration Validation

The proxy performs comprehensive configuration validation on startup:

1. **File existence checks** for certificates and configuration files
2. **Certificate validation** including expiration and format verification
3. **Route pattern validation** to prevent conflicts and invalid patterns
4. **Network address validation** for bind addresses and upstream servers
5. **Parameter range validation** for timeouts, pool sizes, and thread counts

## Monitoring and Observability

### Health Checks
- Configure health checks for all upstream servers
- Monitor health check failures in logs
- Implement alerting for upstream service failures

### Metrics and Logging
- The proxy provides structured logging for all operations
- Monitor connection counts, request rates, and error rates
- Use distributed tracing for request flow visibility

### Performance Tuning
- Adjust worker thread counts based on CPU cores and load
- Configure connection pool sizes based on expected concurrency
- Set appropriate timeouts for different service types
- Monitor memory usage and adjust max_connections accordingly

## Troubleshooting

### Common Issues

1. **Certificate errors:** Verify certificate paths, permissions, and validity
2. **Connection refused:** Check upstream server availability and addresses
3. **Route conflicts:** Review route patterns and priorities
4. **Performance issues:** Adjust worker threads and connection pool sizes
5. **Health check failures:** Verify upstream servers implement health checking protocol

### Debug Configuration
Enable debug logging by setting appropriate log levels in the proxy configuration or environment variables.

## Security Best Practices

1. **Use TLS in production** environments
2. **Implement mTLS** for high-security requirements
3. **Secure private keys** with appropriate file permissions
4. **Monitor certificate expiration** and implement rotation
5. **Use strong cipher suites** and disable weak protocols
6. **Implement proper access controls** for configuration files
7. **Regular security audits** of certificate chains and configurations
8. **Network segmentation** to isolate proxy and upstream servers