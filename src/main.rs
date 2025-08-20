use bcrypt::{DEFAULT_COST, hash, verify};
use chrono::{DateTime, Utc};
use futures::{SinkExt, StreamExt};
use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Header, Validation, decode, encode};
use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool};
use std::collections::HashMap;
use std::collections::VecDeque;
use std::env;
use std::error::Error as StdError;
use std::fmt;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock;
use uuid::Uuid;
use sha2::{Sha256, Digest};
use base64::{Engine as _, engine::general_purpose};
use rand::Rng;
use warp::Filter; // Required for using warp filters
use warp::Rejection;
use warp::Reply;
use warp::reject::Reject;
use warp::ws::{Message, WebSocket};

type Users = Arc<RwLock<HashMap<String, User>>>;
type WaitingUsers = Arc<RwLock<Vec<String>>>;
type IceServers = Arc<Vec<IceServer>>;
type Database = Arc<SqlitePool>;

#[derive(Debug)]
#[allow(dead_code)]
struct User {
    id: String,
    tx: tokio::sync::mpsc::UnboundedSender<Message>,
    status: UserStatus,
    connected_at: std::time::SystemTime,
    message_count: u32,
    last_message_time: std::time::SystemTime,
    message_timestamps: VecDeque<std::time::SystemTime>,
}

#[derive(Serialize, Deserialize)]
struct WebRTCMessage {
    target: String,
    message_type: String,
    data: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    source: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct ChatMessage {
    message_type: String,
    content: String,
    timestamp: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    source: Option<String>,
}

#[derive(Debug, Clone)]
enum UserStatus {
    Connected,
    Waiting,
    InCall,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DbUser {
    id: String,
    username: String,
    email: String,
    display_name: Option<String>,
    created_at: DateTime<Utc>,
    last_login: Option<DateTime<Utc>>,
    is_active: bool,
    is_admin: bool,
    preferences: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct UserRegistration {
    username: String,
    email: String,
    password: String,
    display_name: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct UserLogin {
    username: String,
    password: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct Claims {
    sub: String, // user id
    username: String,
    exp: usize, // expiration time
    iat: usize, // issued at
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Room {
    id: String,
    name: Option<String>,
    room_type: String,
    invite_code: Option<String>,
    created_by: Option<String>,
    created_at: DateTime<Utc>,
    max_participants: i32,
    is_active: bool,
}

#[derive(Debug, Serialize, Deserialize)]
struct UserReport {
    reporter_id: String,
    reported_user_id: String,
    report_type: String,
    description: Option<String>,
    room_id: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct CreateRoomRequest {
    name: Option<String>,
    room_type: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct JoinRoomRequest {
    invite_code: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct AdminStats {
    total_users: i64,
    active_users: i64,
    total_reports: i64,
    pending_reports: i64,
    active_rooms: i64,
}

#[derive(Debug, Serialize, Deserialize)]
struct ReportUpdateRequest {
    status: String,
    admin_notes: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct CaptchaRequest {
    token: String,
    action: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct CaptchaResponse {
    success: bool,
    score: Option<f64>,
    action: Option<String>,
    challenge_ts: Option<String>,
    hostname: Option<String>,
    error_codes: Option<Vec<String>>,
}

#[derive(Debug, Clone)]
struct SecurityConfig {
    rate_limits: HashMap<String, RateLimit>,
    captcha_secret: Option<String>,
    encryption_key: [u8; 32],
}

#[derive(Debug, Clone)]
struct RateLimit {
    requests_per_window: u32,
    window_seconds: u64,
}

#[derive(Debug, Clone)]
struct RateLimitState {
    requests: VecDeque<SystemTime>,
    blocked_until: Option<SystemTime>,
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

// Admin route handlers
async fn verify_admin_auth(auth_header: String, db: &Database) -> Result<DbUser, warp::Rejection> {
    let token = auth_header.strip_prefix("Bearer ").ok_or_else(|| {
        warp::reject::custom(AppError::AuthenticationError(
            "Invalid authorization header".to_string(),
        ))
    })?;

    let claims = verify_jwt_token(token)?;

    match get_user_by_id(db, &claims.sub).await? {
        Some(user) => {
            if !user.is_admin {
                return Err(warp::reject::custom(AppError::PermissionDenied(
                    "Admin privileges required".to_string(),
                )));
            }
            Ok(user)
        }
        None => Err(warp::reject::custom(AppError::AuthenticationError(
            "User not found".to_string(),
        ))),
    }
}

async fn handle_admin_stats(
    auth_header: String,
    db: Database,
) -> Result<impl warp::Reply, warp::Rejection> {
    let _admin = verify_admin_auth(auth_header, &db).await?;

    // Get statistics from database
    let total_users = sqlx::query("SELECT COUNT(*) as count FROM users")
        .fetch_one(&*db)
        .await
        .map_err(|e| warp::reject::custom(AppError::InternalError(format!("Database error: {}", e))))?
        .get::<i64, _>("count");

    let active_users = sqlx::query("SELECT COUNT(*) as count FROM users WHERE last_login > datetime('now', '-1 day')")
        .fetch_one(&*db)
        .await
        .map_err(|e| warp::reject::custom(AppError::InternalError(format!("Database error: {}", e))))?
        .get::<i64, _>("count");

    let total_reports = sqlx::query("SELECT COUNT(*) as count FROM user_reports")
        .fetch_one(&*db)
        .await
        .map_err(|e| warp::reject::custom(AppError::InternalError(format!("Database error: {}", e))))?
        .get::<i64, _>("count");

    let pending_reports = sqlx::query("SELECT COUNT(*) as count FROM user_reports WHERE status = 'pending'")
        .fetch_one(&*db)
        .await
        .map_err(|e| warp::reject::custom(AppError::InternalError(format!("Database error: {}", e))))?
        .get::<i64, _>("count");

    let active_rooms = sqlx::query("SELECT COUNT(*) as count FROM rooms WHERE is_active = TRUE")
        .fetch_one(&*db)
        .await
        .map_err(|e| warp::reject::custom(AppError::InternalError(format!("Database error: {}", e))))?
        .get::<i64, _>("count");

    let stats = AdminStats {
        total_users,
        active_users,
        total_reports,
        pending_reports,
        active_rooms,
    };

    let response = serde_json::json!({
        "success": true,
        "stats": stats
    });
    Ok(warp::reply::json(&response))
}

async fn handle_admin_users(
    auth_header: String,
    db: Database,
) -> Result<impl warp::Reply, warp::Rejection> {
    let _admin = verify_admin_auth(auth_header, &db).await?;

    let users = sqlx::query("SELECT id, username, email, display_name, created_at, last_login, is_active, is_admin FROM users ORDER BY created_at DESC LIMIT 100")
        .fetch_all(&*db)
        .await
        .map_err(|e| warp::reject::custom(AppError::InternalError(format!("Database error: {}", e))))?;

    let user_list: Vec<serde_json::Value> = users
        .iter()
        .map(|u| {
            serde_json::json!({
                "id": u.get::<String, _>("id"),
                "username": u.get::<String, _>("username"),
                "email": u.get::<String, _>("email"),
                "display_name": u.get::<Option<String>, _>("display_name"),
                "created_at": u.get::<DateTime<Utc>, _>("created_at"),
                "last_login": u.get::<Option<DateTime<Utc>>, _>("last_login"),
                "is_active": u.get::<bool, _>("is_active"),
                "is_admin": u.get::<bool, _>("is_admin")
            })
        })
        .collect();

    let response = serde_json::json!({
        "success": true,
        "users": user_list
    });
    Ok(warp::reply::json(&response))
}

async fn handle_admin_reports(
    auth_header: String,
    db: Database,
) -> Result<impl warp::Reply, warp::Rejection> {
    let _admin = verify_admin_auth(auth_header, &db).await?;

    let reports = sqlx::query(
        "SELECT r.id, r.reporter_id, r.reported_user_id, r.report_type, r.description, 
                r.created_at, r.status, r.admin_notes,
                reporter.username as reporter_username,
                reported.username as reported_username
         FROM user_reports r
         LEFT JOIN users reporter ON r.reporter_id = reporter.id
         LEFT JOIN users reported ON r.reported_user_id = reported.id
         ORDER BY r.created_at DESC LIMIT 100"
    )
    .fetch_all(&*db)
    .await
    .map_err(|e| warp::reject::custom(AppError::InternalError(format!("Database error: {}", e))))?;

    let report_list: Vec<serde_json::Value> = reports
        .iter()
        .map(|r| {
            serde_json::json!({
                "id": r.get::<String, _>("id"),
                "reporter_id": r.get::<String, _>("reporter_id"),
                "reported_user_id": r.get::<String, _>("reported_user_id"),
                "reporter_username": r.get::<Option<String>, _>("reporter_username").unwrap_or_else(|| "Unknown".to_string()),
                "reported_username": r.get::<Option<String>, _>("reported_username").unwrap_or_else(|| "Unknown".to_string()),
                "report_type": r.get::<String, _>("report_type"),
                "description": r.get::<Option<String>, _>("description"),
                "created_at": r.get::<DateTime<Utc>, _>("created_at"),
                "status": r.get::<String, _>("status"),
                "admin_notes": r.get::<Option<String>, _>("admin_notes")
            })
        })
        .collect();

    let response = serde_json::json!({
        "success": true,
        "reports": report_list
    });
    Ok(warp::reply::json(&response))
}

async fn handle_admin_update_report(
    report_id: String,
    update_request: ReportUpdateRequest,
    auth_header: String,
    db: Database,
) -> Result<impl warp::Reply, warp::Rejection> {
    let admin = verify_admin_auth(auth_header, &db).await?;

    sqlx::query(
        "UPDATE user_reports SET status = ?, admin_notes = ?, reviewed_by = ?, reviewed_at = ? WHERE id = ?"
    )
    .bind(&update_request.status)
    .bind(&update_request.admin_notes)
    .bind(&admin.id)
    .bind(Utc::now())
    .bind(&report_id)
    .execute(&*db)
    .await
    .map_err(|e| warp::reject::custom(AppError::InternalError(format!("Database error: {}", e))))?;

    let response = serde_json::json!({
        "success": true,
        "message": "Report updated successfully"
    });
    Ok(warp::reply::json(&response))
}

async fn handle_admin_rooms(
    auth_header: String,
    db: Database,
) -> Result<impl warp::Reply, warp::Rejection> {
    let _admin = verify_admin_auth(auth_header, &db).await?;

    let rooms = sqlx::query(
        "SELECT r.id, r.name, r.room_type, r.invite_code, r.created_at, r.max_participants, r.is_active,
                creator.username as creator_username,
                COUNT(rp.user_id) as participant_count
         FROM rooms r
         LEFT JOIN users creator ON r.created_by = creator.id
         LEFT JOIN room_participants rp ON r.id = rp.room_id AND rp.left_at IS NULL
         GROUP BY r.id
         ORDER BY r.created_at DESC LIMIT 100"
    )
    .fetch_all(&*db)
    .await
    .map_err(|e| warp::reject::custom(AppError::InternalError(format!("Database error: {}", e))))?;

    let room_list: Vec<serde_json::Value> = rooms
        .iter()
        .map(|r| {
            serde_json::json!({
                "id": r.get::<String, _>("id"),
                "name": r.get::<Option<String>, _>("name"),
                "room_type": r.get::<String, _>("room_type"),
                "invite_code": r.get::<Option<String>, _>("invite_code"),
                "created_at": r.get::<DateTime<Utc>, _>("created_at"),
                "max_participants": r.get::<i32, _>("max_participants"),
                "is_active": r.get::<bool, _>("is_active"),
                "creator_username": r.get::<Option<String>, _>("creator_username").unwrap_or_else(|| "Unknown".to_string()),
                "participant_count": r.get::<i64, _>("participant_count")
            })
        })
        .collect();

    let response = serde_json::json!({
        "success": true,
        "rooms": room_list
    });
    Ok(warp::reply::json(&response))
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

async fn handle_websocket(
    ws: WebSocket,
    users: Users,
    waiting: WaitingUsers,
    ice_servers: IceServers,
    my_id: String,
) {
    let (user_tx, user_rx) = tokio::sync::mpsc::unbounded_channel();
    let (mut ws_tx, ws_rx) = ws.split();

    // Send client their ID and ICE server configuration
    let connect_msg = serde_json::json!({
        "message_type": "connected",
        "data": my_id.clone(),
        "ice_servers": ice_servers.as_ref(),
    })
    .to_string();

    if let Err(e) = ws_tx.send(Message::text(connect_msg)).await {
        eprintln!("Error sending connection message: {}", e);
        return;
    }

    // Add user to active users map
    users.write().await.insert(
        my_id.clone(),
        User {
            id: my_id.clone(),
            tx: user_tx,
            status: UserStatus::Connected,
            connected_at: std::time::SystemTime::now(),
            message_count: 0,
            last_message_time: std::time::SystemTime::now(),
            message_timestamps: VecDeque::new(),
        },
    );

    // Log connection
    eprintln!(
        "User {} connected, total users: {}",
        my_id,
        users.read().await.len()
    );

    let users_clone = users.clone();
    let waiting_clone = waiting.clone();
    let my_id_clone = my_id.clone();
    let forward_messages = ws_rx.for_each(|msg| {
        let users = users_clone.clone();
        let waiting = waiting_clone.clone();
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
                match process_message(text, users.clone(), waiting.clone(), my_id.clone()).await {
                    Ok(_) => {}
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
                            })
                            .to_string();

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
                    })
                    .to_string();

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

async fn process_message(
    text: &str,
    users: Users,
    waiting: WaitingUsers,
    sender_id: String,
) -> Result<(), AppError> {
    // Check message size to prevent DoS attacks
    if text.len() > 16384 {
        // 16KB limit
        return Err(AppError::MessageTooLarge(format!(
            "Message size {} exceeds limit",
            text.len()
        )));
    }

    // Rate limiting check
    {
        let mut users_map = users.write().await;
        if let Some(user) = users_map.get_mut(&sender_id) {
            let now = std::time::SystemTime::now();

            // Clean up old timestamps (older than 1 minute)
            while let Some(&front_time) = user.message_timestamps.front() {
                if now.duration_since(front_time).unwrap_or_default().as_secs() > 60 {
                    user.message_timestamps.pop_front();
                } else {
                    break;
                }
            }

            // Check if user has exceeded rate limit (20 messages per minute)
            if user.message_timestamps.len() >= 20 {
                return Err(AppError::RateLimitExceeded(format!(
                    "Too many messages from user {}",
                    sender_id
                )));
            }

            // Add current timestamp
            user.message_timestamps.push_back(now);
            user.message_count += 1;
            user.last_message_time = now;
        }
    }

    // Parse the incoming message
    let mut rtc_msg: WebRTCMessage = serde_json::from_str(text)
        .map_err(|e| AppError::ParseError(format!("Invalid message format: {}", e)))?;

    // Validate message type
    match rtc_msg.message_type.as_str() {
        "offer" | "answer" | "ice-candidate" | "chat" | "find-partner" | "partner-found"
        | "partner-left" => {} // Valid types
        _ => {
            return Err(AppError::InvalidMessageType(format!(
                "Unsupported message type: {}",
                rtc_msg.message_type
            )));
        }
    }

    // Handle special messages that don't need a target
    if rtc_msg.message_type == "find-partner" {
        return handle_find_partner(users, waiting, sender_id).await;
    }

    // Add the source ID to the message
    rtc_msg.source = Some(sender_id.clone());

    // Special handling for chat messages - add timestamp and content filtering
    if rtc_msg.message_type == "chat" {
        // Content filtering for inappropriate content
        if contains_inappropriate_content(&rtc_msg.data) {
            return Err(AppError::PermissionDenied(
                "Message contains inappropriate content".to_string(),
            ));
        }

        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let chat_msg = ChatMessage {
            message_type: "chat".to_string(),
            content: rtc_msg.data.clone(),
            timestamp,
            source: Some(sender_id.clone()),
        };

        let users_read = users.read().await;
        let target_user = users_read.get(&rtc_msg.target).ok_or_else(|| {
            AppError::TargetNotFound(format!("User {} not found", rtc_msg.target))
        })?;

        let chat_msg_str = serde_json::to_string(&chat_msg).map_err(|e| {
            AppError::InternalError(format!("Failed to serialize chat message: {}", e))
        })?;

        eprintln!(
            "Forwarding chat message from {} to {}",
            sender_id, rtc_msg.target
        );

        target_user
            .tx
            .send(Message::text(chat_msg_str))
            .map_err(|e| {
                AppError::ConnectionError(format!("Failed to send chat message: {}", e))
            })?;

        return Ok(());
    }

    // Find the target user
    let users_read = users.read().await;
    let target_user = users_read
        .get(&rtc_msg.target)
        .ok_or_else(|| AppError::TargetNotFound(format!("User {} not found", rtc_msg.target)))?;

    // Forward the message to the target
    let forwarded_msg = serde_json::to_string(&rtc_msg)
        .map_err(|e| AppError::InternalError(format!("Failed to serialize message: {}", e)))?;

    // Log the message type for debugging
    eprintln!(
        "Forwarding {} message from {} to {}",
        rtc_msg.message_type, sender_id, rtc_msg.target
    );

    target_user
        .tx
        .send(Message::text(forwarded_msg))
        .map_err(|e| AppError::ConnectionError(format!("Failed to send message: {}", e)))?;

    Ok(())
}

async fn handle_find_partner(
    users: Users,
    waiting: WaitingUsers,
    user_id: String,
) -> Result<(), AppError> {
    let mut waiting_users = waiting.write().await;
    let mut users_map = users.write().await;

    // Update user status to waiting
    if let Some(user) = users_map.get_mut(&user_id) {
        user.status = UserStatus::Waiting;
    }

    // Check if there's someone already waiting
    if let Some(partner_id) = waiting_users.pop() {
        if partner_id != user_id {
            // Found a partner! Update both users' status
            if let Some(user) = users_map.get_mut(&user_id) {
                user.status = UserStatus::InCall;
            }
            if let Some(partner) = users_map.get_mut(&partner_id) {
                partner.status = UserStatus::InCall;
            }

            // Notify both users they found a partner
            let partner_found_msg = serde_json::json!({
                "message_type": "partner-found",
                "partner_id": partner_id,
                "data": "Partner found! Starting call..."
            })
            .to_string();

            let user_found_msg = serde_json::json!({
                "message_type": "partner-found",
                "partner_id": user_id,
                "data": "Partner found! Starting call..."
            })
            .to_string();

            // Send to current user
            if let Some(user) = users_map.get(&user_id) {
                let _ = user.tx.send(Message::text(partner_found_msg));
            }

            // Send to partner
            if let Some(partner) = users_map.get(&partner_id) {
                let _ = partner.tx.send(Message::text(user_found_msg));
            }

            eprintln!("Matched users {} and {}", user_id, partner_id);
        } else {
            // Same user, add back to queue
            waiting_users.push(user_id);
        }
    } else {
        // No one waiting, add this user to queue
        waiting_users.push(user_id.clone());

        // Send waiting message
        if let Some(user) = users_map.get(&user_id) {
            let waiting_msg = serde_json::json!({
                "message_type": "waiting",
                "data": format!("Looking for a partner... {} users in queue", waiting_users.len())
            })
            .to_string();

            let _ = user.tx.send(Message::text(waiting_msg));
        }

        eprintln!(
            "User {} added to waiting queue. Queue size: {}",
            user_id,
            waiting_users.len()
        );
    }

    Ok(())
}

// Handle rejections (errors) from warp
async fn handle_rejection(err: Rejection) -> Result<impl Reply, Rejection> {
    let (error_message, status_code, error_code, details) =
        if let Some(app_error) = err.find::<AppError>() {
            // Get user-friendly message and code from our AppError
            let (msg, code) = app_error.user_message();

            // Log detailed error information for debugging
            eprintln!("API Error: {}", app_error.log_details());

            // Determine HTTP status code based on error type
            let status = match app_error {
                AppError::TargetNotFound(_) => warp::http::StatusCode::NOT_FOUND,
                AppError::ParseError(_) | AppError::InvalidMessageType(_) => {
                    warp::http::StatusCode::BAD_REQUEST
                }
                AppError::AuthenticationError(_) | AppError::PermissionDenied(_) => {
                    warp::http::StatusCode::FORBIDDEN
                }
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
            (
                "Resource not found".to_string(),
                warp::http::StatusCode::NOT_FOUND,
                1404,
                None,
            )
        } else if err
            .find::<warp::filters::body::BodyDeserializeError>()
            .is_some()
        {
            (
                "Invalid request data".to_string(),
                warp::http::StatusCode::BAD_REQUEST,
                1400,
                None,
            )
        } else if err.find::<warp::reject::MethodNotAllowed>().is_some() {
            (
                "Method not allowed".to_string(),
                warp::http::StatusCode::METHOD_NOT_ALLOWED,
                1405,
                None,
            )
        } else {
            // Unexpected error - log it for debugging
            eprintln!("Unhandled rejection: {:?}", err);
            (
                "Internal Server Error".to_string(),
                warp::http::StatusCode::INTERNAL_SERVER_ERROR,
                1500,
                None,
            )
        };

    let json = warp::reply::json(&ErrorMessage {
        error: error_message,
        code: error_code,
        details,
    });

    Ok(warp::reply::with_status(json, status_code))
}

fn contains_inappropriate_content(message: &str) -> bool {
    let message_lower = message.to_lowercase();
    let inappropriate_words = [
        // Basic profanity filter - in production, use a comprehensive service
        "spam",
        "scam",
        "viagra",
        "casino",
        "bitcoin",
        "crypto",
        "investment",
        // Add more words as needed - this is a minimal example
    ];

    for word in inappropriate_words.iter() {
        if message_lower.contains(word) {
            return true;
        }
    }

    // Check for excessive caps (> 80% uppercase)
    let caps_count = message.chars().filter(|c| c.is_uppercase()).count();
    let total_letters = message.chars().filter(|c| c.is_alphabetic()).count();

    if total_letters > 5 && caps_count as f32 / total_letters as f32 > 0.8 {
        return true;
    }

    // Check for excessive repeated characters
    let mut prev_char = '\0';
    let mut repeat_count = 1;
    for ch in message.chars() {
        if ch == prev_char {
            repeat_count += 1;
            if repeat_count > 4 {
                // More than 4 repeated characters
                return true;
            }
        } else {
            repeat_count = 1;
        }
        prev_char = ch;
    }

    false
}

// Database functions
async fn init_database() -> Result<SqlitePool, sqlx::Error> {
    let database_url = env::var("DATABASE_URL").unwrap_or_else(|_| "sqlite:chat.db".to_string());
    let pool = SqlitePool::connect(&database_url).await?;

    // Create tables directly since we don't have migrations set up
    // sqlx::migrate!("./migrations").run(&pool).await.unwrap_or_else(|e| {
    //     eprintln!("Failed to run migrations, creating tables manually: {}", e);
    // });

    // Create tables if they don't exist
    let schema = include_str!("../schema.sql");
    for statement in schema.split(';') {
        let statement = statement.trim();
        if !statement.is_empty() {
            sqlx::query(statement)
                .execute(&pool)
                .await
                .unwrap_or_else(|e| {
                    eprintln!("Failed to execute schema statement: {}", e);
                    std::process::exit(1);
                });
        }
    }

    Ok(pool)
}

async fn create_user(
    pool: &SqlitePool,
    registration: UserRegistration,
) -> Result<DbUser, AppError> {
    // Check if username or email already exists
    let existing = sqlx::query("SELECT id FROM users WHERE username = ? OR email = ?")
        .bind(&registration.username)
        .bind(&registration.email)
        .fetch_optional(pool)
        .await
        .map_err(|e| AppError::InternalError(format!("Database error: {}", e)))?;

    if existing.is_some() {
        return Err(AppError::AuthenticationError(
            "Username or email already exists".to_string(),
        ));
    }

    // Hash password
    let password_hash = hash(registration.password, DEFAULT_COST)
        .map_err(|e| AppError::InternalError(format!("Password hashing error: {}", e)))?;

    let user_id = Uuid::new_v4().to_string();
    let now = Utc::now();

    // Insert user
    sqlx::query(
        r#"
        INSERT INTO users (id, username, email, password_hash, display_name, created_at, is_active, is_admin)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?)
        "#
    )
    .bind(&user_id)
    .bind(&registration.username)
    .bind(&registration.email)
    .bind(&password_hash)
    .bind(&registration.display_name)
    .bind(now)
    .bind(true)
    .bind(false)
    .execute(pool)
    .await
    .map_err(|e| AppError::InternalError(format!("Failed to create user: {}", e)))?;

    Ok(DbUser {
        id: user_id,
        username: registration.username,
        email: registration.email,
        display_name: registration.display_name,
        created_at: now,
        last_login: None,
        is_active: true,
        is_admin: false,
        preferences: None,
    })
}

async fn authenticate_user(pool: &SqlitePool, login: UserLogin) -> Result<DbUser, AppError> {
    let user = sqlx::query("SELECT id, username, email, password_hash, display_name, created_at, last_login, is_active, is_admin, preferences FROM users WHERE username = ? AND is_active = TRUE")
        .bind(&login.username)
        .fetch_optional(pool)
        .await
        .map_err(|e| AppError::InternalError(format!("Database error: {}", e)))?;

    let user = user
        .ok_or_else(|| AppError::AuthenticationError("Invalid username or password".to_string()))?;

    // Extract values from row
    let user_id: String = user.get("id");
    let username: String = user.get("username");
    let email: String = user.get("email");
    let password_hash: String = user.get("password_hash");
    let display_name: Option<String> = user.get("display_name");
    let created_at: DateTime<Utc> = user.get("created_at");
    let _last_login: Option<DateTime<Utc>> = user.get("last_login");
    let is_active: bool = user.get("is_active");
    let is_admin: bool = user.get("is_admin");
    let preferences: Option<String> = user.get("preferences");

    // Verify password
    if !verify(login.password, &password_hash)
        .map_err(|e| AppError::InternalError(format!("Password verification error: {}", e)))?
    {
        return Err(AppError::AuthenticationError(
            "Invalid username or password".to_string(),
        ));
    }

    // Update last login
    let now = Utc::now();
    sqlx::query("UPDATE users SET last_login = ? WHERE id = ?")
        .bind(now)
        .bind(&user_id)
        .execute(pool)
        .await
        .map_err(|e| AppError::InternalError(format!("Failed to update last login: {}", e)))?;

    Ok(DbUser {
        id: user_id,
        username,
        email,
        display_name,
        created_at,
        last_login: Some(now),
        is_active,
        is_admin,
        preferences,
    })
}

async fn get_user_by_id(pool: &SqlitePool, user_id: &str) -> Result<Option<DbUser>, AppError> {
    let user = sqlx::query("SELECT id, username, email, display_name, created_at, last_login, is_active, is_admin, preferences FROM users WHERE id = ?")
        .bind(user_id)
        .fetch_optional(pool)
        .await
        .map_err(|e| AppError::InternalError(format!("Database error: {}", e)))?;

    Ok(user.map(|u| DbUser {
        id: u.get("id"),
        username: u.get("username"),
        email: u.get("email"),
        display_name: u.get("display_name"),
        created_at: u.get("created_at"),
        last_login: u.get("last_login"),
        is_active: u.get("is_active"),
        is_admin: u.get("is_admin"),
        preferences: u.get("preferences"),
    }))
}

fn generate_jwt_token(user: &DbUser) -> Result<String, AppError> {
    let secret = env::var("JWT_SECRET").unwrap_or_else(|_| "your-secret-key".to_string());
    let expiration = Utc::now()
        .checked_add_signed(chrono::Duration::days(7))
        .expect("valid timestamp")
        .timestamp() as usize;

    let claims = Claims {
        sub: user.id.clone(),
        username: user.username.clone(),
        exp: expiration,
        iat: Utc::now().timestamp() as usize,
    };

    encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(secret.as_ref()),
    )
    .map_err(|e| AppError::InternalError(format!("JWT generation error: {}", e)))
}

fn verify_jwt_token(token: &str) -> Result<Claims, AppError> {
    let secret = env::var("JWT_SECRET").unwrap_or_else(|_| "your-secret-key".to_string());
    let validation = Validation::new(Algorithm::HS256);

    decode::<Claims>(
        token,
        &DecodingKey::from_secret(secret.as_ref()),
        &validation,
    )
    .map(|data| data.claims)
    .map_err(|e| AppError::AuthenticationError(format!("Invalid token: {}", e)))
}

async fn create_room(
    pool: &SqlitePool,
    created_by: &str,
    room_type: &str,
    name: Option<String>,
) -> Result<Room, AppError> {
    let room_id = Uuid::new_v4().to_string();
    let invite_code = if room_type == "private" {
        Some(generate_invite_code())
    } else {
        None
    };
    let now = Utc::now();

    sqlx::query(
        "INSERT INTO rooms (id, name, room_type, invite_code, created_by, created_at, max_participants, is_active) VALUES (?, ?, ?, ?, ?, ?, ?, ?)"
    )
    .bind(&room_id)
    .bind(&name)
    .bind(room_type)
    .bind(&invite_code)
    .bind(created_by)
    .bind(now)
    .bind(2)
    .bind(true)
    .execute(pool)
    .await
    .map_err(|e| AppError::InternalError(format!("Failed to create room: {}", e)))?;

    Ok(Room {
        id: room_id,
        name,
        room_type: room_type.to_string(),
        invite_code,
        created_by: Some(created_by.to_string()),
        created_at: now,
        max_participants: 2,
        is_active: true,
    })
}

fn generate_invite_code() -> String {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    let code: String = (0..6).map(|_| rng.gen_range(0..10).to_string()).collect();
    code
}

async fn create_user_report(pool: &SqlitePool, report: UserReport) -> Result<(), AppError> {
    let report_id = Uuid::new_v4().to_string();

    sqlx::query(
        "INSERT INTO user_reports (id, reporter_id, reported_user_id, report_type, description, room_id, created_at, status) VALUES (?, ?, ?, ?, ?, ?, ?, ?)"
    )
    .bind(&report_id)
    .bind(&report.reporter_id)
    .bind(&report.reported_user_id)
    .bind(&report.report_type)
    .bind(&report.description)
    .bind(&report.room_id)
    .bind(Utc::now())
    .bind("pending")
    .execute(pool)
    .await
    .map_err(|e| AppError::InternalError(format!("Failed to create report: {}", e)))?;

    Ok(())
}

async fn find_room_by_invite_code(
    pool: &SqlitePool,
    invite_code: &str,
) -> Result<Option<Room>, AppError> {
    let room = sqlx::query("SELECT id, name, room_type, invite_code, created_by, created_at, max_participants, is_active FROM rooms WHERE invite_code = ? AND is_active = TRUE")
        .bind(invite_code)
        .fetch_optional(pool)
        .await
        .map_err(|e| AppError::InternalError(format!("Database error: {}", e)))?;

    Ok(room.map(|r| Room {
        id: r.get("id"),
        name: r.get("name"),
        room_type: r.get("room_type"),
        invite_code: r.get("invite_code"),
        created_by: r.get("created_by"),
        created_at: r.get("created_at"),
        max_participants: r.get("max_participants"),
        is_active: r.get("is_active"),
    }))
}

async fn join_room_participant(
    pool: &SqlitePool,
    room_id: &str,
    user_id: &str,
) -> Result<(), AppError> {
    // Check if user is already in room
    let existing = sqlx::query("SELECT room_id FROM room_participants WHERE room_id = ? AND user_id = ? AND left_at IS NULL")
        .bind(room_id)
        .bind(user_id)
        .fetch_optional(pool)
        .await
        .map_err(|e| AppError::InternalError(format!("Database error: {}", e)))?;

    if existing.is_some() {
        return Ok(()); // Already in room
    }

    // Add user to room
    sqlx::query(
        "INSERT INTO room_participants (room_id, user_id, joined_at, role) VALUES (?, ?, ?, ?)",
    )
    .bind(room_id)
    .bind(user_id)
    .bind(Utc::now())
    .bind("participant")
    .execute(pool)
    .await
    .map_err(|e| AppError::InternalError(format!("Failed to join room: {}", e)))?;

    Ok(())
}

#[allow(dead_code)]
async fn leave_room_participant(
    pool: &SqlitePool,
    room_id: &str,
    user_id: &str,
) -> Result<(), AppError> {
    sqlx::query("UPDATE room_participants SET left_at = ? WHERE room_id = ? AND user_id = ? AND left_at IS NULL")
        .bind(Utc::now())
        .bind(room_id)
        .bind(user_id)
        .execute(pool)
        .await
        .map_err(|e| AppError::InternalError(format!("Failed to leave room: {}", e)))?;

    Ok(())
}

// Generate time-limited TURN credentials
fn generate_turn_credentials() -> (String, String) {
    // Default username is just a random string
    let username = Uuid::new_v4().to_string();

    // For a simple implementation, we'll use a timestamp-based credential
    // In production, you would use a more secure method like HMAC
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let expiry = now + 86400; // Valid for 24 hours
    let credential = expiry.to_string();

    (username, credential)
}

// Security Functions
fn init_security_config() -> SecurityConfig {
    let mut rate_limits = HashMap::new();
    
    // Configure rate limits for different endpoints
    rate_limits.insert("login".to_string(), RateLimit {
        requests_per_window: 5,
        window_seconds: 300, // 5 requests per 5 minutes
    });
    
    rate_limits.insert("register".to_string(), RateLimit {
        requests_per_window: 3,
        window_seconds: 600, // 3 requests per 10 minutes
    });
    
    rate_limits.insert("websocket".to_string(), RateLimit {
        requests_per_window: 100,
        window_seconds: 60, // 100 messages per minute
    });
    
    rate_limits.insert("api".to_string(), RateLimit {
        requests_per_window: 60,
        window_seconds: 60, // 60 API calls per minute
    });
    
    // Generate or load encryption key
    let encryption_key = generate_encryption_key();
    
    SecurityConfig {
        rate_limits,
        captcha_secret: env::var("CAPTCHA_SECRET").ok(),
        encryption_key,
    }
}

fn generate_encryption_key() -> [u8; 32] {
    // In production, this should be loaded from environment or key management service
    if let Ok(key_str) = env::var("ENCRYPTION_KEY") {
        let key_bytes = general_purpose::STANDARD.decode(key_str)
            .unwrap_or_else(|_| {
                eprintln!("Invalid ENCRYPTION_KEY format, generating new key");
                generate_random_key().to_vec()
            });
        
        if key_bytes.len() == 32 {
            let mut key = [0u8; 32];
            key.copy_from_slice(&key_bytes);
            key
        } else {
            eprintln!("ENCRYPTION_KEY wrong length, generating new key");
            generate_random_key()
        }
    } else {
        let key = generate_random_key();
        let key_b64 = general_purpose::STANDARD.encode(&key);
        eprintln!("Generated new encryption key (save this): ENCRYPTION_KEY={}", key_b64);
        key
    }
}

fn generate_random_key() -> [u8; 32] {
    let mut key = [0u8; 32];
    let mut rng = rand::thread_rng();
    for byte in &mut key {
        *byte = rng.gen();
    }
    key
}

// Enhanced rate limiting per IP
type IpRateLimits = Arc<RwLock<HashMap<String, RateLimitState>>>;

async fn check_rate_limit(
    ip: &str,
    endpoint: &str,
    rate_limits: &IpRateLimits,
    config: &SecurityConfig,
) -> Result<(), AppError> {
    let rate_limit = config.rate_limits.get(endpoint)
        .unwrap_or(&RateLimit {
            requests_per_window: 60,
            window_seconds: 60,
        });
    
    let now = SystemTime::now();
    let window_start = now - std::time::Duration::from_secs(rate_limit.window_seconds);
    
    let mut limits = rate_limits.write().await;
    let ip_state = limits.entry(ip.to_string()).or_insert_with(|| RateLimitState {
        requests: VecDeque::new(),
        blocked_until: None,
    });
    
    // Check if still blocked
    if let Some(blocked_until) = ip_state.blocked_until {
        if now < blocked_until {
            return Err(AppError::RateLimitExceeded(
                "IP address is temporarily blocked".to_string()
            ));
        } else {
            ip_state.blocked_until = None;
        }
    }
    
    // Clean old requests
    while let Some(&front_time) = ip_state.requests.front() {
        if front_time < window_start {
            ip_state.requests.pop_front();
        } else {
            break;
        }
    }
    
    // Check rate limit
    if ip_state.requests.len() >= rate_limit.requests_per_window as usize {
        // Block IP for 2x window duration
        ip_state.blocked_until = Some(now + std::time::Duration::from_secs(rate_limit.window_seconds * 2));
        return Err(AppError::RateLimitExceeded(
            "Rate limit exceeded - IP temporarily blocked".to_string()
        ));
    }
    
    // Add current request
    ip_state.requests.push_back(now);
    
    Ok(())
}

#[allow(clippy::type_complexity)]
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize database
    let pool = init_database().await?;
    let db = Database::new(pool);

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
        let turn_credential =
            env::var("TURN_CREDENTIAL").unwrap_or_else(|_| turn_credential.clone());

        println!("Using TURN server from environment: {}", turn_server);

        // Parse multiple TURN URLs if provided as comma-separated list
        let turn_urls = turn_server
            .split(',')
            .map(|url| url.trim().to_string())
            .collect::<Vec<String>>();

        ice_servers_vec.push(IceServer {
            urls: turn_urls,
            username: Some(turn_username),
            credential: Some(turn_credential),
        });
    } else {
        // Fallback to example TURN server if environment variables not set
        println!("No TURN server specified in environment, using example configuration");
        println!(
            "For production, set TURN_SERVER, TURN_USERNAME, and TURN_CREDENTIAL environment variables"
        );

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
    let db_filter = warp::any().map(move || db.clone());

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
        .or(warp::path("login.html").and(warp::fs::file("static/login.html")))
        .or(warp::path("admin.html").and(warp::fs::file("static/admin.html")))
        .or(warp::path("static").and(warp::fs::dir("static")));

    // Authentication routes
    let register = warp::path("api")
        .and(warp::path("register"))
        .and(warp::post())
        .and(warp::body::json())
        .and(db_filter.clone())
        .and_then(handle_register);

    let login = warp::path("api")
        .and(warp::path("login"))
        .and(warp::post())
        .and(warp::body::json())
        .and(db_filter.clone())
        .and_then(handle_login);

    let profile = warp::path("api")
        .and(warp::path("profile"))
        .and(warp::get())
        .and(warp::header::<String>("authorization"))
        .and(db_filter.clone())
        .and_then(handle_profile);

    let report_user = warp::path("api")
        .and(warp::path("report"))
        .and(warp::post())
        .and(warp::body::json())
        .and(warp::header::<String>("authorization"))
        .and(db_filter.clone())
        .and_then(handle_report);

    let create_room = warp::path("api")
        .and(warp::path("rooms"))
        .and(warp::post())
        .and(warp::body::json())
        .and(warp::header::<String>("authorization"))
        .and(db_filter.clone())
        .and_then(handle_create_room);

    let join_room = warp::path("api")
        .and(warp::path("rooms"))
        .and(warp::path("join"))
        .and(warp::post())
        .and(warp::body::json())
        .and(warp::header::<String>("authorization"))
        .and(db_filter.clone())
        .and_then(handle_join_room);

    // Admin routes
    let admin_stats = warp::path("api")
        .and(warp::path("admin"))
        .and(warp::path("stats"))
        .and(warp::get())
        .and(warp::header::<String>("authorization"))
        .and(db_filter.clone())
        .and_then(handle_admin_stats);

    let admin_users = warp::path("api")
        .and(warp::path("admin"))
        .and(warp::path("users"))
        .and(warp::get())
        .and(warp::header::<String>("authorization"))
        .and(db_filter.clone())
        .and_then(handle_admin_users);

    let admin_reports = warp::path("api")
        .and(warp::path("admin"))
        .and(warp::path("reports"))
        .and(warp::get())
        .and(warp::header::<String>("authorization"))
        .and(db_filter.clone())
        .and_then(handle_admin_reports);

    let admin_update_report = warp::path("api")
        .and(warp::path("admin"))
        .and(warp::path("reports"))
        .and(warp::path::param::<String>())
        .and(warp::put())
        .and(warp::body::json())
        .and(warp::header::<String>("authorization"))
        .and(db_filter.clone())
        .and_then(handle_admin_update_report);

    let admin_rooms = warp::path("api")
        .and(warp::path("admin"))
        .and(warp::path("rooms"))
        .and(warp::get())
        .and(warp::header::<String>("authorization"))
        .and(db_filter.clone())
        .and_then(handle_admin_rooms);

    // Health check endpoint
    let health = warp::path("health").map(|| {
        warp::reply::json(&serde_json::json!({
            "status": "ok",
            "version": env!("CARGO_PKG_VERSION", "0.1.0"),
            "uptime": "just started"
        }))
    });

    // Combine all routes
    let routes = websocket
        .or(static_files)
        .or(register)
        .or(login)
        .or(profile)
        .or(report_user)
        .or(create_room)
        .or(join_room)
        .or(admin_stats)
        .or(admin_users)
        .or(admin_reports)
        .or(admin_update_report)
        .or(admin_rooms)
        .or(health)
        .with(
            warp::cors()
                .allow_any_origin()
                .allow_headers(vec!["authorization", "content-type"])
                .allow_methods(vec!["GET", "POST", "PUT", "DELETE"]),
        )
        .recover(handle_rejection);

    // Start the server
    let addr = ([0, 0, 0, 0], 5001);
    println!(
        "✅ WebRTC signaling server starting on http://localhost:{}",
        addr.1
    );
    println!("📝 API endpoints:");
    println!("   - WebSocket: ws://localhost:{}/ws", addr.1);
    println!("   - Health check: http://localhost:{}/health", addr.1);
    println!("   - Web client: http://localhost:{}", addr.1);

    // Run the server
    warp::serve(routes).run(addr).await;

    Ok(())
}

// Authentication route handlers
async fn handle_register(
    registration: UserRegistration,
    db: Database,
) -> Result<impl warp::Reply, warp::Rejection> {
    match create_user(&db, registration).await {
        Ok(user) => {
            let token = generate_jwt_token(&user)?;
            let response = serde_json::json!({
                "success": true,
                "user": {
                    "id": user.id,
                    "username": user.username,
                    "email": user.email,
                    "display_name": user.display_name
                },
                "token": token
            });
            Ok(warp::reply::json(&response))
        }
        Err(e) => Err(warp::reject::custom(e)),
    }
}

async fn handle_login(login: UserLogin, db: Database) -> Result<impl warp::Reply, warp::Rejection> {
    match authenticate_user(&db, login).await {
        Ok(user) => {
            let token = generate_jwt_token(&user)?;
            let response = serde_json::json!({
                "success": true,
                "user": {
                    "id": user.id,
                    "username": user.username,
                    "email": user.email,
                    "display_name": user.display_name
                },
                "token": token
            });
            Ok(warp::reply::json(&response))
        }
        Err(e) => Err(warp::reject::custom(e)),
    }
}

async fn handle_profile(
    auth_header: String,
    db: Database,
) -> Result<impl warp::Reply, warp::Rejection> {
    let token = auth_header.strip_prefix("Bearer ").ok_or_else(|| {
        warp::reject::custom(AppError::AuthenticationError(
            "Invalid authorization header".to_string(),
        ))
    })?;

    let claims = verify_jwt_token(token)?;

    match get_user_by_id(&db, &claims.sub).await? {
        Some(user) => {
            let response = serde_json::json!({
                "success": true,
                "user": {
                    "id": user.id,
                    "username": user.username,
                    "email": user.email,
                    "display_name": user.display_name,
                    "created_at": user.created_at,
                    "last_login": user.last_login,
                    "preferences": user.preferences
                }
            });
            Ok(warp::reply::json(&response))
        }
        None => Err(warp::reject::custom(AppError::AuthenticationError(
            "User not found".to_string(),
        ))),
    }
}

async fn handle_report(
    report: UserReport,
    auth_header: String,
    db: Database,
) -> Result<impl warp::Reply, warp::Rejection> {
    let token = auth_header.strip_prefix("Bearer ").ok_or_else(|| {
        warp::reject::custom(AppError::AuthenticationError(
            "Invalid authorization header".to_string(),
        ))
    })?;

    let claims = verify_jwt_token(token)?;

    let mut report = report;
    report.reporter_id = claims.sub;

    create_user_report(&db, report).await?;

    let response = serde_json::json!({
        "success": true,
        "message": "Report submitted successfully"
    });
    Ok(warp::reply::json(&response))
}

async fn handle_create_room(
    request: CreateRoomRequest,
    auth_header: String,
    db: Database,
) -> Result<impl warp::Reply, warp::Rejection> {
    let token = auth_header.strip_prefix("Bearer ").ok_or_else(|| {
        warp::reject::custom(AppError::AuthenticationError(
            "Invalid authorization header".to_string(),
        ))
    })?;

    let claims = verify_jwt_token(token)?;

    match create_room(&db, &claims.sub, &request.room_type, request.name).await {
        Ok(room) => {
            // Add creator as first participant
            let _ = join_room_participant(&db, &room.id, &claims.sub).await;

            let response = serde_json::json!({
                "success": true,
                "room": room,
                "message": "Room created successfully"
            });
            Ok(warp::reply::json(&response))
        }
        Err(e) => Err(warp::reject::custom(e)),
    }
}

async fn handle_join_room(
    request: JoinRoomRequest,
    auth_header: String,
    db: Database,
) -> Result<impl warp::Reply, warp::Rejection> {
    let token = auth_header.strip_prefix("Bearer ").ok_or_else(|| {
        warp::reject::custom(AppError::AuthenticationError(
            "Invalid authorization header".to_string(),
        ))
    })?;

    let claims = verify_jwt_token(token)?;

    match find_room_by_invite_code(&db, &request.invite_code).await? {
        Some(room) => {
            // Check if room is full
            let participant_count = sqlx::query("SELECT COUNT(*) as count FROM room_participants WHERE room_id = ? AND left_at IS NULL")
                .bind(&room.id)
                .fetch_one(&*db)
                .await
                .map_err(|e| warp::reject::custom(AppError::InternalError(format!("Database error: {}", e))))?;

            let count: i64 = participant_count.get("count");
            if count >= room.max_participants as i64 {
                return Err(warp::reject::custom(AppError::PermissionDenied(
                    "Room is full".to_string(),
                )));
            }

            // Add user to room
            join_room_participant(&db, &room.id, &claims.sub).await?;

            let response = serde_json::json!({
                "success": true,
                "room": room,
                "message": "Joined room successfully"
            });
            Ok(warp::reply::json(&response))
        }
        None => Err(warp::reject::custom(AppError::TargetNotFound(
            "Room not found or invite code invalid".to_string(),
        ))),
    }
}
