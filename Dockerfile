FROM rust:latest as builder

# Install Node.js and npm for Tailwind CSS
RUN apt-get update && apt-get install -y nodejs npm

WORKDIR /app

# Copy project files
COPY . .

# Install npm dependencies
RUN npm install

# Build CSS
RUN npx tailwindcss -i style/input.css -o style/output.css --minify

# Install cargo-leptos and add wasm target
RUN rustup target add wasm32-unknown-unknown
RUN cargo install cargo-leptos

# Build the application
RUN cargo leptos build --release

# Runner stage
FROM debian:bookworm-slim

WORKDIR /app

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    ca-certificates \
    openssl \
    libssl-dev \
    libsqlite3-0 \
    && rm -rf /var/lib/apt/lists/*

# Create data directory for SQLite
RUN mkdir -p data

# Copy the binary
COPY --from=builder /app/target/release/individuateai /app/individuateai

# Copy the site files (JS, CSS, WASM, static assets)
COPY --from=builder /app/target/site /app/site

# Set environment variables
ENV LEPTOS_SITE_ADDR="0.0.0.0:3008"
ENV LEPTOS_SITE_ROOT="site"
# Default DB path (can be overridden in Coolify)
ENV MEMORY_DB_PATH="/app/data/memory.sqlite"

# Expose the port
EXPOSE 3008

# Run the application
CMD ["./individuateai"]
