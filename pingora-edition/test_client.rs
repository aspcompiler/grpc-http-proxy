#!/usr/bin/env cargo +nightly -Zscript
//! Simple HTTP/2 client to test the gRPC proxy server
//! 
//! Usage: cargo +nightly -Zscript test_client.rs [port]

use std::env;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let port = env::args().nth(1).unwrap_or_else(|| "8080".to_string());
    let url = format!("http://127.0.0.1:{}/test.Service/Method", port);
    
    println!("Testing gRPC proxy server at {}", url);
    
    // Create an HTTP/2 client
    let client = reqwest::Client::builder()
        .http2_prior_knowledge()
        .build()?;
    
    // Create a gRPC-like request
    let response = client
        .post(&url)
        .header("content-type", "application/grpc")
        .header("te", "trailers")
        .header("grpc-encoding", "identity")
        .body("test gRPC body")
        .send()
        .await;
    
    match response {
        Ok(resp) => {
            println!("✅ Connection successful!");
            println!("Status: {}", resp.status());
            println!("Version: {:?}", resp.version());
            
            // Print headers
            println!("Headers:");
            for (name, value) in resp.headers() {
                println!("  {}: {:?}", name, value);
            }
            
            // Print body
            let body = resp.text().await?;
            println!("Body: {}", body);
        }
        Err(e) => {
            println!("❌ Connection failed: {}", e);
            
            // Check if server is running
            let tcp_check = std::net::TcpStream::connect(format!("127.0.0.1:{}", port));
            match tcp_check {
                Ok(_) => println!("✅ TCP connection successful - server is listening"),
                Err(e) => println!("❌ TCP connection failed: {} - server may not be running", e),
            }
        }
    }
    
    Ok(())
}

// Cargo.toml embedded in the script
/*
[dependencies]
tokio = { version = "1.0", features = ["full"] }
reqwest = { version = "0.11", features = ["json"] }
*/