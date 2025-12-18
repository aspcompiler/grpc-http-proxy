use std::convert::Infallible;
use std::net::SocketAddr;
use hyper::service::service_fn;
use hyper::{Request, Response, StatusCode};
use hyper::body::Incoming;
use hyper_util::rt::TokioIo;
use hyper_util::server::conn::auto::Builder;
use tokio::net::TcpListener;
use tokio::time::{sleep, Duration};
use http_body_util::{BodyExt, Full};
use bytes::Bytes;

async fn handle_request(req: Request<Incoming>) -> Result<Response<Full<Bytes>>, Infallible> {
    println!("📥 Mock upstream received: {} {}", req.method(), req.uri());
    
    // Print headers
    for (name, value) in req.headers() {
        println!("  {}: {:?}", name, value);
    }
    
    // Read the body
    let body_bytes = req.collect().await.unwrap().to_bytes();
    println!("  Body: {} bytes", body_bytes.len());
    
    // Simulate some processing time
    sleep(Duration::from_millis(100)).await;
    
    // Return a gRPC response
    let response = Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "application/grpc")
        .header("grpc-status", "0")
        .header("grpc-message", "OK")
        .body(Full::new(Bytes::from("mock response data")))
        .unwrap();
    
    println!("📤 Mock upstream responding with status 200");
    Ok(response)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🚀 Starting mock upstream server on [::1]:10000");
    
    let addr = SocketAddr::from(([0, 0, 0, 0, 0, 0, 0, 1], 10000));
    let listener = TcpListener::bind(addr).await?;
    
    println!("✅ Mock upstream server listening on {}", addr);
    
    loop {
        let (stream, _) = listener.accept().await?;
        let io = TokioIo::new(stream);
        
        tokio::task::spawn(async move {
            if let Err(err) = Builder::new(hyper_util::rt::TokioExecutor::new())
                .serve_connection(io, service_fn(handle_request))
                .await
            {
                eprintln!("❌ Error serving connection: {:?}", err);
            }
        });
    }
}