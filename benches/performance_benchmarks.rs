use criterion::{black_box, criterion_group, criterion_main, Criterion, BenchmarkId};
use std::time::Duration;
use uuid::Uuid;
use bcrypt::{hash, verify, DEFAULT_COST};
use chrono::Utc;
use serde_json::json;

// Benchmark message processing throughput
fn benchmark_message_processing(c: &mut Criterion) {
    let mut group = c.benchmark_group("message_processing");
    
    let test_messages = vec![
        json!({"message_type": "offer", "target": "user123", "data": "test_offer_data"}),
        json!({"message_type": "answer", "target": "user456", "data": "test_answer_data"}),
        json!({"message_type": "ice-candidate", "target": "user789", "data": "test_ice_data"}),
        json!({"message_type": "chat", "target": "user101", "data": "Hello, world!"}),
    ];
    
    for (i, message) in test_messages.iter().enumerate() {
        group.bench_with_input(
            BenchmarkId::new("process_message", i),
            message,
            |b, msg| {
                b.iter(|| {
                    let serialized = black_box(serde_json::to_string(msg).unwrap());
                    // Simulate message processing
                    let _: serde_json::Value = black_box(serde_json::from_str(&serialized).unwrap());
                })
            }
        );
    }
    
    group.finish();
}

// Benchmark content filtering performance
fn benchmark_content_filtering(c: &mut Criterion) {
    let mut group = c.benchmark_group("content_filtering");
    
    let test_messages = vec![
        "Hello, how are you today?",
        "This is a normal message with some content",
        "SPAM SPAM SPAM BUY CRYPTO NOW!!!",
        "aaaaaaaaaaaaaaaaaaaaaa", // Excessive repeated chars
        "EXCESSIVE CAPS MESSAGE HERE", // Excessive caps
        "A very long message that contains multiple words and should test the filtering performance with various content including some potentially inappropriate words like spam and casino",
    ];
    
    for (i, message) in test_messages.iter().enumerate() {
        group.bench_with_input(
            BenchmarkId::new("filter_content", i),
            message,
            |b, msg| {
                b.iter(|| {
                    black_box(contains_inappropriate_content(msg))
                })
            }
        );
    }
    
    group.finish();
}

// Benchmark password hashing performance
fn benchmark_password_hashing(c: &mut Criterion) {
    let mut group = c.benchmark_group("password_hashing");
    
    let passwords = vec![
        "password123",
        "super_secure_password_with_special_chars!@#$",
        "short",
        "a_very_long_password_that_might_take_longer_to_hash_than_normal_passwords",
    ];
    
    for (i, password) in passwords.iter().enumerate() {
        group.bench_with_input(
            BenchmarkId::new("hash_password", i),
            password,
            |b, pwd| {
                b.iter(|| {
                    black_box(hash(pwd, DEFAULT_COST).unwrap())
                })
            }
        );
    }
    
    // Benchmark password verification
    let test_password = "test_password_123";
    let hashed = hash(test_password, DEFAULT_COST).unwrap();
    
    group.bench_function("verify_password", |b| {
        b.iter(|| {
            black_box(verify(test_password, &hashed).unwrap())
        })
    });
    
    group.finish();
}

// Benchmark JWT token operations
fn benchmark_jwt_operations(c: &mut Criterion) {
    use jsonwebtoken::{encode, decode, Header, Algorithm, EncodingKey, DecodingKey, Validation};
    use serde::{Deserialize, Serialize};
    
    #[derive(Debug, Serialize, Deserialize)]
    struct Claims {
        sub: String,
        username: String,
        exp: usize,
        iat: usize,
    }
    
    let mut group = c.benchmark_group("jwt_operations");
    
    let secret = "test_secret_key_for_benchmarking";
    let claims = Claims {
        sub: Uuid::new_v4().to_string(),
        username: "testuser".to_string(),
        exp: (Utc::now().timestamp() + 3600) as usize,
        iat: Utc::now().timestamp() as usize,
    };
    
    // Benchmark token generation
    group.bench_function("generate_token", |b| {
        b.iter(|| {
            black_box(encode(
                &Header::default(),
                &claims,
                &EncodingKey::from_secret(secret.as_ref())
            ).unwrap())
        })
    });
    
    // Benchmark token validation
    let token = encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(secret.as_ref())
    ).unwrap();
    
    let validation = Validation::new(Algorithm::HS256);
    
    group.bench_function("validate_token", |b| {
        b.iter(|| {
            black_box(decode::<Claims>(
                &token,
                &DecodingKey::from_secret(secret.as_ref()),
                &validation
            ).unwrap())
        })
    });
    
    group.finish();
}

// Benchmark UUID generation
fn benchmark_uuid_generation(c: &mut Criterion) {
    c.bench_function("generate_uuid", |b| {
        b.iter(|| {
            black_box(Uuid::new_v4().to_string())
        })
    });
}

// Benchmark JSON serialization/deserialization
fn benchmark_json_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("json_operations");
    
    let test_data = json!({
        "message_type": "offer",
        "target": "user123",
        "data": "very_long_webrtc_offer_data_that_might_be_large_in_real_scenarios",
        "source": "user456",
        "timestamp": Utc::now().timestamp(),
        "additional_metadata": {
            "connection_id": Uuid::new_v4().to_string(),
            "quality": "high",
            "codec": "h264"
        }
    });
    
    group.bench_function("serialize_json", |b| {
        b.iter(|| {
            black_box(serde_json::to_string(&test_data).unwrap())
        })
    });
    
    let serialized = serde_json::to_string(&test_data).unwrap();
    
    group.bench_function("deserialize_json", |b| {
        b.iter(|| {
            let _: serde_json::Value = black_box(serde_json::from_str(&serialized).unwrap());
        })
    });
    
    group.finish();
}

// Benchmark concurrent operations
fn benchmark_concurrent_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("concurrent_operations");
    
    use tokio::runtime::Runtime;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    
    let rt = Runtime::new().unwrap();
    
    group.bench_function("concurrent_uuid_generation", |b| {
        b.iter(|| {
            rt.block_on(async {
                let counter = Arc::new(AtomicUsize::new(0));
                let mut tasks = Vec::new();
                
                for _ in 0..100 {
                    let counter_clone = counter.clone();
                    let task = tokio::spawn(async move {
                        let _uuid = Uuid::new_v4().to_string();
                        counter_clone.fetch_add(1, Ordering::Relaxed);
                    });
                    tasks.push(task);
                }
                
                for task in tasks {
                    task.await.unwrap();
                }
                
                black_box(counter.load(Ordering::Relaxed))
            })
        })
    });
    
    group.finish();
}

// Mock function for testing (this would be imported from your main module in a real scenario)
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

criterion_group!(
    benches,
    benchmark_message_processing,
    benchmark_content_filtering,
    benchmark_password_hashing,
    benchmark_jwt_operations,
    benchmark_uuid_generation,
    benchmark_json_operations,
    benchmark_concurrent_operations
);

criterion_main!(benches);