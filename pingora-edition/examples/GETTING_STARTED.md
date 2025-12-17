# Getting Started with gRPC HTTP Proxy

This guide will help you quickly get started with the gRPC HTTP Proxy using the provided example configurations.

## Quick Start

### 1. Choose Your Configuration

Select the configuration that best matches your use case:

- **`basic-http2-proxy.yaml`** - Simple HTTP/2 proxy without encryption (development/testing)
- **`tls-enabled-proxy.yaml`** - HTTP/2 proxy with TLS server authentication (production)
- **`mtls-proxy-with-routing.yaml`** - Full mTLS with client authentication and advanced routing (high security)

### 2. Generate Certificates (if needed)

For TLS or mTLS configurations, generate the required certificates:

```bash
# For TLS-only (server authentication)
./examples/generate-certs.sh --no-client

# For mTLS (mutual authentication)
./examples/generate-certs.sh

# Custom server name
./examples/generate-certs.sh --name myserver.com
```

### 3. Validate Your Configuration

Before starting the proxy, validate your configuration:

```bash
# Validate configuration and certificates
./examples/validate-config.sh examples/tls-enabled-proxy.yaml
```

### 4. Start the Proxy

```bash
# Using the basic configuration
./grpc-http-proxy --config examples/basic-http2-proxy.yaml

# Using TLS configuration
./grpc-http-proxy --config examples/tls-enabled-proxy.yaml

# Using mTLS configuration
./grpc-http-proxy --config examples/mtls-proxy-with-routing.yaml
```

## Configuration Scenarios

### Scenario 1: Development Environment

**Use Case:** Local development with plain HTTP/2

**Configuration:** `basic-http2-proxy.yaml`

**Setup:**
```bash
# No certificates needed
./grpc-http-proxy --config examples/basic-http2-proxy.yaml
```

**Client Connection:**
```go
// Go client
conn, err := grpc.Dial("localhost:8080", grpc.WithInsecure())
```

### Scenario 2: Production with TLS

**Use Case:** Production deployment with encrypted connections

**Configuration:** `tls-enabled-proxy.yaml`

**Setup:**
```bash
# Generate server certificates
./examples/generate-certs.sh --no-client --name your-domain.com

# Validate configuration
./examples/validate-config.sh examples/tls-enabled-proxy.yaml

# Start proxy
./grpc-http-proxy --config examples/tls-enabled-proxy.yaml
```

**Client Connection:**
```go
// Go client with TLS
creds := credentials.NewTLS(&tls.Config{ServerName: "your-domain.com"})
conn, err := grpc.Dial("your-domain.com:8443", grpc.WithTransportCredentials(creds))
```

### Scenario 3: High Security with mTLS

**Use Case:** Enterprise deployment requiring client certificate authentication

**Configuration:** `mtls-proxy-with-routing.yaml`

**Setup:**
```bash
# Generate all certificates (CA, server, client)
./examples/generate-certs.sh --name your-domain.com

# Validate configuration
./examples/validate-config.sh examples/mtls-proxy-with-routing.yaml

# Start proxy
./grpc-http-proxy --config examples/mtls-proxy-with-routing.yaml
```

**Client Connection:**
```go
// Go client with mTLS
cert, err := tls.LoadX509KeyPair("certs/client.crt", "certs/client.key")
if err != nil {
    log.Fatal(err)
}

creds := credentials.NewTLS(&tls.Config{
    Certificates: []tls.Certificate{cert},
    ServerName:   "your-domain.com",
})
conn, err := grpc.Dial("your-domain.com:443", grpc.WithTransportCredentials(creds))
```

## Customizing Configurations

### Basic Customization

1. **Change bind address:**
   ```yaml
   server:
     bind_address: "0.0.0.0:9090"  # Listen on port 9090
   ```

2. **Add upstream servers:**
   ```yaml
   routes:
     - path_pattern: "/myservice.*"
       upstream:
         address: "backend1.example.com:9001"
         timeout: "30s"
   ```

3. **Configure connection pooling:**
   ```yaml
   default_upstream:
     address: "backend.example.com:9000"
     connection_pool_size: 100  # Increase pool size
   ```

### Advanced Routing

```yaml
routes:
  # High priority for authentication
  - path_pattern: "/auth.AuthService/*"
    priority: 100
    upstream:
      address: "auth-server:9001"
      timeout: "10s"
  
  # Lower priority for general API
  - path_pattern: "/api/*"
    priority: 50
    upstream:
      address: "api-server:9002"
      timeout: "30s"
```

### Health Checks

```yaml
upstream:
  address: "backend:9000"
  health_check:
    path: "/grpc.health.v1.Health/Check"
    interval: "10s"
    timeout: "3s"
```

## Troubleshooting

### Common Issues

1. **Certificate errors:**
   ```bash
   # Regenerate certificates
   rm -rf certs/
   ./examples/generate-certs.sh
   ```

2. **Connection refused:**
   ```bash
   # Check if upstream servers are running
   telnet backend-server 9000
   ```

3. **Route conflicts:**
   ```bash
   # Validate configuration
   ./examples/validate-config.sh your-config.yaml
   ```

### Debug Mode

Enable debug logging by setting environment variables:
```bash
RUST_LOG=debug ./grpc-http-proxy --config your-config.yaml
```

### Testing Connectivity

```bash
# Test upstream connectivity
curl -v http://backend-server:9000/health

# Test proxy connectivity (plain HTTP/2)
curl -v --http2-prior-knowledge http://localhost:8080/

# Test proxy connectivity (TLS)
curl -v --http2 https://localhost:8443/
```

## Production Deployment

### Security Checklist

- [ ] Use certificates from a trusted CA (not self-signed)
- [ ] Set appropriate file permissions on private keys (600)
- [ ] Configure firewall rules to restrict access
- [ ] Enable structured logging and monitoring
- [ ] Implement certificate rotation procedures
- [ ] Set up health check monitoring
- [ ] Configure resource limits (memory, CPU)

### Performance Tuning

1. **Worker threads:** Set to 1-2x CPU cores
2. **Connection pools:** Size based on expected concurrency
3. **Timeouts:** Adjust based on service characteristics
4. **Max connections:** Set based on available memory

### Monitoring

Monitor these key metrics:
- Connection count and rate
- Request latency and throughput
- Error rates by service
- Certificate expiration dates
- Upstream server health

## Next Steps

1. **Read the full documentation:** See `examples/README.md` for complete configuration reference
2. **Set up monitoring:** Implement health checks and metrics collection
3. **Configure logging:** Set up structured logging for production
4. **Plan certificate management:** Implement rotation and renewal procedures
5. **Load testing:** Validate performance under expected load

## Support

For issues and questions:
1. Check the troubleshooting section above
2. Validate your configuration with the provided tools
3. Review the example configurations for reference
4. Check the proxy logs for detailed error messages

## Example Commands Summary

```bash
# Generate certificates
./examples/generate-certs.sh

# Validate configuration
./examples/validate-config.sh examples/tls-enabled-proxy.yaml

# Start proxy
./grpc-http-proxy --config examples/tls-enabled-proxy.yaml

# Test connectivity
curl -v --http2 https://localhost:8443/health
```