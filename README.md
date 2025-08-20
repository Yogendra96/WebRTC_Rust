# Advanced WebRTC Video Chat Application

A modern, feature-rich WebRTC video chat application built with Rust backend and vanilla JavaScript frontend. This application provides a complete random video chat experience with advanced features for reliable peer-to-peer communication.

## ✨ Key Features

### 🎥 **Video & Audio**
- **High-quality video calls** with configurable resolution (360p, 480p, 720p)
- **Audio/video toggle controls** for privacy
- **Screen sharing capability** with automatic fallback to camera
- **Auto-start preferences** for media devices

### 👥 **Smart Matching**
- **Intelligent waiting room** with real-time queue status
- **Instant partner matching** when users are available
- **"Next Partner" functionality** for quick connections
- **Connection quality monitoring** with real-time indicators

### 💬 **Communication**
- **Real-time text chat** alongside video calls
- **Message history** during conversations
- **Content filtering** to prevent spam and inappropriate content
- **Rate limiting** (20 messages/minute) for abuse prevention

### 🔒 **Security & Privacy**
- **Message size limits** (16KB) to prevent DoS attacks
- **Automated content filtering** for inappropriate messages
- **Rate limiting per user** to prevent spam
- **Session management** with connection tracking
- **Comprehensive error handling** with user-friendly messages

### ⚙️ **User Experience**
- **Persistent user settings** stored locally
- **Responsive modern UI** with clean design
- **Connection status indicators** 
- **Quality monitoring** (Excellent/Good/Poor)
- **Graceful error handling** with recovery suggestions

### 🌐 **Network Reliability**
- **Multiple STUN/TURN server support** for NAT traversal
- **Automatic fallback mechanisms** for connection reliability
- **Environment variable configuration** for production
- **Time-limited TURN credentials** for security

## TURN Server Integration

The application now supports TURN (Traversal Using Relays around NAT) servers to enable WebRTC connections in restrictive network environments. TURN servers act as relays when direct peer-to-peer connections cannot be established.

### Configuration

The server supports multiple STUN and TURN servers for reliability:

1. **Default Configuration**: The server includes Google's public STUN servers by default.

2. **Environment Variables**: For production use, configure your TURN server using environment variables:
   - `TURN_SERVER`: Your TURN server URL(s), comma-separated for multiple URLs
   - `TURN_USERNAME`: Username for TURN authentication (optional)
   - `TURN_CREDENTIAL`: Credential for TURN authentication (optional)

   Example:
   ```bash
   export TURN_SERVER="turn:your-turn-server.com:3478?transport=udp,turn:your-turn-server.com:3478?transport=tcp"
   export TURN_USERNAME="your-username"
   export TURN_CREDENTIAL="your-credential"
   ```

3. **Fallback Mechanism**: The client automatically attempts reconnection with different transport options if the initial connection fails.

## Running the Server

```bash
# Run with default configuration
cargo run

# Run with custom TURN server
TURN_SERVER="turn:your-turn-server.com:3478" TURN_USERNAME="username" TURN_CREDENTIAL="password" cargo run
```

## Setting Up Your Own TURN Server

For production use, it's recommended to set up your own TURN server. You can use open-source solutions like:

1. **Coturn**: A popular TURN server implementation
   ```bash
   # Installation on Ubuntu
   sudo apt-get install coturn
   
   # Basic configuration in /etc/turnserver.conf
   listening-port=3478
   fingerprint
   lt-cred-mech
   realm=your-domain.com
   user=username:password
   ```

2. **Twilio's TURN Service**: A managed solution if you prefer not to host your own

## Client Usage

The client automatically receives the ICE server configuration from the server during connection setup. The WebRTC connection will attempt to use the provided STUN/TURN servers in order of preference.

## 🔧 Development Tooling

This project uses modern Rust development tools for code quality and consistency:

### Code Quality Tools
- **Clippy**: Advanced linting with custom configuration
- **rustfmt**: Consistent code formatting 
- **cargo check**: Fast compilation checking

### Available Commands
```bash
# Check code without building
cargo check

# Run advanced linting (zero warnings policy)
cargo clippy -- -D warnings

# Format code consistently
cargo fmt

# Run all quality checks
cargo check && cargo clippy -- -D warnings && cargo fmt
```

### Configuration Files
- `clippy.toml`: Clippy linting configuration
- `rustfmt.toml`: Code formatting rules
- Both configured for modern Rust best practices

## 📊 Code Quality Standards

The codebase maintains:
- ✅ Zero compilation warnings
- ✅ Zero Clippy warnings (strict mode)
- ✅ Consistent formatting via rustfmt
- ✅ Comprehensive error handling
- ✅ Modern Rust idioms and patterns

## Troubleshooting

- If connections fail, check that your TURN server is properly configured and accessible
- Ensure your TURN server supports both UDP and TCP for maximum compatibility
- For secure environments, use TURNS (TURN over TLS) on port 5349

## Architecture Notes

The application follows modern Rust patterns:
- **Type Safety**: Comprehensive error types with user-friendly messages
- **Memory Safety**: No unsafe code, all borrowing rules followed
- **Performance**: Async/await throughout, zero-copy where possible
- **Maintainability**: Clean separation of concerns, well-documented APIs