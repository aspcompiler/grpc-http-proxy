use std::time::{Duration, Instant};
use tokio::time::sleep;
use bytes::Bytes;
use reqwest::Client;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🚀 Testing long bidirectional streaming through gRPC proxy...");
    
    test_long_streaming_request().await?;
    
    println!("🎉 Long streaming test completed!");
    Ok(())
}

async fn test_long_streaming_request() -> Result<(), Box<dyn std::error::Error>> {
    println!("\n📡 Testing long streaming request through proxy...");
    
    let start_time = Instant::now();
    
    // Create a streaming body with many chunks
    let (tx, rx) = tokio::sync::mpsc::channel::<Result<Bytes, std::io::Error>>(10);
    
    // Send streaming data in the background
    let send_handle = tokio::spawn(async move {
        for i in 1..=50 {  // Send 50 chunks
            let data = format!("streaming chunk {} with some data to make it larger", i);
            println!("📤 Sending chunk: {} ({})", i, data.len());
            
            if tx.send(Ok(Bytes::from(data))).await.is_err() {
                println!("❌ Failed to send chunk {}", i);
                break;
            }
            
            // Small delay between chunks to simulate real streaming
            sleep(Duration::from_millis(50)).await;
        }
        println!("📤 Finished sending all streaming chunks");
    });
    
    let stream = tokio_stream::wrappers::ReceiverStream::new(rx);
    let body = reqwest::Body::wrap_stream(stream);
    
    let client = Client::builder()
        .http2_prior_knowledge()
        .timeout(Duration::from_secs(30))  // Longer timeout for long stream
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
            println!("✅ Long streaming request successful!");
            println!("Status: {}", resp.status());
            
            // Try to read the response as a stream
            let mut stream = resp.bytes_stream();
            let mut chunk_count = 0;
            let mut total_bytes = 0;
            
            use futures_util::StreamExt;
            while let Some(chunk) = stream.next().await {
                match chunk {
                    Ok(bytes) => {
                        chunk_count += 1;
                        total_bytes += bytes.len();
                        if chunk_count <= 5 || chunk_count % 10 == 0 {
                            println!("📥 Received chunk {}: {} bytes", chunk_count, bytes.len());
                        }
                    }
                    Err(e) => {
                        println!("❌ Error reading chunk: {}", e);
                        break;
                    }
                }
            }
            
            let elapsed = start_time.elapsed();
            println!("📥 Finished receiving response:");
            println!("   - Total chunks: {}", chunk_count);
            println!("   - Total bytes: {}", total_bytes);
            println!("   - Time elapsed: {:?}", elapsed);
            
            // Wait for send task to complete
            if let Err(e) = send_handle.await {
                println!("❌ Send task failed: {}", e);
            }
            
            if elapsed < Duration::from_secs(10) {
                println!("✅ Streaming completed quickly - no blocking detected!");
            } else {
                println!("⚠️  Streaming took longer than expected - possible blocking");
            }
        }
        Err(e) => {
            println!("❌ Long streaming request failed: {}", e);
        }
    }
    
    Ok(())
}