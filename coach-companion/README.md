# Coach Companion — privacy-first voice prototype

A standalone, **synthetic-data-only** prototype for a Work and Health Coach's **personal reflective practice**, based on product concepts from IndividuateAI. It is *not* a clinical tool, official case record, NHS-approved product, or de-identification guarantee.

## Launch

From the repository root:

```sh
python3 -m http.server 8765
# Visit http://localhost:8765/coach-companion/
```

Use a recent desktop Chromium browser with on-device speech recognition (and an installed English language pack). Unsupported browsers fail closed; there is deliberately **no server/cloud transcription fallback**.

## Non-negotiable prototype boundaries

- **Use invented examples only. Never speak real clients' details into this prototype.**
- Only the coach speaks *after* a session. Do not record client conversations.
- No copying, pasting, typing, uploads, external model calls, storage, account sync, or export.
- Browser on-device speech recognition is requested explicitly (`processLocally = true`), and unavailable support blocks capture.
- The detected text is held in memory for this page session and discarded on Clear/refresh. The page has a restrictive network CSP.
- It may mask obvious contact details, but a pattern matcher **cannot reliably anonymise real case material**. It does not certify safety to share.
- Guidance generated here is deterministic, educational, reflective and deliberately generic. Not diagnoses, care plans, benefits eligibility, safeguarding triage or clinical recommendations.
- Do not add API requests, analytics, telemetry, persisted case records, or cloud voices in this folder without prior Information Governance approval and written threat-model review.

## Proposed end state (separate application after governance clearance)

See [product plan](./PRODUCT.md) and [safety gates](./GOVERNANCE.md). Future production needs employer/DPO/Caldicott review, approved hosting and processors, DPIA as appropriate, explicit lawful basis and retention controls, clinical-risk review if applicable. A passkey + encrypted database is **not** itself permission to process NHS client information.

## Test

Open `test.html` in a browser for deterministic screening and example classification tests. Never test using real cases.

## Suggested separation

Eventually create a new private, independently deployed repository/application rather than introducing a coach mode into the paid personal-therapy product. The branch here exists only to make the architecture/prototype reviewable without touching `master` or production.
