# Justfile
check:
    cargo check

dev:
    cargo watch -x run

run:
    cargo run

e2e:
    npm run test:e2e
