#!/bin/bash

# Production deployment script for WebRTC Rust Application

set -e  # Exit on any error

echo "🚀 Starting WebRTC Rust Application in Production Mode"
echo "=================================================="

# Set production environment variables
export RUST_ENV=production
export RUST_LOG=warn

# Optional: Set custom database URL
export DATABASE_URL=${DATABASE_URL:-"sqlite:chat.db"}

# Optional: Set JWT secret (should be set in production)
export JWT_SECRET=${JWT_SECRET:-$(openssl rand -hex 32)}

# Optional: Set TURN server configuration
if [ -n "$TURN_SERVER" ]; then
    echo "📡 Using custom TURN server: $TURN_SERVER"
else
    echo "📡 Using default STUN servers (Google public STUN)"
fi

# Run quality checks
echo "🔍 Running pre-deployment checks..."
cargo check --quiet
cargo clippy --quiet -- -D warnings
echo "✅ Quality checks passed"

# Build optimized release version
echo "🏗️  Building optimized release..."
cargo build --release --quiet
echo "✅ Release build completed"

# Start the server
echo "🌐 Starting WebRTC signaling server..."
echo "📝 Available at: http://localhost:5001"
echo "🔗 WebSocket: ws://localhost:5001/ws"
echo "📊 Health check: http://localhost:5001/health"
echo ""
echo "Press Ctrl+C to stop the server"
echo "=================================================="

# Run the optimized binary
exec ./target/release/my-project