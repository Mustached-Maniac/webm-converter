# Multi-stage build for optimized image size
FROM rust:latest AS builder

WORKDIR /app

# Install dependencies
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

# Copy manifests
COPY Cargo.toml ./

# Copy source code
COPY src ./src

# Build the application
RUN cargo build --release

# Runtime stage with FFmpeg
FROM debian:sid-slim

# Install FFmpeg and runtime dependencies
RUN apt-get update && apt-get install -y \
    ffmpeg \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# Copy the built binary from builder
COPY --from=builder /app/target/release/webm-converter /usr/local/bin/webm-converter

# Create a non-root user
RUN useradd -m -u 1000 converter && \
    mkdir -p /tmp/conversions && \
    chown -R converter:converter /tmp/conversions

USER converter

WORKDIR /home/converter

# Expose the port
EXPOSE 8666

# Run the server
CMD ["webm-converter"]