# Justfile
check:
    cargo check

dev:
    cargo watch -x run

run:
    cargo run