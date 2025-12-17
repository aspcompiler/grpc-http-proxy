#!/bin/bash

# Certificate Generation Script for gRPC HTTP Proxy
# This script generates self-signed certificates for development and testing

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

# Default values
CERT_DIR="certs"
SERVER_NAME="localhost"
ORGANIZATION="gRPC Proxy Development"
COUNTRY="US"
STATE="CA"
CITY="San Francisco"
DAYS_VALID=365
CA_DAYS_VALID=3650

# Function to create certificate directory
create_cert_dir() {
    if [[ ! -d "$CERT_DIR" ]]; then
        mkdir -p "$CERT_DIR"
        print_success "Created certificate directory: $CERT_DIR"
    else
        print_info "Certificate directory already exists: $CERT_DIR"
    fi
    
    # Set appropriate permissions
    chmod 755 "$CERT_DIR"
}

# Function to generate CA certificate
generate_ca() {
    print_info "Generating Certificate Authority (CA)..."
    
    # Generate CA private key
    openssl genrsa -out "$CERT_DIR/ca.key" 4096
    chmod 600 "$CERT_DIR/ca.key"
    print_success "Generated CA private key: $CERT_DIR/ca.key"
    
    # Generate CA certificate
    openssl req -new -x509 -days $CA_DAYS_VALID -key "$CERT_DIR/ca.key" -out "$CERT_DIR/ca.crt" \
        -subj "/C=$COUNTRY/ST=$STATE/L=$CITY/O=$ORGANIZATION/OU=Certificate Authority/CN=gRPC Proxy CA"
    chmod 644 "$CERT_DIR/ca.crt"
    print_success "Generated CA certificate: $CERT_DIR/ca.crt (valid for $CA_DAYS_VALID days)"
}

# Function to generate server certificate
generate_server_cert() {
    print_info "Generating server certificate..."
    
    # Generate server private key
    openssl genrsa -out "$CERT_DIR/server.key" 2048
    chmod 600 "$CERT_DIR/server.key"
    print_success "Generated server private key: $CERT_DIR/server.key"
    
    # Create server certificate signing request
    openssl req -new -key "$CERT_DIR/server.key" -out "$CERT_DIR/server.csr" \
        -subj "/C=$COUNTRY/ST=$STATE/L=$CITY/O=$ORGANIZATION/OU=Server/CN=$SERVER_NAME"
    print_success "Generated server certificate signing request: $CERT_DIR/server.csr"
    
    # Create server certificate extensions file
    cat > "$CERT_DIR/server.ext" << EOF
authorityKeyIdentifier=keyid,issuer
basicConstraints=CA:FALSE
keyUsage = digitalSignature, nonRepudiation, keyEncipherment, dataEncipherment
subjectAltName = @alt_names

[alt_names]
DNS.1 = $SERVER_NAME
DNS.2 = localhost
DNS.3 = *.localhost
IP.1 = 127.0.0.1
IP.2 = ::1
EOF
    
    # Sign server certificate with CA
    if [[ -f "$CERT_DIR/ca.crt" && -f "$CERT_DIR/ca.key" ]]; then
        openssl x509 -req -days $DAYS_VALID -in "$CERT_DIR/server.csr" \
            -CA "$CERT_DIR/ca.crt" -CAkey "$CERT_DIR/ca.key" -CAcreateserial \
            -out "$CERT_DIR/server.crt" -extensions v3_req -extfile "$CERT_DIR/server.ext"
        print_success "Generated server certificate signed by CA: $CERT_DIR/server.crt"
    else
        # Generate self-signed certificate if no CA
        openssl x509 -req -days $DAYS_VALID -in "$CERT_DIR/server.csr" \
            -signkey "$CERT_DIR/server.key" -out "$CERT_DIR/server.crt" \
            -extensions v3_req -extfile "$CERT_DIR/server.ext"
        print_success "Generated self-signed server certificate: $CERT_DIR/server.crt"
    fi
    
    chmod 644 "$CERT_DIR/server.crt"
    
    # Clean up temporary files
    rm -f "$CERT_DIR/server.csr" "$CERT_DIR/server.ext"
}

# Function to generate client certificate
generate_client_cert() {
    print_info "Generating client certificate for mTLS..."
    
    if [[ ! -f "$CERT_DIR/ca.crt" || ! -f "$CERT_DIR/ca.key" ]]; then
        print_error "CA certificate and key are required for client certificate generation"
        return 1
    fi
    
    # Generate client private key
    openssl genrsa -out "$CERT_DIR/client.key" 2048
    chmod 600 "$CERT_DIR/client.key"
    print_success "Generated client private key: $CERT_DIR/client.key"
    
    # Create client certificate signing request
    openssl req -new -key "$CERT_DIR/client.key" -out "$CERT_DIR/client.csr" \
        -subj "/C=$COUNTRY/ST=$STATE/L=$CITY/O=$ORGANIZATION/OU=Client/CN=gRPC Client"
    print_success "Generated client certificate signing request: $CERT_DIR/client.csr"
    
    # Create client certificate extensions file
    cat > "$CERT_DIR/client.ext" << EOF
authorityKeyIdentifier=keyid,issuer
basicConstraints=CA:FALSE
keyUsage = digitalSignature, nonRepudiation, keyEncipherment, dataEncipherment
extendedKeyUsage = clientAuth
EOF
    
    # Sign client certificate with CA
    openssl x509 -req -days $DAYS_VALID -in "$CERT_DIR/client.csr" \
        -CA "$CERT_DIR/ca.crt" -CAkey "$CERT_DIR/ca.key" -CAcreateserial \
        -out "$CERT_DIR/client.crt" -extensions v3_req -extfile "$CERT_DIR/client.ext"
    chmod 644 "$CERT_DIR/client.crt"
    print_success "Generated client certificate signed by CA: $CERT_DIR/client.crt"
    
    # Clean up temporary files
    rm -f "$CERT_DIR/client.csr" "$CERT_DIR/client.ext"
}

# Function to display certificate information
display_cert_info() {
    local cert_file=$1
    local cert_name=$2
    
    if [[ -f "$cert_file" ]]; then
        print_info "$cert_name Certificate Information:"
        echo "  Subject: $(openssl x509 -in "$cert_file" -subject -noout | sed 's/subject=//')"
        echo "  Issuer:  $(openssl x509 -in "$cert_file" -issuer -noout | sed 's/issuer=//')"
        echo "  Valid:   $(openssl x509 -in "$cert_file" -startdate -noout | sed 's/notBefore=//')"
        echo "  Expires: $(openssl x509 -in "$cert_file" -enddate -noout | sed 's/notAfter=//')"
        
        # Check for Subject Alternative Names
        local san
        san=$(openssl x509 -in "$cert_file" -text -noout | grep -A1 "Subject Alternative Name" | tail -1 | sed 's/^[[:space:]]*//' || true)
        if [[ -n "$san" ]]; then
            echo "  SAN:     $san"
        fi
        echo
    fi
}

# Function to verify certificate chain
verify_certificates() {
    print_info "Verifying certificate chain..."
    
    # Verify server certificate against CA
    if [[ -f "$CERT_DIR/server.crt" && -f "$CERT_DIR/ca.crt" ]]; then
        if openssl verify -CAfile "$CERT_DIR/ca.crt" "$CERT_DIR/server.crt" >/dev/null 2>&1; then
            print_success "Server certificate verification: PASSED"
        else
            print_error "Server certificate verification: FAILED"
        fi
    fi
    
    # Verify client certificate against CA
    if [[ -f "$CERT_DIR/client.crt" && -f "$CERT_DIR/ca.crt" ]]; then
        if openssl verify -CAfile "$CERT_DIR/ca.crt" "$CERT_DIR/client.crt" >/dev/null 2>&1; then
            print_success "Client certificate verification: PASSED"
        else
            print_error "Client certificate verification: FAILED"
        fi
    fi
}

# Function to create certificate bundle for easy distribution
create_bundle() {
    print_info "Creating certificate bundle..."
    
    # Create a bundle with all certificates
    if [[ -f "$CERT_DIR/ca.crt" ]]; then
        cat "$CERT_DIR/ca.crt" > "$CERT_DIR/bundle.crt"
        
        if [[ -f "$CERT_DIR/server.crt" ]]; then
            cat "$CERT_DIR/server.crt" >> "$CERT_DIR/bundle.crt"
        fi
        
        print_success "Created certificate bundle: $CERT_DIR/bundle.crt"
    fi
}

# Usage information
usage() {
    echo "Usage: $0 [OPTIONS]"
    echo
    echo "Generates certificates for gRPC HTTP Proxy development and testing."
    echo
    echo "Options:"
    echo "  -d, --dir DIR          Certificate directory (default: certs)"
    echo "  -n, --name NAME        Server name/hostname (default: localhost)"
    echo "  -o, --org ORG          Organization name (default: gRPC Proxy Development)"
    echo "  -c, --country CODE     Country code (default: US)"
    echo "  -s, --state STATE      State/Province (default: CA)"
    echo "  -l, --city CITY        City/Locality (default: San Francisco)"
    echo "  --days DAYS            Certificate validity days (default: 365)"
    echo "  --ca-days DAYS         CA certificate validity days (default: 3650)"
    echo "  --server-only          Generate only server certificate (no CA or client)"
    echo "  --no-client            Skip client certificate generation"
    echo "  -h, --help             Show this help message"
    echo
    echo "Examples:"
    echo "  $0                                    # Generate all certificates with defaults"
    echo "  $0 --name myserver.com               # Generate certificates for myserver.com"
    echo "  $0 --dir /etc/ssl/grpc-proxy         # Use custom certificate directory"
    echo "  $0 --server-only                     # Generate only server certificate"
    echo "  $0 --no-client                       # Skip client certificate"
    echo
    exit 1
}

# Parse command line arguments
GENERATE_CA=true
GENERATE_CLIENT=true
SERVER_ONLY=false

while [[ $# -gt 0 ]]; do
    case $1 in
        -d|--dir)
            CERT_DIR="$2"
            shift 2
            ;;
        -n|--name)
            SERVER_NAME="$2"
            shift 2
            ;;
        -o|--org)
            ORGANIZATION="$2"
            shift 2
            ;;
        -c|--country)
            COUNTRY="$2"
            shift 2
            ;;
        -s|--state)
            STATE="$2"
            shift 2
            ;;
        -l|--city)
            CITY="$2"
            shift 2
            ;;
        --days)
            DAYS_VALID="$2"
            shift 2
            ;;
        --ca-days)
            CA_DAYS_VALID="$2"
            shift 2
            ;;
        --server-only)
            SERVER_ONLY=true
            GENERATE_CA=false
            GENERATE_CLIENT=false
            shift
            ;;
        --no-client)
            GENERATE_CLIENT=false
            shift
            ;;
        -h|--help)
            usage
            ;;
        *)
            print_error "Unknown option: $1"
            usage
            ;;
    esac
done

# Main script
main() {
    print_info "gRPC HTTP Proxy Certificate Generator"
    print_info "====================================="
    echo
    
    # Check dependencies
    if ! command -v openssl >/dev/null 2>&1; then
        print_error "openssl is required but not installed"
        exit 1
    fi
    
    print_info "Configuration:"
    echo "  Certificate directory: $CERT_DIR"
    echo "  Server name: $SERVER_NAME"
    echo "  Organization: $ORGANIZATION"
    echo "  Country: $COUNTRY"
    echo "  State: $STATE"
    echo "  City: $CITY"
    echo "  Certificate validity: $DAYS_VALID days"
    if [[ "$GENERATE_CA" == true ]]; then
        echo "  CA validity: $CA_DAYS_VALID days"
    fi
    echo
    
    # Create certificate directory
    create_cert_dir
    
    # Generate certificates
    if [[ "$GENERATE_CA" == true ]]; then
        generate_ca
    fi
    
    generate_server_cert
    
    if [[ "$GENERATE_CLIENT" == true ]]; then
        generate_client_cert
    fi
    
    # Verify certificates
    verify_certificates
    
    # Create bundle
    create_bundle
    
    echo
    print_success "Certificate generation completed!"
    echo
    
    # Display certificate information
    if [[ -f "$CERT_DIR/ca.crt" ]]; then
        display_cert_info "$CERT_DIR/ca.crt" "CA"
    fi
    
    if [[ -f "$CERT_DIR/server.crt" ]]; then
        display_cert_info "$CERT_DIR/server.crt" "Server"
    fi
    
    if [[ -f "$CERT_DIR/client.crt" ]]; then
        display_cert_info "$CERT_DIR/client.crt" "Client"
    fi
    
    # Usage instructions
    print_info "Usage Instructions:"
    echo
    
    if [[ "$SERVER_ONLY" == true ]]; then
        echo "  For TLS-only proxy (server authentication):"
        echo "    - Use server.crt and server.key in your proxy configuration"
        echo "    - Configure tls.cert_path and tls.key_path in your YAML config"
        echo
    elif [[ -f "$CERT_DIR/ca.crt" ]]; then
        echo "  For TLS-only proxy (server authentication):"
        echo "    - Use server.crt and server.key in your proxy configuration"
        echo "    - Configure tls.cert_path and tls.key_path in your YAML config"
        echo
        echo "  For mTLS proxy (mutual authentication):"
        echo "    - Use server.crt, server.key, and ca.crt in your proxy configuration"
        echo "    - Configure tls.cert_path, tls.key_path, and tls.ca_cert_path in your YAML config"
        echo "    - Clients must present certificates signed by the CA (ca.crt)"
        echo
        if [[ -f "$CERT_DIR/client.crt" ]]; then
            echo "  For client applications:"
            echo "    - Use client.crt and client.key for mTLS authentication"
            echo "    - Trust the CA certificate (ca.crt) for server verification"
            echo
        fi
    fi
    
    print_warning "Security Notice:"
    echo "  These certificates are for DEVELOPMENT and TESTING only!"
    echo "  For production use:"
    echo "    - Use certificates from a trusted Certificate Authority"
    echo "    - Implement proper certificate rotation procedures"
    echo "    - Secure private keys with appropriate permissions and storage"
    echo "    - Monitor certificate expiration dates"
    echo
}

# Run main function
main