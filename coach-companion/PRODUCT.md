# Coach Companion — product / architecture plan

## Problem and intent

Georgia, working as an NHS-associated Work and Health Coach, wants to think out loud about complex coaching challenges, find non-judgemental perspectives and build her practice, **without copying records or typing**.

**North star:** helpful reflection without collecting a shadow patient record.

## Scope

### In first offline demo

- One large talk/stop control with *local-only* STT and fail-closed capability check.
- No text input, clipboard ingest, file ingest, external calls, permanent case memory or remote audio.
- Start with fictional sample. Local rules provide brief OARS, COM-B, boundaries and next-question ideas.
- Local-only identifier screen shows what was masked / uncertainty warning; never makes an anonymisation certification.
- Clear immediately, no client sessions recorded.

### Later, subject to employer approval

- Separate app, not a toggle in IndividuateAI production; re-use secure design patterns (passkeys, encrypted storage, memory isolation) only after audit.
- Reflective **themes**, not client case files; capture the minimum possible.
- Speech-to-text completely on approved device; e.g. vetted native or local model where browser local recognition unavailable.
- Conversational voice with constrained advice and on-device or approved TTS; avoid unreviewed speech services.
- Spoken prompts: “What felt difficult?”, “What assumptions am I making?”, “How can I elicit change talk?”, “What can I raise in supervision?”
- Optional *profession-focused* memory: Georgia's own learning goals, tools tried, reflections and confidence. No longitudinal individual case histories unless a formal governance decision allows them.
- OARS, motivational interviewing, COM-B, strengths-based coaching, return-to-work barriers, graded goals, workplace adjustments, signposting and supervision boundaries.
- Use insight cards: **Observed**, **One possibility**, **Alternative possibility**, **Try saying**, **Bring to supervision**.
- Accessibility: spoken input/output, big buttons, very little text, short cards, accessible fonts and contrast, transcript optional but read-only.

## Bounded assistant prompt contract (future)

“You support a coach's reflective professional development, not the coaching client's treatment. Refer to people generically. Ask about the coach's observations rather than inferring diagnoses. Clearly label assumptions, avoid personal judgments, never make employment, welfare, medical or safeguarding determinations. Encourage supervision and organisational escalation as required. Do not request, remember, or reveal identifiers.”

Prompt wording is an additional safeguard **not** a technical privacy control.

## Data categories

| Item | Demo | Proposed governed product |
| --- | --- | --- |
| Spoken audio | Browser/OS on-device only, ephemeral | Approved on-device STT; do not retain |
| Raw transcript | Ephemeral RAM only | Prefer ephemeral RAM only; purge on submit/timeout |
| Names/contact identifiers | Block use of real cases | Strip/block on-device *before transmission*, review |
| Generalised themes | Ephemeral RAM only | May allow under documented governance |
| Longitudinal client IDs/case graph | None | Default **off**, explicit institutional approval required |
| Professional-learning memory | None | Opt-in minimal encrypted notes about Georgia only |
| Logs and telemetry | None | Metadata only, audited for PHI/PII leaks |

## Acceptance criteria

- Production therapeutic / billing paths in original repo remain unchanged.
- Unsupported browser has **no recognition fallback**.
- No raw audio, name or transcript leaves prototype origin.
- Example email/phone/NHS number/postcode are flagged/masked; reviewers are told the filter is incomplete.
- No input/copy-paste surfaces, file upload or record-session feature.
- No patient/client names, employer, exact location, rare biography, precise dates or addresses in test fixtures.
- Assistant outputs are explicitly educational and grounded in general coaching frameworks.
- Before a production feature, pass written governance gate in GOVERNANCE.md.
- Existing `AGENTS.md` remains authoritative for core app; local instructions in this folder govern the prototype.
