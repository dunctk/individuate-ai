# Safety and Information Governance gate

**Status: DESIGN / SYNTHETIC DEMO ONLY. NOT AUTHORISED FOR REAL CLIENT DATA.**

This tool is intended for *post-session reflective professional development*, not to conduct appointments or create the official patient record. Consent to coaching, employment, or the existence of a confidentiality agreement does **not** automatically authorise processing in a personal app.

## Before any real client-related information can be used

1. Seek written direction from the employer's Information Governance lead / Data Protection Officer and where applicable the Caldicott Guardian, clinical safety lead, and IT security team. Check local NHS primary-care / GP Alliance policies and contracts; the employer may not be the only data controller.
2. Agree the precise purpose, controller/processor roles, privacy notice, lawful basis (UK GDPR Article 6 and Article 9 if special-category personal data), any required DPIA, supplier agreements, international transfers, model training/retention restrictions, hosting, access and retention.
3. Confirm whether DTAC applies. Determine DCB0129 / DCB0160 applicability based on *intended use* and commission/deployment, using NHS England's official applicability tool. If advice influences health-care decisions, perform appropriate safety case/hazard controls and clinical review. A coaching assistant must not function as an unreviewed medical device.
4. Assess indirect identifiability: rare job + location + circumstances + time can identify an individual despite pseudonyms. Client A, stable aliases and remembered histories are *pseudonymised personal data*, not necessarily anonymous.
5. Identify safe device and context: no recording clients, no unapproved personal devices for identifiable data, no use in public locations where speech can be overheard.
6. Penetration test, processor/subprocessor audit, log/trace inspection, data deletion/retention validation, permission checks, incident handling and accessibility testing. Test with fictional and adversarial inputs only until authorised.
7. Approval should define whether **only genuinely non-identifying, generalised themes** may be analysed, or controlled pseudonymised cases. Do not assume automated redaction is sufficient.

## Data pipeline for a future approved product

```text
Georgia speaks ALONE post-session
      ↓
On-device speech to text (never send unredacted audio externally)
      ↓
Local direct-identifier masking + indirect-identifier risk detection
      ↓
Only a generalised coaching dilemma, not a case note
      ↓
Local second-pass challenge and user review, BLOCK on uncertainty
      ↓
[If institution authorises] governed inference, or local model
      ↓
Evidence-aware reflective prompts, with uncertainty and human oversight
      ↓
Minimal, encrypted, time-limited approved storage OR no storage
```

Never store/upload audio by default. Provider 'zero retention' is a *contractual/routing claim*, not NHS approval. Do not enable the inherited Deepgram/remote OpenRouter pipeline for confidential case material.

## Safety behaviour

- No automated diagnoses, risk stratification, suitability judgements, benefit eligibility determinations, or treatment plans.
- Do not imply certainty about clients' motivations or psychology. Separate observations, interpretations and unknowns; offer alternatives.
- If urgency/safeguarding is voiced, stop normal coaching and advise following the employer's immediate escalation pathways. No suggestion to keep risks in the private application.
- Do not invent or silently insert entries in any clinical record; official systems are source of truth.
- Avoid cross-case recalls and graph links that could lead to re-identification, or leak one person's details into another conversation.
- Present framework prompts (MI/OARS, COM-B, biopsychosocial, reasonable adjustments, strengths-based) as options, not clinical conclusions.
- Be dyslexia-friendly: generous whitespace, concise cards, speech as primary action, visible controls, no forced animations.

## Sources, re-check at deployment

- [ICO — pseudonymisation](https://ico.org.uk/for-organisations/uk-gdpr-guidance-and-resources/data-sharing/anonymisation/pseudonymisation)
- [ICO — special category data](https://ico.org.uk/for-organisations/uk-gdpr-guidance-and-resources/lawful-basis/special-category-data/what-are-the-rules-on-special-category-data/)
- [NHS England — AI guidance for health and care professionals](https://digital.nhs.uk/data-and-information/information-governance/guidance/artificial-intelligence/guidance-for-health-and-care-professionals)
- [NHS England — DCB clinical risk management standards](https://digital.nhs.uk/services/clinical-safety/clinical-risk-management-standards)
- [NHS England — ambient scribing guidance](https://www.england.nhs.uk/long-read/guidance-on-the-use-of-ai-enabled-ambient-scribing-products-in-health-and-care-settings/)
- [MDN — on-device SpeechRecognition](https://developer.mozilla.org/en-US/docs/Web/API/SpeechRecognition/processLocally)
