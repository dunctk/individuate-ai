// Educational synthetic-only reflection utilities. Do not treat this as reliable anonymisation.
// No API calls, storage or third-party imports.
const directPatterns = [
  {label: "email", re: /\b[A-Z0-9._%+-]+@[A-Z0-9.-]+\.[A-Z]{2,}\b/gi, replacement: "[EMAIL]"},
  {label: "UK postcode", re: /\b(?:GIR\s?0AA|(?:[A-Z]{1,2}\d[A-Z\d]?\s?\d[A-Z]{2}))\b/gi, replacement: "[POSTCODE]"},
  {label: "telephone", re: /(?<!\d)(?:(?:\+44\s?\(?0?\)?|0)\s?\d(?:[\s-]?\d){8,9})(?!\d)/g, replacement: "[TELEPHONE]"},
  {label: "NHS number-like digits", re: /\b\d{3}[\s-]?\d{3}[\s-]?\d{4}\b/g, replacement: "[IDENTIFIER]"},
  {label: "web address", re: /\bhttps?:\/\/[^\s]+/gi, replacement: "[WEB ADDRESS]"}
];
const indirect = [
  /\b(?:works?|working|employed|employer|manager|school|college|practice|hospital|surgery|company)\s+(?:at|for|in|near)\b/i,
  /\b(?:called|named|surname|first name|full name|lives? at|born on|birthday|diagnosed on)\b/i,
  /\b(?:yesterday|last (?:Monday|Tuesday|Wednesday|Thursday|Friday|Saturday|Sunday)|on (?:Monday|Tuesday|Wednesday|Thursday|Friday|Saturday|Sunday))\b/i,
  /\b(?:only person|one of (?:a few|two|three)|rare condition|small village)\b/i
];
const urgent = /\b(?:suicide|suicidal|self.harm|kill myself|end my life|abuse|safeguarding|immediate danger|violence|overdose)\b/i;

export function screenText(value) {
  // A *demonstration* of direct-ID detection, not an anonymiser.
  let screened = String(value || "").slice(0, 16000);
  const found = [];
  for (const pattern of directPatterns) {
    if (new RegExp(pattern.re.source, pattern.re.flags).test(screened)) {
      found.push(pattern.label);
    }
    screened = screened.replace(pattern.re, pattern.replacement);
  }
  const possibleIndirectIdentification = indirect.some(re => re.test(value || ""));
  return {
    screened,
    found,
    possibleIndirectIdentification,
    urgent: urgent.test(value || ""),
    requiresHumanReview: Boolean(found.length || possibleIndirectIdentification)
  };
}

const frameworks = {
  oars: [
    ["What you noticed", "Start with what was actually said or done, rather than assuming why it happened."],
    ["Try an open question", "“What would make the next step feel possible for you?”"],
    ["A reflective response", "“Part of you wants a change, and another part is understandably unsure.”"],
    ["For supervision", "Did I listen for change talk, or rush to solve the problem?"]
  ],
  comb: [
    ["Capability", "What knowledge, energy, skills or confidence might be relevant? Ask, don't infer."],
    ["Opportunity", "What external constraints, access needs or practical supports deserve attention?"],
    ["Motivation", "What matters to the person, and what feels competing or uncertain?"],
    ["Try asking", "“Which obstacle would be most useful to explore first?”"]
  ],
  strengths: [
    ["Find existing strengths", "What has already helped this fictional person make progress, however small?"],
    ["Make it theirs", "Let the person set the goal in their own language."],
    ["Small next step", "Explore one realistic, self-chosen action instead of prescribing a plan."],
    ["Try asking", "“What would a one-point improvement look like this week?”"]
  ],
  supervision: [
    ["Separate facts and interpretation", "What did I observe, what did I infer, and what remains unknown?"],
    ["Check my response", "What feelings or assumptions did this interaction bring up in me?"],
    ["Alternative explanation", "What else could explain the situation, including structural obstacles?"],
    ["Bring to supervision", "What boundary, uncertainty or decision requires support from my supervisor?"]
  ]
};
export function coachingCards(mode) {
  return frameworks[mode] || frameworks.oars;
}
export function isRiskEscalation(value) { return urgent.test(String(value || "")); }
