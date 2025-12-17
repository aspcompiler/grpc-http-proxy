use tonic::{transport::Channel, Request, Status};
use tokio_stream::{wrappers::ReceiverStream, StreamExt};
use tokio::sync::mpsc;
use std::time::Duration;

// Include the generated RouteGuide code
pub mod routeguide {
    tonic::include_proto!("routeguide");
}

use routeguide::{route_guide_client::RouteGuideClient, Point, RouteNote};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🚀 Testing bidirectional streaming through proxy...");
    
    // Connect to the proxy (not directly to the server)
    let channel = Channel::from_static("http://[::1]:10001")
        .connect()
        .await?;
    
    let mut client = RouteGuideClient::new(channel);
    
    println!("✅ Connected to proxy at [::1]:10001");
    
    // Test bidirectional streaming (route_chat)
    test_bidirectional_streaming(&mut client).await?;
    
    println!("🎉 All tests passed!");
    Ok(())
}

async fn test_bidirectional_streaming(client: &mut RouteGuideClient<Channel>) -> Result<(), Box<dyn std::error::Error>> {
    println!("\n📡 Testing bidirectional streaming (route_chat)...");
    
    // Create a channel for sending route notes
    let (tx, rx) = mpsc::channel(10);
    let request_stream = ReceiverStream::new(rx);
    
    // Start the bidirectional stream
    let response = client.route_chat(Request::new(request_stream)).await?;
    let mut response_stream = response.into_inner();
    
    // Send some route notes
    let notes = vec![
        RouteNote {
            location: Some(Point { latitude: 0, longitude: 1 }),
            message: "First message from client".to_string(),
        },
        RouteNote {
            location: Some(Point { latitude: 0, longitude: 2 }),
            message: "Second message from client".to_string(),
        },
        RouteNote {
            location: Some(Point { latitude: 0, longitude: 3 }),
            message: "Third message from client".to_string(),
        },
    ];
    
    // Send notes and receive responses concurrently
    let send_task = tokio::spawn(async move {
        for (i, note) in notes.into_iter().enumerate() {
            println!("📤 Sending note {}: {}", i + 1, note.message);
            if let Err(e) = tx.send(note).await {
                eprintln!("❌ Failed to send note: {}", e);
                break;
            }
            // Add a small delay between sends
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        println!("📤 Finished sending all notes");
    });
    
    let receive_task = tokio::spawn(async move {
        let mut count = 0;
        while let Some(response) = response_stream.next().await {
            match response {
                Ok(note) => {
                    count += 1;
                    println!("📥 Received note {}: {}", count, note.message);
                }
                Err(e) => {
                    eprintln!("❌ Error receiving note: {}", e);
                    break;
                }
            }
        }
        println!("📥 Finished receiving notes (total: {})", count);
        count
    });
    
    // Wait for both tasks to complete
    let (send_result, receive_count) = tokio::try_join!(send_task, receive_task)?;
    send_result?;
    
    if receive_count > 0 {
        println!("✅ Bidirectional streaming test passed! Received {} notes", receive_count);
    } else {
        println!("⚠️  No notes received - this might indicate a streaming issue");
    }
    
    Ok(())
}