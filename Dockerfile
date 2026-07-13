FROM rust:1.93-bookworm AS builder

# Install Node.js and npm for Tailwind CSS
RUN apt-get update && apt-get install -y --no-install-recommends \
    nodejs \
    npm \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Install dependencies first so source-only changes reuse the dependency layer.
COPY Cargo.toml Cargo.lock package.json package-lock.json ./
RUN npm ci --ignore-scripts

COPY . .

# Build CSS
RUN npx tailwindcss -i style/input.css -o style/output.css --minify

# This application is server-rendered by Axum; the browser assets are served
# from public/ and the generated Tailwind bundle.
RUN cargo build --locked --release --bin individuateai

# Runner stage
FROM debian:bookworm-slim

WORKDIR /app

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    libsqlite3-0 \
    && rm -rf /var/lib/apt/lists/*

RUN mkdir -p data

COPY --from=builder /app/target/release/individuateai /app/individuateai

# Copy static assets used by the server.
COPY --from=builder /app/public /app/public
COPY --from=builder /app/style/output.css /app/style/output.css
COPY --from=builder /app/mandala-avatar.jpg /app/mandala-avatar.jpg
COPY --from=builder /app/mandala-avatar.mp4 /app/mandala-avatar.mp4

# Coolify can override these values, especially the persistent DB path.
ENV PORT="3008"
ENV LEPTOS_SITE_ROOT="/app/public"
ENV MEMORY_DB_PATH="/app/data/memory.sqlite"

# Expose the port
EXPOSE 3008

# Run the application
CMD ["./individuateai"]
