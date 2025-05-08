use tokio::sync::mpsc::UnboundedReceiver;
use futures::stream::Stream; // Required for using StreamExt
use warp::Filter; // Required for using warp filters
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use warp::ws::{Message, WebSocket};
use uuid::Uuid;
use futures::{StreamExt, SinkExt};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::error::Error as StdError;
use warp::reject::Reject;
use warp::Rejection;
use warp::Reply;
use std::time::{SystemTime, UNIX_EPOCH};
use std::env;
use tokio_stream;

type Users = Arc<RwLock<HashMap<String, User>>>;
type WaitingUsers = Arc<RwLock<Vec<String>>>;
type IceServers = Arc<Vec<IceServer>>;

// Example filter for processing messages from a channel
// Commented out as it's not used in the current implementation
/*
const FILTER: warp::Filter<()> = warp::filters::any()
    .map(move || {
        let mut receiver = /* your receiver */;
        move || {
            // Wrapped logic to handle messages from the receiver
            if let Ok(message) = receiver.try_recv() {
                // process your message
            }
        }
    });
*/

#[derive(Debug)]
struct User {
    id: String,
    tx: tokio::sync::mpsc::UnboundedSender<Message>,
}

#[derive(Serialize, Deserialize)]
struct WebRTCMessage {
    target: String,
    message_type: String,
    data: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    source: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct IceServer {
    urls: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    username: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    credential: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct IceServerConfig {
    ice_servers: Vec<IceServer>,
}

// Custom error types
#[derive(Debug)]
enum AppError {
    WebSocketError(String),
    ParseError(String),
    ConnectionError(String),
    TargetNotFound(String),
    InternalError(String),
    InvalidMessageType(String),
    AuthenticationError(String),
    RateLimitExceeded(String),
    MessageTooLarge(String),
    PermissionDenied(String),
}

impl AppError {
    // Get a user-friendly error message and error code
    pub fn user_message(&self) -> (&'static str, u16) {
        match self {
            AppError::WebSocketError(_) => ("Connection error occurred", 1001),
            AppError::ParseError(_) => ("Invalid message format", 1002),
            AppError::ConnectionError(_) => ("Failed to deliver message", 1003),
            AppError::TargetNotFound(_) => ("Recipient not found", 1004),
            AppError::InternalError(_) => ("Server encountered an error", 1005),
            AppError::InvalidMessageType(_) => ("Unsupported message type", 1006),
            AppError::AuthenticationError(_) => ("Authentication failed", 1007),
            AppError::RateLimitExceeded(_) => ("Too many messages sent", 1008),
            AppError::MessageTooLarge(_) => ("Message exceeds size limit", 1009),
            AppError::PermissionDenied(_) => ("Operation not permitted", 1010),
        }
    }
    
    // Get detailed error information for logging
    pub fn log_details(&self) -> String {
        match self {
            AppError::WebSocketError(msg) => format!("WebSocket error: {}", msg),
            AppError::ParseError(msg) => format!("Parse error: {}", msg),
            AppError::ConnectionError(msg) => format!("Connection error: {}", msg),
            AppError::TargetNotFound(msg) => format!("Target not found: {}", msg),
            AppError::InternalError(msg) => format!("Internal server error: {}", msg),
            AppError::InvalidMessageType(msg) => format!("Invalid message type: {}", msg),
            AppError::AuthenticationError(msg) => format!("Authentication error: {}", msg),
            AppError::RateLimitExceeded(msg) => format!("Rate limit exceeded: {}", msg),
            AppError::MessageTooLarge(msg) => format!("Message too large: {}", msg),
            AppError::PermissionDenied(msg) => format!("Permission denied: {}", msg),
        }
    }
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let (user_msg, code) = self.user_message();
        write!(f, "[{}] {}", code, user_msg)
    }
}

impl StdError for AppError {}

impl Reject for AppError {}

// Error response for clients
#[derive(Serialize)]
struct ErrorMessage {
    error: String,
    code: u16,
    details: Option<String>,
}

async fn handle_websocket(ws: WebSocket, users: Users, waiting: WaitingUsers, ice_servers: IceServers, my_id: String) {
    let (user_tx, mut user_rx) = tokio::sync::mpsc::unbounded_channel();
    let (mut ws_tx, mut ws_rx) = ws.split();

    // Send client their ID and ICE server configuration
    let connect_msg = serde_json::json!({
        "message_type": "connected",
        "data": my_id.clone(),
        "ice_servers": ice_servers.as_ref(),
    }).to_string();
    
    if let Err(e) = ws_tx.send(Message::text(connect_msg)).await {
        eprintln!("Error sending connection message: {}", e);
        return;
    }

    // Add user to active users map
    users.write().await.insert(my_id.clone(), User {
        id: my_id.clone(),
        tx: user_tx,
    });

    // Log connection
    eprintln!("User {} connected, total users: {}", my_id, users.read().await.len());

    let users_clone = users.clone();
    let my_id_clone = my_id.clone();
    let forward_messages = ws_rx.for_each(|msg| {
        let users = users_clone.clone();
        let my_id = my_id_clone.clone();
        async move {
            let msg = match msg {
                Ok(msg) => msg,
                Err(e) => {
                    let error = AppError::WebSocketError(format!("Error receiving message: {}", e));
                    eprintln!("{}", error.log_details());
                    return;
                }
            };

            if let Ok(text) = msg.to_str() {
                match process_message(text, users.clone(), my_id.clone()).await {
                    Ok(_) => {},
                    Err(e) => {
                        // Log the error with detailed information
                        eprintln!("{}", e.log_details());
                        
                        // Try to send error back to client with structured information
                        if let Some(user) = users.read().await.get(&my_id) {
                            let (user_msg, code) = e.user_message();
                            
                            let error_msg = serde_json::json!({
                                "message_type": "error",
                                "error": user_msg,
                                "code": code,
                                "data": "Error processing your request"
                            }).to_string();
                            
                            if let Err(send_err) = user.tx.send(Message::text(error_msg)) {
                                eprintln!("Failed to send error message: {}", send_err);
                            }
                        }
                    }
                }
            } else {
                // Handle binary or other non-text messages
                eprintln!("Received non-text message from {}", my_id);
                
                // Notify client about unsupported message format
                if let Some(user) = users.read().await.get(&my_id) {
                    let error_msg = serde_json::json!({
                        "message_type": "error",
                        "error": "Only text messages are supported",
                        "code": 1002,
                        "data": "Please send text messages only"
                    }).to_string();
                    
                    let _ = user.tx.send(Message::text(error_msg));
                }
            }
        }
    });

    // Convert the receiver stream to a stream of Results
    let receive_from_others = async {
        let mut rx_stream = tokio_stream::wrappers::UnboundedReceiverStream::new(user_rx);
        while let Some(msg) = rx_stream.next().await {
            if let Err(e) = ws_tx.send(msg).await {
                eprintln!("Error sending message to WebSocket: {}", e);
                break;
            }
        }
    };

    tokio::select! {
        _ = forward_messages => {},
        _ = receive_from_others => {},
    }

    // Clean up when user disconnects
    eprintln!("User {} disconnected", my_id);
    users.write().await.remove(&my_id);
    waiting.write().await.retain(|id| id != &my_id);
    eprintln!("Remaining users: {}", users.read().await.len());
}

async fn process_message(text: &str, users: Users, sender_id: String) -> Result<(), AppError> {
    // Check message size to prevent DoS attacks
    if text.len() > 16384 { // 16KB limit
        return Err(AppError::MessageTooLarge(format!("Message size {} exceeds limit", text.len())));
    }
    
    // Parse the incoming message
    let mut rtc_msg: WebRTCMessage = serde_json::from_str(text)
        .map_err(|e| AppError::ParseError(format!("Invalid message format: {}", e)))?;
    
    // Validate message type
    match rtc_msg.message_type.as_str() {
        "offer" | "answer" | "ice-candidate" | "chat" => {}, // Valid types
        _ => return Err(AppError::InvalidMessageType(format!("Unsupported message type: {}", rtc_msg.message_type))),
    }
    
    // Add the source ID to the message
    rtc_msg.source = Some(sender_id.clone());
    
    // Find the target user
    let users_read = users.read().await;
    let target_user = users_read.get(&rtc_msg.target)
        .ok_or_else(|| AppError::TargetNotFound(format!("User {} not found", rtc_msg.target)))?;
    
    // Forward the message to the target
    let forwarded_msg = serde_json::to_string(&rtc_msg)
        .map_err(|e| AppError::InternalError(format!("Failed to serialize message: {}", e)))?;
    
    // Log the message type for debugging
    eprintln!("Forwarding {} message from {} to {}", rtc_msg.message_type, sender_id, rtc_msg.target);
    
    target_user.tx.send(Message::text(forwarded_msg))
        .map_err(|e| AppError::ConnectionError(format!("Failed to send message: {}", e)))?;
    
    Ok(())
}

// Handle rejections (errors) from warp
async fn handle_rejection(err: Rejection) -> Result<impl Reply, Rejection> {
    let (error_message, status_code, error_code, details) = if let Some(app_error) = err.find::<AppError>() {
        // Get user-friendly message and code from our AppError
        let (msg, code) = app_error.user_message();
        
        // Log detailed error information for debugging
        eprintln!("API Error: {}", app_error.log_details());
        
        // Determine HTTP status code based on error type
        let status = match app_error {
            AppError::TargetNotFound(_) => warp::http::StatusCode::NOT_FOUND,
            AppError::ParseError(_) | AppError::InvalidMessageType(_) => warp::http::StatusCode::BAD_REQUEST,
            AppError::AuthenticationError(_) | AppError::PermissionDenied(_) => warp::http::StatusCode::FORBIDDEN,
            AppError::RateLimitExceeded(_) => warp::http::StatusCode::TOO_MANY_REQUESTS,
            _ => warp::http::StatusCode::INTERNAL_SERVER_ERROR,
        };
        
        // In development mode, we might want to include detailed error info
        #[cfg(debug_assertions)]
        let details = Some(app_error.log_details());
        
        #[cfg(not(debug_assertions))]
        let details = None;
        
        (msg.to_string(), status, code, details)
    } else if err.is_not_found() {
        ("Resource not found".to_string(), warp::http::StatusCode::NOT_FOUND, 1404, None)
    } else if err.find::<warp::filters::body::BodyDeserializeError>().is_some() {
        ("Invalid request data".to_string(), warp::http::StatusCode::BAD_REQUEST, 1400, None)
    } else if err.find::<warp::reject::MethodNotAllowed>().is_some() {
        ("Method not allowed".to_string(), warp::http::StatusCode::METHOD_NOT_ALLOWED, 1405, None)
    } else {
        // Unexpected error - log it for debugging
        eprintln!("Unhandled rejection: {:?}", err);
        ("Internal Server Error".to_string(), warp::http::StatusCode::INTERNAL_SERVER_ERROR, 1500, None)
    };

    let json = warp::reply::json(&ErrorMessage {
        error: error_message,
        code: error_code,
        details,
    });

    Ok(warp::reply::with_status(json, status_code))
}

// Generate time-limited TURN credentials
fn generate_turn_credentials() -> (String, String) {
    // Default username is just a random string
    let username = Uuid::new_v4().to_string();
    
    // For a simple implementation, we'll use a timestamp-based credential
    // In production, you would use a more secure method like HMAC
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
    let expiry = now + 86400; // Valid for 24 hours
    let credential = expiry.to_string();
    
    (username, credential)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize shared state
    let users = Users::default();
    let waiting = WaitingUsers::default();
    
    // Configure ICE servers (STUN and TURN)
    let (turn_username, turn_credential) = generate_turn_credentials();
    
    // Create ICE server configuration with multiple servers for reliability
    let mut ice_servers_vec = vec![
        // Google's public STUN server
        IceServer {
            urls: vec!["stun:stun.l.google.com:19302".to_string()],
            username: None,
            credential: None,
        },
        // Additional Google STUN servers for redundancy
        IceServer {
            urls: vec!["stun:stun1.l.google.com:19302".to_string()],
            username: None,
            credential: None,
        },
    ];
    
    // Add TURN server from environment variables if available
    // This allows for secure configuration in production without hardcoding
    if let Ok(turn_server) = env::var("TURN_SERVER") {
        let turn_username = env::var("TURN_USERNAME").unwrap_or_else(|_| turn_username.clone());
        let turn_credential = env::var("TURN_CREDENTIAL").unwrap_or_else(|_| turn_credential.clone());
        
        println!("Using TURN server from environment: {}", turn_server);
        
        // Parse multiple TURN URLs if provided as comma-separated list
        let turn_urls = turn_server.split(',').map(|url| url.trim().to_string()).collect::<Vec<String>>();
        
        ice_servers_vec.push(IceServer {
            urls: turn_urls,
            username: Some(turn_username),
            credential: Some(turn_credential),
        });
    } else {
        // Fallback to example TURN server if environment variables not set
        println!("No TURN server specified in environment, using example configuration");
        println!("For production, set TURN_SERVER, TURN_USERNAME, and TURN_CREDENTIAL environment variables");
        
        ice_servers_vec.push(IceServer {
            urls: vec![
                "turn:turn.example.com:3478?transport=udp".to_string(),
                "turn:turn.example.com:3478?transport=tcp".to_string(),
                "turns:turn.example.com:5349?transport=tcp".to_string(), // Secure TURN over TLS
            ],
            username: Some(turn_username),
            credential: Some(turn_credential),
        });
    }
    
    let ice_servers = Arc::new(ice_servers_vec);
    
    // Log server startup
    println!("Initializing WebRTC signaling server with TURN support...");

    // Create filters with shared state
    let users = warp::any().map(move || users.clone());
    let waiting = warp::any().map(move || waiting.clone());
    let ice_servers_filter = warp::any().map(move || ice_servers.clone());

    // WebSocket route with error handling
    let websocket = warp::path("ws")
        .and(warp::ws())
        .and(users.clone())
        .and(waiting.clone())
        .and(ice_servers_filter.clone())
        .map(|ws: warp::ws::Ws, users, waiting, ice_servers| {
            ws.on_upgrade(move |socket| {
                let my_id = Uuid::new_v4().to_string();
                println!("New WebSocket connection: {}", my_id);
                handle_websocket(socket, users, waiting, ice_servers, my_id)
            })
        });

    // Verify static directory exists
    if !std::path::Path::new("static").exists() {
        eprintln!("Error: 'static' directory not found. Please create it and add index.html");
        return Err("Static directory missing".into());
    }

    if !std::path::Path::new("static/index.html").exists() {
        eprintln!("Error: 'static/index.html' not found. Please create it");
        return Err("Index.html file missing".into());
    }

    // Static files route
    let static_files = warp::path::end()
        .and(warp::fs::file("static/index.html"))
        .or(warp::path("static").and(warp::fs::dir("static")));

    // Health check endpoint
    let health = warp::path("health")
        .map(|| warp::reply::json(&serde_json::json!({
            "status": "ok",
            "version": env!("CARGO_PKG_VERSION", "0.1.0"),
            "uptime": "just started"
        })));

    // Combine all routes
    let routes = websocket
        .or(static_files)
        .or(health)
        .with(warp::cors().allow_any_origin())
        .recover(handle_rejection);

    // Start the server
    let addr = ([0, 0, 0, 0], 5001);
    println!("✅ WebRTC signaling server starting on http://localhost:{}", addr.1);
    println!("📝 API endpoints:");
    println!("   - WebSocket: ws://localhost:{}/ws", addr.1);
    println!("   - Health check: http://localhost:{}/health", addr.1);
    println!("   - Web client: http://localhost:{}", addr.1);
    
    // Run the server
    warp::serve(routes).run(addr).await;
    
    Ok(())
}
