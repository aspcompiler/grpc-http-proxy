#!/bin/bash

# Configuration Validation Script for gRPC HTTP Proxy
# This script helps validate configuration files and certificates before starting the proxy

set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Function to print colored output
print_status() {
    local color=$1
    local message=$2
    echo -e "${color}${message}${NC}"
}

print_success() {
    print_status "$GREEN" "✓ $1"
}

print_error() {
    print_status "$RED" "✗ $1"
}

print_warning() {
    print_status "$YELLOW" "⚠ $1"
}

print_info() {
    print_status "$BLUE" "ℹ $1"
}

# Function to check if a file exists and is readable
check_file() {
    local file_path=$1
    local file_type=$2
    
    if [[ ! -f "$file_path" ]]; then
        print_error "$file_type file not found: $file_path"
        return 1
    fi
    
    if [[ ! -r "$file_path" ]]; then
        print_error "$file_type file not readable: $file_path"
        return 1
    fi
    
    print_success "$file_type file exists and is readable: $file_path"
    return 0
}

# Function to validate certificate
validate_certificate() {
    local cert_path=$1
    local cert_type=$2
    
    if ! check_file "$cert_path" "$cert_type certificate"; then
        return 1
    fi
    
    # Check if certificate is valid PEM format
    if ! openssl x509 -in "$cert_path" -text -noout >/dev/null 2>&1; then
        print_error "$cert_type certificate is not valid PEM format: $cert_path"
        return 1
    fi
    
    # Check certificate expiration
    local expiry_date
    expiry_date=$(openssl x509 -in "$cert_path" -enddate -noout | cut -d= -f2)
    local expiry_epoch
    expiry_epoch=$(date -d "$expiry_date" +%s 2>/dev/null || date -j -f "%b %d %H:%M:%S %Y %Z" "$expiry_date" +%s 2>/dev/null)
    local current_epoch
    current_epoch=$(date +%s)
    
    if [[ $expiry_epoch -lt $current_epoch ]]; then
        print_error "$cert_type certificate has expired: $cert_path (expired: $expiry_date)"
        return 1
    fi
    
    # Check if certificate expires within 30 days
    local thirty_days_epoch
    thirty_days_epoch=$((current_epoch + 30 * 24 * 3600))
    if [[ $expiry_epoch -lt $thirty_days_epoch ]]; then
        print_warning "$cert_type certificate expires soon: $cert_path (expires: $expiry_date)"
    else
        print_success "$cert_type certificate is valid and not expiring soon: $cert_path"
    fi
    
    return 0
}

# Function to validate private key
validate_private_key() {
    local key_path=$1
    
    if ! check_file "$key_path" "Private key"; then
        return 1
    fi
    
    # Check if private key is valid
    if openssl rsa -in "$key_path" -check -noout >/dev/null 2>&1; then
        print_success "Private key is valid RSA key: $key_path"
        return 0
    elif openssl ec -in "$key_path" -check -noout >/dev/null 2>&1; then
        print_success "Private key is valid EC key: $key_path"
        return 0
    elif openssl pkey -in "$key_path" -check -noout >/dev/null 2>&1; then
        print_success "Private key is valid: $key_path"
        return 0
    else
        print_error "Private key is not valid: $key_path"
        return 1
    fi
}

# Function to validate certificate and key match
validate_cert_key_match() {
    local cert_path=$1
    local key_path=$2
    
    local cert_modulus
    local key_modulus
    
    # Get certificate public key
    cert_modulus=$(openssl x509 -in "$cert_path" -modulus -noout 2>/dev/null | openssl md5)
    
    # Get private key public key
    key_modulus=$(openssl rsa -in "$key_path" -modulus -noout 2>/dev/null | openssl md5 2>/dev/null || \
                  openssl ec -in "$key_path" -pubout 2>/dev/null | openssl md5 2>/dev/null || \
                  openssl pkey -in "$key_path" -pubout 2>/dev/null | openssl md5 2>/dev/null)
    
    if [[ "$cert_modulus" == "$key_modulus" ]]; then
        print_success "Certificate and private key match"
        return 0
    else
        print_error "Certificate and private key do not match"
        return 1
    fi
}

# Function to check network connectivity
check_upstream_connectivity() {
    local address=$1
    local host
    local port
    
    # Extract host and port from address
    if [[ $address =~ ^([^:]+):([0-9]+)$ ]]; then
        host="${BASH_REMATCH[1]}"
        port="${BASH_REMATCH[2]}"
    else
        print_error "Invalid upstream address format: $address"
        return 1
    fi
    
    # Test connectivity
    if timeout 5 bash -c "</dev/tcp/$host/$port" >/dev/null 2>&1; then
        print_success "Upstream server is reachable: $address"
        return 0
    else
        print_warning "Upstream server is not reachable: $address (this may be expected if the server is not running)"
        return 0  # Don't fail validation for unreachable upstreams
    fi
}

# Main validation function
validate_config() {
    local config_file=$1
    local validation_errors=0
    
    print_info "Validating configuration file: $config_file"
    echo
    
    # Check if configuration file exists
    if ! check_file "$config_file" "Configuration"; then
        return 1
    fi
    
    # Parse YAML configuration (requires yq or python)
    if command -v yq >/dev/null 2>&1; then
        # Use yq if available
        local cert_path
        local key_path
        local ca_cert_path
        local bind_address
        local upstreams
        
        cert_path=$(yq eval '.tls.cert_path // ""' "$config_file" 2>/dev/null)
        key_path=$(yq eval '.tls.key_path // ""' "$config_file" 2>/dev/null)
        ca_cert_path=$(yq eval '.tls.ca_cert_path // ""' "$config_file" 2>/dev/null)
        bind_address=$(yq eval '.server.bind_address // ""' "$config_file" 2>/dev/null)
        
        # Validate TLS configuration if present
        if [[ -n "$cert_path" && "$cert_path" != "null" ]]; then
            print_info "Validating TLS configuration..."
            
            if ! validate_certificate "$cert_path" "Server"; then
                ((validation_errors++))
            fi
            
            if [[ -n "$key_path" && "$key_path" != "null" ]]; then
                if ! validate_private_key "$key_path"; then
                    ((validation_errors++))
                fi
                
                # Check if certificate and key match
                if ! validate_cert_key_match "$cert_path" "$key_path"; then
                    ((validation_errors++))
                fi
            else
                print_error "TLS certificate specified but no private key found"
                ((validation_errors++))
            fi
            
            # Validate CA certificate for mTLS
            if [[ -n "$ca_cert_path" && "$ca_cert_path" != "null" ]]; then
                print_info "mTLS enabled - validating CA certificate..."
                if ! validate_certificate "$ca_cert_path" "CA"; then
                    ((validation_errors++))
                fi
            fi
        else
            print_info "No TLS configuration found - proxy will run in plain HTTP/2 mode"
        fi
        
        # Validate bind address
        if [[ -n "$bind_address" && "$bind_address" != "null" ]]; then
            print_success "Bind address configured: $bind_address"
        else
            print_error "No bind address specified in server configuration"
            ((validation_errors++))
        fi
        
        # Validate upstream servers
        print_info "Checking upstream server connectivity..."
        
        # Check default upstream
        local default_upstream
        default_upstream=$(yq eval '.default_upstream.address // ""' "$config_file" 2>/dev/null)
        if [[ -n "$default_upstream" && "$default_upstream" != "null" ]]; then
            check_upstream_connectivity "$default_upstream"
        fi
        
        # Check route upstreams
        local route_count
        route_count=$(yq eval '.routes | length' "$config_file" 2>/dev/null)
        if [[ "$route_count" != "null" && "$route_count" -gt 0 ]]; then
            for ((i=0; i<route_count; i++)); do
                local route_upstream
                route_upstream=$(yq eval ".routes[$i].upstream.address // \"\"" "$config_file" 2>/dev/null)
                if [[ -n "$route_upstream" && "$route_upstream" != "null" ]]; then
                    check_upstream_connectivity "$route_upstream"
                fi
            done
        fi
        
    else
        print_warning "yq not found - skipping detailed configuration parsing"
        print_info "Install yq for comprehensive configuration validation: https://github.com/mikefarah/yq"
    fi
    
    echo
    if [[ $validation_errors -eq 0 ]]; then
        print_success "Configuration validation completed successfully!"
        return 0
    else
        print_error "Configuration validation failed with $validation_errors error(s)"
        return 1
    fi
}

# Usage information
usage() {
    echo "Usage: $0 <config-file>"
    echo
    echo "Validates a gRPC HTTP Proxy configuration file and associated certificates."
    echo
    echo "Examples:"
    echo "  $0 examples/basic-http2-proxy.yaml"
    echo "  $0 examples/tls-enabled-proxy.yaml"
    echo "  $0 examples/mtls-proxy-with-routing.yaml"
    echo "  $0 /path/to/your/config.yaml"
    echo
    echo "Requirements:"
    echo "  - openssl (for certificate validation)"
    echo "  - yq (optional, for detailed configuration parsing)"
    echo
    exit 1
}

# Main script
main() {
    if [[ $# -ne 1 ]]; then
        usage
    fi
    
    local config_file=$1
    
    # Check dependencies
    if ! command -v openssl >/dev/null 2>&1; then
        print_error "openssl is required but not installed"
        exit 1
    fi
    
    print_info "gRPC HTTP Proxy Configuration Validator"
    print_info "========================================"
    echo
    
    if validate_config "$config_file"; then
        echo
        print_success "Configuration is valid and ready to use!"
        exit 0
    else
        echo
        print_error "Configuration validation failed. Please fix the errors above."
        exit 1
    fi
}

# Run main function with all arguments
main "$@"