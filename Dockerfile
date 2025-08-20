# Multi-stage Docker build for WebRTC Video Chat Application

# Build stage
FROM rust:1.75-alpine AS builder

# Install build dependencies
RUN apk add --no-cache \
    musl-dev \
    pkgconfig \
    openssl-dev \
    sqlite-dev

# Create a new empty shell project
WORKDIR /app

# Copy manifests
COPY Cargo.toml Cargo.lock ./

# Copy source code
COPY src ./src
COPY static ./static
COPY schema.sql ./schema.sql
COPY benches ./benches
COPY tests ./tests

# Build the application in release mode
RUN cargo build --release

# Runtime stage
FROM alpine:3.19

# Install runtime dependencies
RUN apk add --no-cache \
    ca-certificates \
    sqlite \
    openssl

# Create app user
RUN addgroup -g 1001 -S appgroup && \
    adduser -S appuser -u 1001 -G appgroup

# Create app directory
WORKDIR /app

# Copy the binary from builder stage
COPY --from=builder /app/target/release/webrtc-video-chat /usr/local/bin/webrtc-video-chat

# Copy static files
COPY --from=builder /app/static ./static
COPY --from=builder /app/schema.sql ./schema.sql

# Create necessary directories
RUN mkdir -p /app/data && \
    chown -R appuser:appgroup /app

# Switch to non-root user
USER appuser

# Expose the port the app runs on
EXPOSE 5001

# Health check
HEALTHCHECK --interval=30s --timeout=3s --start-period=5s --retries=3 \
    CMD wget --no-verbose --tries=1 --spider http://localhost:5001/health || exit 1

# Set environment variables
ENV RUST_LOG=info
ENV DATABASE_URL=sqlite:/app/data/chat.db

# Run the binary
CMD ["webrtc-video-chat"]