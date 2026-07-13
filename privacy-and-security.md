# Privacy & Security Design

This app stores therapy conversations, autobiographical memory, relationship
profiles, and social graphs — the most sensitive class of personal data. This
document defines the security architecture: what we protect against, what we
deliberately do not, and the simplest design that meets those requirements.

## Requirements

1. A stolen copy of the database, a disk, or a backup must be unreadable.
2. A full compromise of the running server must not expose the data of users
   who are not actively using the app during the compromise.
3. The operator (us) must not be able to read user data — no master key,
   nothing meaningful to leak, subpoena, or abuse.
4. Users log in once per device and stay logged in. No password or PIN
   re-entry, ever.
5. All memory features keep working: background extraction, vector recall,
   mind map, social graph, session summaries.
6. Conversations sent to LLM providers must not be retained or trained on.

## Explicit non-goals

- **LLM plaintext.** The model must read conversations to respond. We control
  this exposure contractually (zero-retention routing), not cryptographically.
- **Active-session compromise.** A root attacker on a live server can capture
  the keys of users whose requests flow through during the compromise. The
  design shrinks the blast radius from "everyone, always" to "active users,
  during the window" — it cannot eliminate it.
- **No PIN.** A 4–6 digit PIN wrapping a key can be brute-forced offline in
  minutes to days. Without hardware rate-limiting (secure enclave), a PIN is
  theater. We rely on full-entropy secrets only.

## Layer 1 — implemented

- **SQLCipher at rest.** The whole SQLite file is encrypted with
  `MEMORY_DB_KEY` (env). A plaintext DB is auto-migrated on first keyed start
  (a `*.plaintext.bak` copy is kept and must be deleted after verification).
  Without the key the server runs unencrypted and warns; a wrong key fails
  cleanly at first open.
- **Fail-closed cookie key.** `COOKIE_SECRET` (min 32 chars) is required; the
  server refuses to start without it. The cookie key is derived via HKDF
  (`Key::derive_from`). Auth cookies are encrypted (`PrivateCookieJar`).
  Rotating the secret logs every user out but loses no data.
- **Zero-retention LLM routing.** Every OpenRouter request (chat agents and
  all extractors) sends `provider.data_collection = "deny"`, restricting
  routing to providers that do not retain or train on prompts.
  `OPENROUTER_DATA_COLLECTION=allow` is the escape hatch.
- **Scoping and auth.** Authentication is passkey-only. Every graph/stream/
  session handler verifies the authenticated private cookie matches the
  requested user; session ownership is checked before streaming. `data/` is
  gitignored wholesale so no database or backup can be committed.

Layer 1 protects against stolen disks, leaked backups, file-level exfiltration,
and commit accidents. It does not protect against a compromised running server,
because `MEMORY_DB_KEY` lives in the deployment environment.

## Layer 2 — per-user encryption (implemented)

One key hierarchy, hotel-lock model: **one DEK per user, many independent
sealed copies of it. The server can never unseal any of them.**

### Keys

- **DEK** — random 256-bit data-encryption key per user, created at signup.
  Never stored in plaintext.
- **Wraps** — each credential holds its own sealed copy of the DEK:
  - each **passkey**, via the WebAuthn **PRF extension**: the authenticator
    derives a deterministic 256-bit secret from a stored per-credential salt;
    HKDF turns it into the wrap key. Full entropy — nothing to brute-force.
  - the **recovery key** — a one-time code shown at signup, wrapping the same
    DEK. Mandatory: it is the only fallback when all passkeys are lost.

Schema per credential: `credential_id, user_id, public_key, prf_salt,
wrapped_dek, label, created_at`. The recovery key is just another wrap row.

### Auth is passkey-only

- Registration requires a PRF-capable authenticator (`prf.enabled` checked at
  create; reject otherwise). Coverage in 2026: iCloud Keychain, Google
  Password Manager, recent Windows Hello, YubiKeys, 1Password/Bitwarden.
- Password login, forgot-password, and reset-password flows are deleted —
  less attack surface, no password table, no weak-password brute-force.
- PRF evaluation happens in browser JS (`extensions.prf` on the ceremony);
  the output is POSTed alongside the credential JSON over TLS. webauthn-rs
  validates the ceremony unchanged.

### Stay-logged-in: DEK rides in the cookie

At login, the PRF output unwraps the DEK once; the DEK is then stored **inside
the encrypted auth cookie** and the server forgets it. Every request carries
the key; the server holds it only for the duration of the request plus a short
in-memory window (~15–30 min after last activity) so post-conversation
extraction can run. Logout deletes the cookie. Server restarts lose nothing
but an unlucky in-flight extraction.

This satisfies requirement 4 with no PIN and no re-prompt: one biometric tap
per device, then months of persistent login — while the server at rest holds
only sealed DEK copies.

### What gets encrypted

Content columns only, encrypted app-side with XChaCha20-Poly1305 under the
DEK: message text, memory-fragment text and embeddings, episodes, relationship
profiles, graph node/edge labels. IDs, user_ids, timestamps, and foreign keys
stay plaintext so queries, joins, and listings work unchanged. SQLCipher
remains underneath as the outer layer.

### Memory features under encryption

- **Extraction** — unchanged; extractors read the DEK from the session cache,
  which is always warm because extraction fires right after a conversation.
- **Vector recall** — embeddings are encrypted (inversion attacks reconstruct
  text), so SQL-side KNN is replaced by: fetch the user's fragments, decrypt,
  cosine similarity in memory. Per-user corpora are small (≤ thousands);
  this is sub-millisecond and removes the sqlite-vec dependency on plaintext.
- **Memory-status polling** — already compares graph *signatures*; compute
  them at write time (plaintext in hand) and store them unencrypted.

### Passkey lifecycle

- **Add** (requires an unlocked session): register new passkey → verify
  `prf.enabled` → silent `get()` restricted to the new credential to obtain
  its PRF output → wrap the session's DEK → store. Two biometric taps.
- **Login**: whichever passkey authenticates, its own salt + wrapped DEK are
  used. Synced passkeys (iCloud/Google) are one credential, one wrap.
- **Remove/revoke**: delete the row — the credential can no longer
  authenticate and its sealed DEK copy ceases to exist. Refuse to remove the
  last wrap unless the user confirms holding the recovery key; require a fresh
  passkey assertion before removals.
- **DEK rotation** (hardening, later): on revocation, optionally generate a
  fresh DEK, re-encrypt the user's content, re-wrap for remaining credentials.
  Covers the rare "old DB snapshot + later-stolen passkey" combination.

### Accepted tradeoffs

- Losing all passkeys **and** the recovery key means the data is
  unrecoverable. This is inherent: any recovery path we could offer is a
  backdoor someone else could use.
- The LLM boundary is unchanged; see non-goals.

## Operational rules

- Secrets (`MEMORY_DB_KEY`, `COOKIE_SECRET`, API keys) live only in the
  deployment environment, never in the repo. Back up `MEMORY_DB_KEY`
  out-of-band — it is unrecoverable by design.
- Delete `data/*.plaintext.bak` and any pre-encryption backups once the
  encrypted store is verified.
- The deployment dashboard (Coolify) holds every env var: 2FA, no public
  exposure (VPN/allowlist), keep it patched, don't co-host untrusted apps.
  SSH: keys only.
- Never log conversation content; keep `tracing` calls on the chat/extraction
  paths metadata-only.
- Rate-limit auth endpoints.

## Rollout phases

1. **Implemented:** key hierarchy, DEK + wraps table, PRF plumbing in the
   passkey JS/handlers, and DEK-in-cookie.
2. **Implemented:** encrypted content columns with per-user migration on next
   login.
3. **Implemented:** in-app vector recall over encrypted embeddings.
4. **Implemented:** recovery-key UX and passkey add/revoke support. Optional
   DEK rotation on revocation remains a later hardening step.

Each phase ships independently; the app works normally between phases.
