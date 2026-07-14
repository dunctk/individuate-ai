# Repository Guidelines

## Project Structure & Module Organization
- `src/`: Rust code for Leptos components and Axum server. `main.rs` runs SSR, `app.rs` hosts the UI, `lib.rs` exposes hydration entry points, and `fileserv.rs` handles static files/errors.
- Persistent memory, mind map, and social graph live in SQLite through `src/agent.rs`; see "Memory System" below before editing related flows.
- `public/`: Static assets copied as-is to the built site; place favicon, manifest, and other non-compiled files here.
- `style/`: Tailwind sources. Edit `input.css`; the generated bundle is `output.css` (referenced in `Cargo.toml`).
- `index.html`: Base HTML shell for the WASM client.
- `target/`: Cargo build artifacts and the generated `target/site` output when building with `cargo-leptos`.

## Memory System
- SQLite path is controlled by `MEMORY_DB_PATH`, defaulting to `data/memory.sqlite`; never commit the database.
- The database is encrypted at rest with SQLCipher when `MEMORY_DB_KEY` is set (treated as a passphrase). On first start with a key, an existing plaintext database is auto-migrated in place, leaving a `*.plaintext.bak` copy to delete after verification. Without the key the store runs unencrypted and logs a warning; a wrong key fails cleanly when the store is first opened.
- Broader autobiographical memory comes from saved chat/session logs plus the sqlite vector store table `therapy_memory`.
- The therapist prompt receives a `<persistent_memory>` block containing relevant broader memories, mind-map nodes/edges, and social graph relationships. Treat that block as the agent's recall layer.
- Mind map data is stored in `patient_graphs`, exposed through `/api/graph/:user_id`, and updated from conversations by `GRAPH_DELTA_PROMPT` plus the `read_mind_map` / `update_mind_map` Rig tools. Updates are delta-only.
- Relationship profiles are stored in `relationship_profiles` and extracted from conversation text by the relationship-profile extractor.
- Direct person-to-person social facts are stored in `social_relationships`; the rendered social graph is stored in `social_graphs`.
- The social graph shown at `/social-graph` and returned by `/api/social-graph` is a projection built from relationship profiles, direct social relationships, and selected patient-graph concepts.
- Chat streaming emits `[RESPONSE_DONE]` when assistant text is complete and `[MEMORY_UPDATED]<headline>` when memory actually changed. The UI also polls `/api/memory-status` as a fallback using private graph signatures, not memory contents.
- Memory extraction model env vars: `GRAPH_EXTRACTOR_MODEL`, `RELATIONSHIP_PROFILE_MODEL`, `SOCIAL_RELATIONSHIP_MODEL`, `SESSION_SUMMARY_MODEL`, `EMBEDDING_MODEL`, and `OPENROUTER_MODEL`.
- Memory is scoped to authenticated user IDs. `DEFAULT_GRAPH_USER_ID` is only a fallback/default struct value, not the normal namespace for app users.

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
- The primary test user uses an iPhone SE (3rd generation). Treat its 375 x 667 CSS-pixel portrait viewport as the baseline mobile target, then also verify landscape, the on-screen keyboard, safe-area insets, and 44 x 44 CSS-pixel minimum touch targets.

## Commit & Pull Request Guidelines
- Follow conventional commits as in history (`feat:`, `fix:`, `chore:`). Keep messages in the imperative and scoped when helpful (e.g., `feat(ui): add sidebar sliders`).
- PRs should include: purpose summary, linked issue (if any), screenshots/GIFs for UI changes, and notes on testing performed (`cargo leptos watch`, `cargo test`, etc.).
- Keep changesets small and focused; mention any migrations or config steps explicitly.

## Security & Configuration Tips
- Feature flags control build mode: `ssr` for server builds, `hydrate` for client builds; `cargo-leptos` selects them automatically—avoid manual toggling unless you know the target.
- Configure memory and extraction with environment variables rather than checked-in config; most model env vars default to lightweight OpenRouter/OpenAI models in `src/agent.rs`.
- Do not commit secrets; prefer environment variables loaded by your process manager when deploying the Axum server.
- `COOKIE_SECRET` (min 32 chars, e.g. `openssl rand -hex 32`) is required; the server fails closed at startup without it, and rotating it logs every user out.
- OpenRouter requests send `provider.data_collection = deny` so prompts are only routed to zero-retention providers; set `OPENROUTER_DATA_COLLECTION=allow` if a chosen model has no such provider.
