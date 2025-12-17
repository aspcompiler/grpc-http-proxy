#!/bin/bash

# Test script for the gRPC HTTP proxy server

PORT=${1:-27016}
CONFIG=${2:-config-27016.yaml}

echo "🚀 Testing gRPC HTTP Proxy Server"
echo "Port: $PORT"
echo "Config: $CONFIG"
echo ""

# Check if server is running
echo "1. Checking if server is listening on port $PORT..."
if lsof -i :$PORT > /dev/null 2>&1; then
    echo "✅ Server is listening on port $PORT"
else
    echo "❌ No server found on port $PORT"
    echo ""
    echo "To start the server, run:"
    echo "  cargo run -- --config $CONFIG"
    exit 1
fi

echo ""

# Test TCP connection
echo "2. Testing TCP connection..."
if timeout 5 bash -c "</dev/tcp/127.0.0.1/$PORT" 2>/dev/null; then
    echo "✅ TCP connection successful"
else
    echo "❌ TCP connection failed"
    exit 1
fi

echo ""

# Test HTTP/2 with curl (if available)
echo "3. Testing HTTP/2 with curl..."
if command -v curl > /dev/null 2>&1; then
    echo "Sending gRPC request..."
    
    RESPONSE=$(curl -s -w "\nHTTP_CODE:%{http_code}\nHTTP_VERSION:%{http_version}\n" \
        --http2-prior-knowledge \
        -X POST \
        -H "Content-Type: application/grpc" \
        -H "TE: trailers" \
        -H "grpc-encoding: identity" \
        -d "test gRPC body" \
        "http://127.0.0.1:$PORT/test.Service/Method" 2>&1)
    
    echo "Response:"
    echo "$RESPONSE"
    
    # Check if we got a response
    if echo "$RESPONSE" | grep -q "HTTP_CODE:"; then
        HTTP_CODE=$(echo "$RESPONSE" | grep "HTTP_CODE:" | cut -d: -f2)
        HTTP_VERSION=$(echo "$RESPONSE" | grep "HTTP_VERSION:" | cut -d: -f2)
        
        echo ""
        echo "HTTP Code: $HTTP_CODE"
        echo "HTTP Version: $HTTP_VERSION"
        
        if [ "$HTTP_CODE" = "200" ] || [ "$HTTP_CODE" = "500" ]; then
            echo "✅ HTTP/2 request successful (got response)"
        else
            echo "⚠️  HTTP/2 request completed but with unexpected status: $HTTP_CODE"
        fi
    else
        echo "❌ HTTP/2 request failed"
    fi
else
    echo "⚠️  curl not available, skipping HTTP/2 test"
fi

echo ""

# Test with grpcurl (if available)
echo "4. Testing with grpcurl (if available)..."
if command -v grpcurl > /dev/null 2>&1; then
    echo "Testing with grpcurl..."
    grpcurl -plaintext -d '{}' 127.0.0.1:$PORT list 2>&1 || echo "grpcurl test completed (expected to fail for mock server)"
else
    echo "⚠️  grpcurl not available, skipping gRPC-specific test"
fi

echo ""
echo "🏁 Test completed"