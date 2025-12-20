# Repository Guidelines

## Project Structure & Module Organization
- `src/`: Rust code for Leptos components and Axum server. `main.rs` runs SSR, `app.rs` hosts the UI, `lib.rs` exposes hydration entry points, and `fileserv.rs` handles static files/errors.
- Mind map graph: stored in SQLite `patient_graphs` table, exposed via `/api/graph/:user_id`, and updated through Rig tools + extractor (delta-only updates).
- `public/`: Static assets copied as-is to the built site; place favicon, manifest, and other non-compiled files here.
- `style/`: Tailwind sources. Edit `input.css`; the generated bundle is `output.css` (referenced in `Cargo.toml`).
- `index.html`: Base HTML shell for the WASM client.
- `target/`: Cargo build artifacts and the generated `target/site` output when building with `cargo-leptos`.

## Build, Test, and Development Commands
- Install once: `cargo install cargo-leptos` and `npm install` to pull Tailwind tooling.
- Dev server with hot reload: `cargo leptos watch` (runs server + client rebuilds).
- Production preview: `cargo leptos serve --release` (serves optimized server/client bundles).
- Static build: `cargo leptos build --release` (outputs to `target/site`; pkg assets under `target/site/pkg`).
- CSS tweak loop (optional when editing styles only): `npx tailwindcss -i style/input.css -o style/output.css --watch`.

## Coding Style & Naming Conventions
- Rust: 4-space indentation, `snake_case` for functions/vars, `PascalCase` for types/components, `SCREAMING_SNAKE_CASE` for constants. Keep components small and prefer prop structs over loose tuples.
- Run `cargo fmt` before commits; use `cargo clippy --all-targets --all-features` to catch lints.
- Keep Tailwind classes semantic and grouped logically; extract reusable patterns into small helper components in `app.rs` or a new module.

## Testing Guidelines
- No automated tests are present yet; add unit tests under `#[cfg(test)]` in the relevant module or integration tests in `tests/`.
- Use `cargo test` for Rust coverage. For end-to-end flows, wire up Playwright and run `npx playwright test` (command is predeclared in `Cargo.toml` metadata).
- Prefer deterministic data; avoid network calls in tests.

## Commit & Pull Request Guidelines
- Follow conventional commits as in history (`feat:`, `fix:`, `chore:`). Keep messages in the imperative and scoped when helpful (e.g., `feat(ui): add sidebar sliders`).
- PRs should include: purpose summary, linked issue (if any), screenshots/GIFs for UI changes, and notes on testing performed (`cargo leptos watch`, `cargo test`, etc.).
- Keep changesets small and focused; mention any migrations or config steps explicitly.

## Security & Configuration Tips
- Feature flags control build mode: `ssr` for server builds, `hydrate` for client builds; `cargo-leptos` selects them automatically—avoid manual toggling unless you know the target.
- Mind map extraction uses `GRAPH_EXTRACTOR_MODEL` (default `gpt-4o-mini`) and `GRAPH_USER_ID` (default `local-user`) to namespace the persistent graph.
- Do not commit secrets; prefer environment variables loaded by your process manager when deploying the Axum server.
