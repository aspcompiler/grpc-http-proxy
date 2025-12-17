use std::time::Duration;
use tokio::time::sleep;
use bytes::Bytes;
use reqwest::Client;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🚀 Testing bidirectional streaming through gRPC proxy...");
    
    // Test 1: Simple unary call
    test_unary_call().await?;
    
    // Test 2: Streaming request (simulate client streaming)
    test_streaming_request().await?;
    
    println!("🎉 All streaming tests completed!");
    Ok(())
}

async fn test_unary_call() -> Result<(), Box<dyn std::error::Error>> {
    println!("\n📡 Testing unary call through proxy...");
    
    let client = Client::builder()
        .http2_prior_knowledge()
        .timeout(Duration::from_secs(10))
        .build()?;
    
    let response = client
        .post("http://[::1]:10001/routeguide.RouteGuide/GetFeature")
        .header("content-type", "application/grpc")
        .header("te", "trailers")
        .header("grpc-encoding", "identity")
        .body("test unary body")
        .send()
        .await;
    
    match response {
        Ok(resp) => {
            println!("✅ Unary call successful!");
            println!("Status: {}", resp.status());
            println!("Headers: {:?}", resp.headers());
            let body = resp.text().await?;
            println!("Response body length: {}", body.len());
        }
        Err(e) => {
            println!("❌ Unary call failed: {}", e);
        }
    }
    
    Ok(())
}

async fn test_streaming_request() -> Result<(), Box<dyn std::error::Error>> {
    println!("\n📡 Testing streaming request through proxy...");
    
    // Create a streaming body
    let (tx, rx) = tokio::sync::mpsc::channel::<Result<Bytes, std::io::Error>>(10);
    
    // Send streaming data in the background
    tokio::spawn(async move {
        for i in 1..=5 {
            let data = format!("streaming chunk {}", i);
            println!("📤 Sending chunk: {}", data);
            
            if tx.send(Ok(Bytes::from(data))).await.is_err() {
                break;
            }
            
            sleep(Duration::from_millis(200)).await;
        }
        println!("📤 Finished sending streaming chunks");
    });
    
    let stream = tokio_stream::wrappers::ReceiverStream::new(rx);
    let body = reqwest::Body::wrap_stream(stream);
    
    let client = Client::builder()
        .http2_prior_knowledge()
        .timeout(Duration::from_secs(15))
        .build()?;
    
    let response = client
        .post("http://[::1]:10001/routeguide.RouteGuide/RouteChat")
        .header("content-type", "application/grpc")
        .header("te", "trailers")
        .header("grpc-encoding", "identity")
        .body(body)
        .send()
        .await;
    
    match response {
        Ok(resp) => {
            println!("✅ Streaming request successful!");
            println!("Status: {}", resp.status());
            println!("Headers: {:?}", resp.headers());
            
            // Try to read the response as a stream
            let mut stream = resp.bytes_stream();
            let mut chunk_count = 0;
            
            use futures_util::StreamExt;
            while let Some(chunk) = stream.next().await {
                match chunk {
                    Ok(bytes) => {
                        chunk_count += 1;
                        println!("📥 Received chunk {}: {} bytes", chunk_count, bytes.len());
                    }
                    Err(e) => {
                        println!("❌ Error reading chunk: {}", e);
                        break;
                    }
                }
            }
            
            println!("📥 Finished receiving response (total chunks: {})", chunk_count);
            
            if chunk_count > 0 {
                println!("✅ Bidirectional streaming appears to be working!");
            } else {
                println!("⚠️  No response chunks received - streaming may have issues");
            }
        }
        Err(e) => {
            println!("❌ Streaming request failed: {}", e);
        }
    }
    
    Ok(())
}