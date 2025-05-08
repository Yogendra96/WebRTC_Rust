# WebRTC Signaling Server with TURN Support

This is a WebRTC signaling server implemented in Rust with TURN server support for NAT traversal. The server facilitates WebRTC connections between peers, especially in challenging network environments where direct peer-to-peer connections might not be possible.

## Features

- WebSocket-based signaling server
- Multiple STUN/TURN server support for reliable connections
- Automatic fallback mechanisms for connection reliability
- Time-limited TURN credentials generation
- Environment variable configuration for production deployment

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

## Troubleshooting

- If connections fail, check that your TURN server is properly configured and accessible
- Ensure your TURN server supports both UDP and TCP for maximum compatibility
- For secure environments, use TURNS (TURN over TLS) on port 5349