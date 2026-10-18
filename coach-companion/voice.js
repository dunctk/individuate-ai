import { screenText, coachingCards } from "./logic.js";
const $ = selector => document.querySelector(selector);
const talk = $("#talk");
const status = $("#status");
const micTitle = $("#mic-title");
const transcript = $("#transcript");
const transcriptPanel = $("#transcript-panel");
const insights = $("#insights");
const grid = $("#insight-grid");
const riskBox = $("#risk");
const clear = $("#clear");
const readInsights = $("#read-insights");
let engine = null;
let active = false;
let mode = "oars";
let text = "";
let genericReadout = "";

function statusMessage(title, body) {
  micTitle.textContent = title;
  status.textContent = body;
}
function setActive(next) {
  active = next;
  talk.setAttribute("aria-pressed", String(next));
  talk.setAttribute("aria-label", next ? "Stop voice reflection" : "Begin a fictional reflection");
}
function stopReadout() { if ("speechSynthesis" in window) speechSynthesis.cancel(); }
function clearAll() {
  if (active && engine) { engine.abort(); }
  setActive(false); stopReadout();
  engine = null; text = ""; genericReadout = "";
  transcript.textContent = ""; transcriptPanel.hidden = true;
  insights.hidden = true; riskBox.hidden = true; grid.replaceChildren();
  statusMessage("Ready to reflect", "Use a fictional scenario. Tap to talk on this device.");
}
function renderScreened(textValue) {
  const screened = screenText(textValue);
  transcript.textContent = screened.screened;
  transcriptPanel.hidden = false;
  return screened;
}
function addCard(heading, content) {
  const card = document.createElement("article");
  card.className = "insight-card";
  const h = document.createElement("h3");
  h.textContent = heading;
  const p = document.createElement("p");
  p.textContent = content;
  card.append(h, p);
  grid.append(card);
}
function renderReflection() {
  if (!text.trim()) return;
  const screened = renderScreened(text);
  insights.hidden = false;
  grid.replaceChildren(); riskBox.hidden = true;
  if (screened.urgent) {
    riskBox.hidden = false;
    riskBox.textContent = "Safeguarding or immediate risk language detected. For a real situation, follow your organisation's urgent safeguarding/escalation processes. Do not rely on this prototype.";
  } else if (screened.requiresHumanReview) {
    riskBox.hidden = false;
    riskBox.textContent = "Potential identifying details detected or suggested. This demo is NOT an anonymisation tool. Do not share or reuse the transcript, and do not speak real cases here.";
  }
  const cards = coachingCards(mode);
  // Never include user transcript in generated cards or readout.
  for (const [title,content] of cards) addCard(title,content);
  genericReadout = cards.map(([title,content]) => title + ". " + content).join(" ");
}
function engineAvailable() {
  const SR = window.SpeechRecognition;
  if (typeof SR !== "function" || typeof SR.available !== "function" || typeof SR.install !== "function") return false;
  try { return "processLocally" in new SR(); } catch { return false; }
}
function failClosed(message) {
  talk.disabled = true;
  setActive(false);
  statusMessage("On-device speech unavailable", message);
}
async function ensureLocalLanguagePack() {
  const SR = window.SpeechRecognition;
  let state;
  try { state = await SR.available({langs:["en-GB"], processLocally:true}); }
  catch { throw new Error("Browser could not check offline recognition. No cloud fallback is allowed."); }
  if (state === "available") return true;
  if (state === "downloadable" || state === "downloading") {
    statusMessage("Preparing offline speech", "Installing an on-device English language pack…");
    try {
      const installed = await SR.install({langs:["en-GB"], processLocally:true});
      if (installed) { statusMessage("Offline speech installed", "Tap the microphone again to start."); return false; }
    } catch { /* Fail closed below */ }
    throw new Error("The offline language pack could not be installed.");
  }
  throw new Error("Offline English speech recognition is unavailable. Never switch to cloud recognition for case material.");
}
async function start() {
  if (!engineAvailable()) {
    failClosed("Use a browser that explicitly supports processLocally and offline language packs. This demo does not use server recognition.");
    return;
  }
  talk.disabled = true;
  try {
    const ready = await ensureLocalLanguagePack();
    if (!ready) return;
    stopReadout(); text = ""; genericReadout = "";
    transcriptPanel.hidden = true; insights.hidden = true; grid.replaceChildren(); riskBox.hidden = true;
    const rec = new window.SpeechRecognition();
    rec.lang = "en-GB";
    rec.continuous = true;
    rec.interimResults = true;
    rec.processLocally = true; // Non-negotiable. Never toggle false.
    engine = rec;
    rec.onresult = event => {
      // Rebuild the complete transcript so repeated result events cannot duplicate segments.
      let final = "", interim = "";
      for (const result of event.results) {
        if (result.isFinal) final += result[0].transcript + " ";
        else interim += result[0].transcript + " ";
      }
      text = (final + interim).trim();
      renderScreened(text);
    };
    rec.onerror = event => {
      // Never log audio or text in console or telemetry.
      statusMessage("Microphone stopped", "Local recognition error: " + String(event.error || "unknown"));
    };
    rec.onend = () => {
      setActive(false);
      engine = null;
      if (text.trim()) {
        renderReflection();
        statusMessage("Reflection ready", "Suggestions are general and generated on this device.");
      } else if (micTitle.textContent === "Listening…") {
        statusMessage("Ready to reflect", "I didn't catch anything. Tap to try again.");
      }
    };
    rec.start();
    setActive(true);
    statusMessage("Listening…", "Only the coach should speak. Tap again to finish.");
  } catch (error) {
    setActive(false); engine = null;
    statusMessage("Voice unavailable", error.message || "Local speech recognition unavailable.");
  } finally {
    talk.disabled = false;
  }
}
talk.addEventListener("click", () => {
  if (active && engine) {
    talk.disabled = true;
    statusMessage("Finishing reflection…", "Your local words are becoming practice questions.");
    engine.stop();
    talk.disabled = false;
  } else start();
});
clear.addEventListener("click", clearAll);
document.querySelectorAll("[data-mode]").forEach(btn => btn.addEventListener("click", () => {
  mode = btn.dataset.mode;
  document.querySelectorAll("[data-mode]").forEach(el => {
    const selected = el === btn;
    el.classList.toggle("selected",selected);
    el.setAttribute("aria-pressed",String(selected));
  });
  if (text.trim() && !active) renderReflection();
}));
readInsights.addEventListener("click", () => {
  if (!genericReadout || !("speechSynthesis" in window)) return;
  stopReadout();
  // Only read out generic prewritten coaching cards, NEVER transcript or PII.
  const utterance = new SpeechSynthesisUtterance(genericReadout);
  utterance.lang = "en-GB"; utterance.rate = .94;
  speechSynthesis.speak(utterance);
});
document.addEventListener("visibilitychange", () => {
  if (document.hidden && active && engine) engine.abort();
});
window.addEventListener("pagehide", () => { if(engine) engine.abort(); text = ""; stopReadout(); });
if (engineAvailable()) {
  talk.disabled = false;
  statusMessage("Ready to reflect", "On-device speech supported. Tap to use a fictional example.");
} else {
  failClosed("This browser does not expose verified on-device speech recognition. No cloud or Deepgram fallback.");
}
