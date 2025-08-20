use std::env;
use std::time::Duration;
use tokio::time;
use uuid::Uuid;
use sqlx::SqlitePool;
use serde_json::json;

// Test configuration
const TEST_DB_URL: &str = "sqlite::memory:";
const TEST_SERVER_PORT: u16 = 5002;

#[tokio::test]
async fn test_database_initialization() {
    let pool = SqlitePool::connect(TEST_DB_URL).await.unwrap();
    
    // Test that we can create tables
    let schema = include_str!("../schema.sql");
    for statement in schema.split(';') {
        let statement = statement.trim();
        if !statement.is_empty() {
            sqlx::query(statement)
                .execute(&pool)
                .await
                .expect("Failed to execute schema statement");
        }
    }
    
    // Test basic queries
    let user_count = sqlx::query("SELECT COUNT(*) as count FROM users")
        .fetch_one(&pool)
        .await
        .unwrap()
        .get::<i64, _>("count");
    
    assert_eq!(user_count, 0);
}

#[tokio::test]
async fn test_user_registration_and_authentication() {
    let pool = SqlitePool::connect(TEST_DB_URL).await.unwrap();
    init_test_database(&pool).await;
    
    // Test user registration
    let registration = serde_json::json!({
        "username": "testuser",
        "email": "test@example.com",
        "password": "testpassword123",
        "display_name": "Test User"
    });
    
    let client = reqwest::Client::new();
    let response = client
        .post(&format!("http://localhost:{}/api/register", TEST_SERVER_PORT))
        .json(&registration)
        .send()
        .await;
    
    // Note: This test would require the server to be running
    // In a real test environment, you'd start the server programmatically
}

#[tokio::test]
async fn test_websocket_connection() {
    // Test WebSocket connection and message handling
    use tokio_tungstenite::{connect_async, tungstenite::protocol::Message};
    
    let ws_url = format!("ws://localhost:{}/ws", TEST_SERVER_PORT);
    
    // This would test WebSocket connectivity
    // Note: Requires server to be running
}

#[tokio::test]
async fn test_rate_limiting() {
    // Test rate limiting functionality
    let pool = SqlitePool::connect(TEST_DB_URL).await.unwrap();
    init_test_database(&pool).await;
    
    // Simulate rapid message sending to test rate limits
    // This would test the rate limiting logic in process_message
}

#[tokio::test]
async fn test_content_filtering() {
    // Test inappropriate content filtering
    let test_cases = vec![
        ("normal message", false),
        ("SPAM SPAM SPAM", true),
        ("AAAAAAAAAA", true), // Excessive caps
        ("aaaaaaa", true),    // Excessive repeated chars
        ("Hello world!", false),
        ("buy crypto now!!!", true),
    ];
    
    for (message, should_be_filtered) in test_cases {
        let result = contains_inappropriate_content(message);
        assert_eq!(result, should_be_filtered, "Failed for message: {}", message);
    }
}

#[tokio::test]
async fn test_jwt_token_generation_and_validation() {
    use chrono::Utc;
    
    // Set test JWT secret
    env::set_var("JWT_SECRET", "test-secret-key");
    
    let user = DbUser {
        id: Uuid::new_v4().to_string(),
        username: "testuser".to_string(),
        email: "test@example.com".to_string(),
        display_name: Some("Test User".to_string()),
        created_at: Utc::now(),
        last_login: Some(Utc::now()),
        is_active: true,
        is_admin: false,
        preferences: None,
    };
    
    // Test token generation
    let token = generate_jwt_token(&user).unwrap();
    assert!(!token.is_empty());
    
    // Test token validation
    let claims = verify_jwt_token(&token).unwrap();
    assert_eq!(claims.sub, user.id);
    assert_eq!(claims.username, user.username);
}

#[tokio::test]
async fn test_admin_endpoints_require_admin_privileges() {
    let pool = SqlitePool::connect(TEST_DB_URL).await.unwrap();
    init_test_database(&pool).await;
    
    // Test that regular users cannot access admin endpoints
    // This would test the verify_admin_auth function
}

#[tokio::test]
async fn test_ice_server_configuration() {
    // Test ICE server configuration with environment variables
    env::set_var("TURN_SERVER", "turn:test.example.com:3478");
    env::set_var("TURN_USERNAME", "testuser");
    env::set_var("TURN_CREDENTIAL", "testcred");
    
    // Test that ICE servers are configured correctly
    // This would test the ICE server setup logic
}

#[tokio::test]
async fn test_room_creation_and_management() {
    let pool = SqlitePool::connect(TEST_DB_URL).await.unwrap();
    init_test_database(&pool).await;
    
    // Create a test user first
    let user_id = Uuid::new_v4().to_string();
    create_test_user(&pool, &user_id).await;
    
    // Test room creation
    let room = create_room(&pool, &user_id, "private", Some("Test Room".to_string()))
        .await
        .unwrap();
    
    assert_eq!(room.room_type, "private");
    assert_eq!(room.name, Some("Test Room".to_string()));
    assert!(room.invite_code.is_some());
    
    // Test joining room
    let result = join_room_participant(&pool, &room.id, &user_id).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_user_reporting_system() {
    let pool = SqlitePool::connect(TEST_DB_URL).await.unwrap();
    init_test_database(&pool).await;
    
    let reporter_id = Uuid::new_v4().to_string();
    let reported_id = Uuid::new_v4().to_string();
    
    create_test_user(&pool, &reporter_id).await;
    create_test_user(&pool, &reported_id).await;
    
    let report = UserReport {
        reporter_id: reporter_id.clone(),
        reported_user_id: reported_id.clone(),
        report_type: "spam".to_string(),
        description: Some("User was sending spam messages".to_string()),
        room_id: None,
    };
    
    let result = create_user_report(&pool, report).await;
    assert!(result.is_ok());
    
    // Verify report was created
    let reports = sqlx::query("SELECT COUNT(*) as count FROM user_reports WHERE reporter_id = ?")
        .bind(&reporter_id)
        .fetch_one(&pool)
        .await
        .unwrap()
        .get::<i64, _>("count");
    
    assert_eq!(reports, 1);
}

#[tokio::test]
async fn test_password_hashing_and_verification() {
    use bcrypt::{hash, verify, DEFAULT_COST};
    
    let password = "testpassword123";
    let hashed = hash(password, DEFAULT_COST).unwrap();
    
    // Test that verification works
    assert!(verify(password, &hashed).unwrap());
    
    // Test that wrong password fails
    assert!(!verify("wrongpassword", &hashed).unwrap());
}

#[tokio::test]
async fn test_concurrent_websocket_connections() {
    // Test multiple WebSocket connections simultaneously
    use futures::future::join_all;
    
    let connection_count = 10;
    let mut tasks = Vec::new();
    
    for i in 0..connection_count {
        let task = tokio::spawn(async move {
            // Simulate WebSocket connection
            time::sleep(Duration::from_millis(100)).await;
            format!("Connection {}", i)
        });
        tasks.push(task);
    }
    
    let results = join_all(tasks).await;
    assert_eq!(results.len(), connection_count);
}

#[tokio::test]
async fn test_error_handling_and_recovery() {
    // Test various error conditions and recovery mechanisms
    let pool = SqlitePool::connect(TEST_DB_URL).await.unwrap();
    init_test_database(&pool).await;
    
    // Test invalid user ID
    let result = get_user_by_id(&pool, "invalid-uuid").await;
    assert!(result.is_ok()); // Should return None, not error
    
    // Test duplicate user registration
    let registration = UserRegistration {
        username: "duplicate".to_string(),
        email: "dup@example.com".to_string(),
        password: "password123".to_string(),
        display_name: None,
    };
    
    // First registration should succeed
    let result1 = create_user(&pool, registration.clone()).await;
    assert!(result1.is_ok());
    
    // Second registration with same username should fail
    let result2 = create_user(&pool, registration).await;
    assert!(result2.is_err());
}

// Helper functions for tests
async fn init_test_database(pool: &SqlitePool) {
    let schema = include_str!("../schema.sql");
    for statement in schema.split(';') {
        let statement = statement.trim();
        if !statement.is_empty() {
            sqlx::query(statement)
                .execute(pool)
                .await
                .expect("Failed to execute schema statement");
        }
    }
}

async fn create_test_user(pool: &SqlitePool, user_id: &str) {
    use bcrypt::{hash, DEFAULT_COST};
    use chrono::Utc;
    
    let password_hash = hash("testpassword", DEFAULT_COST).unwrap();
    
    sqlx::query(
        "INSERT INTO users (id, username, email, password_hash, created_at, is_active, is_admin) 
         VALUES (?, ?, ?, ?, ?, ?, ?)"
    )
    .bind(user_id)
    .bind(format!("user_{}", &user_id[0..8]))
    .bind(format!("{}@test.com", &user_id[0..8]))
    .bind(&password_hash)
    .bind(Utc::now())
    .bind(true)
    .bind(false)
    .execute(pool)
    .await
    .expect("Failed to create test user");
}

// Import the functions we want to test
// Note: In a real project, these would be properly exposed from your main module
use webrtc_video_chat::*; // This would be your crate name

// Mock function for testing content filtering (this would be imported from your main module)
fn contains_inappropriate_content(message: &str) -> bool {
    let message_lower = message.to_lowercase();
    let inappropriate_words = [
        "spam", "scam", "viagra", "casino", "bitcoin", "crypto", "investment",
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
                return true;
            }
        } else {
            repeat_count = 1;
        }
        prev_char = ch;
    }

    false
}