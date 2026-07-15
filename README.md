# IndividuateAI

The "Organic Integral" AI Therapist interface.

## Project Setup

This project is built with **Rust** (Leptos) and **Tailwind CSS**.

### Prerequisites

1.  **Rust**: [Install Rust](https://www.rust-lang.org/tools/install).
2.  **cargo-leptos**: Install the build tool.
    ```bash
    cargo install cargo-leptos
    ```
3.  **Tailwind CSS**: Installed locally via NPM.
    ```bash
    npm install
    ```

### Running the App

To start the development server with hot-reloading:

```bash
cargo leptos watch
```

Open [http://localhost:3008](http://localhost:3008) in your browser.

### Deterministic privacy E2E test

The standalone harness starts the app with an isolated SQLCipher database,
uses a local deterministic OpenAI-compatible stub for chat and embeddings,
and drives a Chromium virtual PRF authenticator. It does not read `.env` or
contact an LLM provider.

Requirements: Node.js/npm, Cargo, and Chromium or Chrome.

```bash
npm ci
E2E_BROWSER_PATH=/usr/bin/google-chrome npm run test:e2e
```

The test covers passkey registration/login, chat persistence, logout
isolation, process-restart persistence, and database assertions proving that
content and vector memory are encrypted and the canary is absent from the
database files. The temporary database is written under `target/e2e/`.

### Coolify deployment

GitHub Actions publishes the image to the public GitHub Container Registry:

```text
ghcr.io/dunctk/individuate-ai:latest
```

In Coolify, configure the application as a public Docker image deployment,
using that image and container port `3008`. Mount persistent storage at
`/app/data`, and provide the required runtime secrets (`COOKIE_SECRET`,
`MEMORY_DB_KEY`, `OPENAI_API_KEY`, and `OPENROUTER_API_KEY`).

Coolify's source auto-deploy is disabled so it does not deploy before the
image exists. Add the Coolify deploy-hook URL as the GitHub Actions secret
`COOLIFY_DEPLOY_HOOK_URL`, and add a Coolify API token with **Deploy** permission
as `COOLIFY_TOKEN`. The workflow sends that token as a Bearer authorization
header after both image pushes succeed. Set the GHCR package visibility to
**Public** once in GitHub package settings; subsequent pushes use the
repository's `GITHUB_TOKEN`.

### Model and usage configuration

The production defaults use `z-ai/glm-5.2` for therapist and drafting replies,
`openai/gpt-5.4-nano` for structured memory extraction,
`openai/gpt-4o-mini` for lightweight session titles, and
`text-embedding-3-small` for vector recall. Override them with
`OPENROUTER_MODEL`, `GRAPH_EXTRACTOR_MODEL`, `RELATIONSHIP_PROFILE_MODEL`,
`SOCIAL_RELATIONSHIP_MODEL`, `EPISODE_EXTRACTOR_MODEL`,
`SESSION_SUMMARY_MODEL`, and `EMBEDDING_MODEL`.

Session titles are refreshed after the first exchange and every four exchanges
thereafter. `SESSION_SUMMARY_EVERY_N_EXCHANGES` changes that interval. Model
prompts retain the most recent 24 messages by default; use
`MAX_AGENT_HISTORY_MESSAGES` to tune the bounded history window. Complete chat
history remains encrypted in SQLite and available to focused recall.

### Stripe subscriptions

The app is paid-only. Stripe Checkout offers one complete plan at $24.99 USD
monthly or $239 USD yearly, and €29.99 monthly or €289 yearly. The EUR prices
are configured as tax-inclusive. Plan copy avoids fixed usage claims; the
server applies private, environment-configurable capacity safeguards.

Create or verify the product, four recurring prices, tax code, and customer
portal configuration in each Stripe mode:

```bash
./scripts/setup_stripe_catalog.sh sandbox
./scripts/setup_stripe_catalog.sh live
```

The script uses stable Stripe lookup keys, so it is safe to rerun and the app
does not need price IDs in its environment. Configure these runtime variables:

```text
BILLING_ENABLED=true
ADMIN_EMAIL=d@uncan.net
STRIPE_MODE=live
APP_BASE_URL=https://individuateai.com
STRIPE_AUTOMATIC_TAX=true
STRIPE_SANDBOX_SECRET_KEY=sk_test_...
STRIPE_SANDBOX_PUBLISHABLE_KEY=pk_test_...
STRIPE_SANDBOX_WEBHOOK_SECRET=whsec_... # optional only during local sandbox setup
STRIPE_LIVE_SECRET_KEY=sk_live_...
STRIPE_LIVE_PUBLISHABLE_KEY=pk_live_...
STRIPE_LIVE_WEBHOOK_SECRET=whsec_...
```

`ADMIN_EMAIL` is matched case-insensitively against the signed-in account. That
account bypasses billing and can open `/admin` to grant or revoke lifetime
complimentary access for registered users. Grants are stored in SQLite with an
append-only grant/revocation event log; they do not cancel an existing Stripe
subscription.

Create each webhook and immediately copy the one-time signing-secret output
into the matching runtime variable:

```bash
./scripts/setup_stripe_webhook.sh sandbox https://individuateai.com
./scripts/setup_stripe_webhook.sh live https://individuateai.com
```

The endpoint subscribes only to Checkout completion and subscription lifecycle
events. Finish the applicable Stripe Tax registrations before enabling
automatic tax in live mode.

Internal safeguards can be adjusted with `MONTHLY_CHAT_SOFT_LIMIT`,
`MONTHLY_VOICE_TOKEN_SOFT_LIMIT`, and `MONTHLY_TTS_CHARACTER_SOFT_LIMIT`.
These are operational controls, not advertised plan quotas.

## Design System

*   **Colors**: Deep Void Green, Parchment, Integral Turquoise, Systemic Yellow.
*   **Fonts**: Fraunces (Headings), Urbanist (Body).
*   **Vibe**: Botanical, Jungian, Glassmorphism.

## Architecture

*   **Frontend**: Leptos (WASM).
*   **Server**: Axum (SSR).
*   **Styling**: Tailwind CSS (v3).

## License

Copyright (C) 2026 Duncan.

This project is licensed under the **GNU Affero General Public License v3.0 or
later** (`AGPL-3.0-or-later`). See [LICENSE](LICENSE) for the complete terms.
If you run a modified version of this software as a network service, you must
make the corresponding source available to its users.
