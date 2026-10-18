# Coach Companion agent instructions

This directory is an isolated, synthetic-data-only proof of concept. **Never deploy it for real NHS/GP patient/client information without the approvals and checks in GOVERNANCE.md.**

1. Do not import Deepgram, existing remote LLM agents, graph extractors, analytics, or production account/billing code.
2. Speech input must enforce on-device processing. Fail closed if support/pack is unavailable. No silent cloud fallback.
3. Do not introduce text entry, clipboard/file ingest, recorded appointment audio, personal data persistence, case-ID aliases, or external network requests in the prototype.
4. Never claim regular-expression screening makes client content anonymous. It is a teaching tool, not a certification. Keep the synthetic-only banner clear.
5. Output must be reflective, non-diagnostic, human-led, modest about evidence and clearly not a clinical/official record.
6. Use only invented anonymised composite demonstrations. Tests must use dummy contact details.
7. Restrict CSP, avoid logging transcripts, and preserve accessible mobile buttons and high readability.
8. Update PRODUCT.md and GOVERNANCE.md before expanding scope. Treat organisational approval as a hard release gate.
