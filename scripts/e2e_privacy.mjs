#!/usr/bin/env node

/*
 * Deterministic privacy E2E test.
 *
 * Requirements:
 *   - Rust/Cargo
 *   - Node.js/npm (npm ci installs playwright-core)
 *   - Chromium/Chrome (or E2E_BROWSER_PATH=/path/to/chromium)
 *
 * The app, completion provider, embedding provider, database, credentials,
 * and browser authenticator are all local to this process. No API key in the
 * user's .env file is read.
 */

import assert from 'node:assert/strict';
import { spawn, spawnSync } from 'node:child_process';
import { createServer } from 'node:http';
import { mkdirSync, rmSync, existsSync, readFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { chromium } from 'playwright-core';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const port = await freePort();
const providerPort = await freePort();
const baseUrl = `http://localhost:${port}`;
const providerUrl = `http://127.0.0.1:${providerPort}/v1`;
const e2eDir = join(root, 'target', 'e2e');
const dbPath = join(e2eDir, 'privacy.sqlite');
const canary = process.env.E2E_CANARY || 'E2E-SECRET-847291';
const email = 'e2e-deterministic@example.test';
const dbKey = 'e2e-db-key-2026-07-13-deterministic-long';
const cookieSecret = 'e2e-cookie-secret-2026-07-13-deterministic-long';

let app;
let browser;
let context;
let page;
let provider;
let recoveryKey = '';
const providerRequests = { chat: 0, embeddings: 0 };
const appOutput = [];
const providerShapes = [];

function log(message) {
  process.stdout.write(`[e2e] ${message}\n`);
}

function sleep(ms) {
  return new Promise((resolvePromise) => setTimeout(resolvePromise, ms));
}

async function freePort() {
  const server = createServer();
  await new Promise((resolvePromise, reject) => {
    server.once('error', reject);
    server.listen(0, '127.0.0.1', resolvePromise);
  });
  const address = server.address();
  const value = address.port;
  await new Promise((resolvePromise) => server.close(resolvePromise));
  return value;
}

async function readBody(request) {
  const chunks = [];
  for await (const chunk of request) chunks.push(chunk);
  return Buffer.concat(chunks).toString('utf8');
}

function jsonResponse(response, value) {
  const body = JSON.stringify(value);
  response.writeHead(200, {
    'content-type': 'application/json',
    'content-length': Buffer.byteLength(body),
  });
  response.end(body);
}

function schemaResponse(body) {
  const schema = body?.response_format?.json_schema?.schema
    || body?.response_format?.schema
    || body?.tools?.find((tool) => tool?.function?.name === 'submit')?.function?.parameters;
  const properties = schema?.properties || {};

  if (properties.episodes) {
    return {
      episodes: [{
        id: 'e2e_concrete_event',
        title: 'Deterministic E2E event',
        narrative: `A deterministic test recorded ${canary}.`,
        occurred_at: '2026-01-01',
        participants: ['self'],
        concepts: ['e2e_pattern'],
        user_quotes: [canary],
      }],
    };
  }
  if (properties.new_concepts) {
    return {
      new_concepts: [{ id: 'e2e_pattern', label: 'E2E pattern', category: 'Pattern' }],
      new_connections: [],
      obsolete_concept_ids: [],
      obsolete_connections: [],
      person_links: [],
    };
  }
  if (properties.profiles) {
    return {
      profiles: [{
        slug: 'e2e_friend',
        display_name: 'E2E Friend',
        relationship_type: 'friend',
        background: 'A deterministic test relationship.',
        goals: [],
        triggers: [],
        do_not_say: [],
        effective_tone: [],
        recent_events: [],
        boundaries: [],
      }],
    };
  }
  if (properties.relationships) {
    return {
      relationships: [{
        from_slug: 'self',
        from_label: 'Self',
        to_slug: 'e2e_friend',
        to_label: 'E2E Friend',
        relation: 'knows',
        evidence: 'Deterministic E2E relationship.',
      }],
    };
  }
  if (properties.title && properties.preview) {
    return { title: 'Deterministic E2E chat', preview: 'A deterministic privacy test.' };
  }

  // Keep this fallback valid for any future structured extractor added to the
  // app while making the important existing extractors explicit above.
  const value = {};
  for (const [name, property] of Object.entries(properties)) {
    if (property.type === 'array') value[name] = [];
    else if (property.type === 'boolean') value[name] = false;
    else if (property.type === 'integer' || property.type === 'number') value[name] = 0;
    else value[name] = '';
  }
  return value;
}

function completionResponse(body) {
  const submitTool = body?.tools?.some((tool) => tool?.function?.name === 'submit');
  const content = submitTool ? null : 'Deterministic therapist response.';
  const message = submitTool
    ? {
        role: 'assistant',
        content: null,
        tool_calls: [{
          id: 'call_e2e_submit',
          type: 'function',
          function: { name: 'submit', arguments: JSON.stringify(schemaResponse(body)) },
        }],
      }
    : { role: 'assistant', content };
  return {
    id: 'e2e-completion',
    object: 'chat.completion',
    created: 1704067200,
    model: body?.model || 'e2e/deterministic',
    choices: [{
      index: 0,
      message,
      finish_reason: 'stop',
    }],
    usage: { prompt_tokens: 1, completion_tokens: 1, total_tokens: 2 },
  };
}

function writeStreamingCompletion(response, body) {
  const completion = completionResponse(body);
  const text = completion.choices[0].message.content;
  response.writeHead(200, {
    'content-type': 'text/event-stream',
    'cache-control': 'no-cache',
    connection: 'keep-alive',
  });
  for (const chunk of [text.slice(0, Math.ceil(text.length / 2)), text.slice(Math.ceil(text.length / 2))]) {
    response.write(`data: ${JSON.stringify({
      id: completion.id,
      model: completion.model,
      choices: [{ index: 0, delta: { content: chunk }, finish_reason: null }],
    })}\n\n`);
  }
  response.write(`data: ${JSON.stringify({
    id: completion.id,
    model: completion.model,
    choices: [{ index: 0, delta: {}, finish_reason: 'stop' }],
    usage: completion.usage,
  })}\n\n`);
  response.end('data: [DONE]\n\n');
}

function startProvider() {
  provider = createServer(async (request, response) => {
    if (request.method !== 'POST') {
      response.writeHead(404);
      response.end();
      return;
    }
    const body = JSON.parse(await readBody(request));
    providerShapes.push({
      path: request.url,
      stream: Boolean(body.stream),
      tools: (body.tools || []).map((tool) => tool?.function?.name || tool?.name || 'unknown'),
      hasBodyContext: JSON.stringify(body.messages || []).includes('<body_context>'),
    });
    if (request.url?.endsWith('/embeddings')) {
      providerRequests.embeddings += 1;
      jsonResponse(response, {
        object: 'list',
        model: body.model || 'e2e-embedding',
        data: [{ object: 'embedding', index: 0, embedding: [1, 0, 0, 0, 0, 0, 0, 0] }],
        usage: { prompt_tokens: 1, total_tokens: 1 },
      });
      return;
    }
    if (request.url?.endsWith('/chat/completions')) {
      providerRequests.chat += 1;
      if (body.stream) writeStreamingCompletion(response, body);
      else jsonResponse(response, completionResponse(body));
      return;
    }
    response.writeHead(404);
    response.end();
  });
  return new Promise((resolvePromise, reject) => {
    provider.once('error', reject);
    provider.listen(providerPort, '127.0.0.1', resolvePromise);
  });
}

const appEnv = {
  ...process.env,
  BILLING_ENABLED: 'false',
  PORT: String(port),
  RP_ID: 'localhost',
  RP_ORIGIN: baseUrl,
  MEMORY_DB_PATH: dbPath,
  MEMORY_DB_KEY: dbKey,
  COOKIE_SECRET: cookieSecret,
  OPENAI_API_KEY: 'e2e-openai-key',
  OPENAI_BASE_URL: providerUrl,
  OPENROUTER_API_KEY: 'e2e-openrouter-key',
  OPENROUTER_BASE_URL: providerUrl,
  OPENROUTER_MODEL: 'e2e/deterministic',
  GRAPH_EXTRACTOR_MODEL: 'e2e/graph',
  EPISODE_EXTRACTOR_MODEL: 'e2e/episode',
  RELATIONSHIP_PROFILE_MODEL: 'e2e/profile',
  SOCIAL_RELATIONSHIP_MODEL: 'e2e/social',
  SESSION_SUMMARY_MODEL: 'e2e/summary',
  EMBEDDING_MODEL: 'e2e/embedding',
  RUST_LOG: 'error',
};

function removeTestDatabase() {
  mkdirSync(e2eDir, { recursive: true });
  for (const suffix of ['', '-wal', '-shm', '.plaintext.bak', '.encrypting']) {
    rmSync(`${dbPath}${suffix}`, { force: true });
  }
}

function buildBinaries() {
  log('building app and SQLCipher inspector');
  const result = spawnSync('cargo', ['build', '--quiet', '--bin', 'individuateai', '--bin', 'e2e_inspect'], {
    cwd: root,
    env: appEnv,
    stdio: 'inherit',
  });
  if (result.status !== 0) throw new Error('cargo build failed');
}

function startApp() {
  app = spawn(join(root, 'target', 'debug', 'individuateai'), [], {
    cwd: root,
    env: appEnv,
    stdio: ['ignore', 'pipe', 'pipe'],
  });
  app.stdout.on('data', (chunk) => appOutput.push(chunk.toString()));
  app.stderr.on('data', (chunk) => appOutput.push(chunk.toString()));
  app.once('exit', (code, signal) => {
    if (code !== 0 && signal !== 'SIGTERM') log(`app exited unexpectedly (${code ?? signal})`);
  });
}

async function stopApp() {
  if (!app || app.exitCode !== null) return;
  const child = app;
  child.kill('SIGTERM');
  await Promise.race([
    new Promise((resolvePromise) => child.once('exit', resolvePromise)),
    sleep(3000),
  ]);
  if (child.exitCode === null) child.kill('SIGKILL');
  app = null;
}

async function waitForApp() {
  const deadline = Date.now() + 15000;
  while (Date.now() < deadline) {
    try {
      const response = await fetch(`${baseUrl}/login`);
      if (response.status === 200) return;
    } catch (_) {
      // The binary may still be starting.
    }
    await sleep(100);
  }
  throw new Error('app did not become ready');
}

async function whoami() {
  return page.evaluate(async () => {
    const response = await fetch('/api/whoami');
    return { status: response.status, body: await response.json() };
  });
}

async function assertAuthenticated() {
  const result = await whoami();
  assert.equal(result.status, 200);
  assert.ok(result.body?.id);
}

async function assertHistoryContains(sessionId) {
  const result = await page.evaluate(async (id) => {
    const response = await fetch(`/api/sessions/${id}/history`);
    return { status: response.status, body: await response.json() };
  }, sessionId);
  assert.equal(result.status, 200);
  assert.ok(result.body.some((message) => message.content.includes(canary)));
}

async function waitForMemoryWrite() {
  const deadline = Date.now() + 45000;
  while (Date.now() < deadline) {
    const result = await page.evaluate(async () => {
      const response = await fetch('/api/memory-status');
      return { status: response.status, body: await response.json() };
    });
    if (result.status === 200 && result.body.episode_count > 0 && result.body.memory_signature) return;
    await sleep(250);
  }
  throw new Error('encrypted background memory write did not complete');
}

async function registerPasskey() {
  log('registering a passkey in Chromium virtual authenticator');
  await page.goto(`${baseUrl}/signup`);
  await page.locator('#signup-email').fill(email);
  await page.locator('#recovery-warning-ack').check();
  await page.locator('#create-passkey-btn').click();
  await page.waitForURL((url) => ['/','/chat'].includes(new URL(url).pathname));
  await assertAuthenticated();
  const syncBanner = page.locator('#passkey-sync-banner');
  if (await syncBanner.isVisible()) {
    await syncBanner.getByRole('button', { name: 'Enable iCloud sync' }).click();
    await syncBanner.waitFor({ state: 'hidden' });
  }
}

async function loginWithPasskey() {
  await page.goto(`${baseUrl}/login`);
  await page.locator('#passkey-login-btn').click();
  await Promise.race([
    page.waitForURL((url) => ['/','/chat','/recovery'].includes(new URL(url).pathname), { timeout: 12000 }),
    page.locator('#passkey-login-error:not(.hidden)').waitFor({ state: 'visible', timeout: 12000 }),
  ]);
  if (new URL(page.url()).pathname === '/login') {
    throw new Error(`passkey login failed: ${await page.locator('#passkey-login-error').textContent()}`);
  }
  if (new URL(page.url()).pathname === '/recovery') {
    assert.ok(recoveryKey, 'recovery key was not captured at registration');
    await page.locator('#email').fill(email);
    await page.locator('#recovery-key').fill(recoveryKey);
    await page.locator('#recovery-submit').click();
    await page.waitForURL((url) => new URL(url).pathname === '/login');
    await page.locator('#passkey-login-btn').click();
    await page.waitForURL((url) => ['/','/chat'].includes(new URL(url).pathname));
  }
  await assertAuthenticated();
}

async function sendChat() {
  log('sending deterministic chat canary');
  await page.locator('#chat-input').fill(`A concrete event happened today. Preserve this exact marker: ${canary}`);
  await page.locator('#seed-send').click();
  await page.waitForFunction((marker) => [...document.querySelectorAll('.bubble-user')]
    .some((element) => element.textContent.includes(marker)), canary);
  await page.waitForFunction(() => [...document.querySelectorAll('.bubble-therapist')]
    .some((element) => element.textContent.includes('Deterministic therapist response.')), null, { timeout: 15000 });
  const sessionId = await page.locator('#app-shell').getAttribute('data-session-id');
  assert.ok(sessionId);
  await assertHistoryContains(sessionId);
  await waitForMemoryWrite();
  return sessionId;
}

async function testCycleTracking() {
  log('testing optional cycle tracking at iPhone SE size');
  await page.setViewportSize({ width: 375, height: 667 });
  await page.goto(`${baseUrl}/cycle`);
  await page.locator('#cycle-setup').waitFor({ state: 'visible' });
  const startDate = await page.evaluate(() => {
    const date = new Date();
    date.setDate(date.getDate() - 28);
    return new Date(date.getTime() - date.getTimezoneOffset() * 60000).toISOString().slice(0, 10);
  });
  await page.locator('#setup-last-start').fill(startDate);
  await page.locator('#setup-cycle-days').fill('28');
  await page.getByRole('button', { name: 'Enable private tracking' }).click();
  await page.waitForURL((url) => new URL(url).pathname === '/cycle');
  await page.locator('#cycle-dashboard').waitFor({ state: 'visible' });
  assert.equal(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth), true);

  await page.getByRole('button', { name: 'Period started', exact: true }).click();
  await page.getByText('Period start recorded.').waitFor();
  await page.getByRole('heading', { name: 'Recorded bleeding window' }).waitFor();

  await page.getByRole('button', { name: 'Daily check-in' }).click();
  await page.locator('input[name="mood"][value="2"] + span').click();
  await page.locator('input[name="sensitivity"][value="4"] + span').click();
  await page.locator('input[name="sleep"][value="2"] + span').click();
  await page.getByRole('button', { name: 'Save check-in' }).click();
  await page.getByText('Check-in saved.').waitFor();
  await page.locator('#cycle-observations').getByText('Sensitivity').waitFor();

  const api = await page.evaluate(async () => {
    const response = await fetch(`/api/cycle?today=${new Date().toISOString().slice(0, 10)}`);
    return { status: response.status, body: await response.json() };
  });
  assert.equal(api.status, 200);
  assert.equal(api.body.profile.enabled, true);
  assert.ok(api.body.events.length >= 2);

  const undersizedControls = await page.locator('#cycle-dashboard button:visible, #cycle-dashboard a:visible')
    .evaluateAll((controls) => controls
      .filter((control) => {
        const box = control.getBoundingClientRect();
        return box.width < 44 || box.height < 44;
      })
      .map((control) => control.textContent.trim() || control.getAttribute('aria-label') || control.tagName));
  assert.deepEqual(undersizedControls, [], `cycle controls below 44px: ${undersizedControls.join(', ')}`);

  await page.setViewportSize({ width: 667, height: 375 });
  await page.reload();
  await page.locator('#cycle-dashboard').waitFor({ state: 'visible' });
  assert.equal(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth), true);
  await page.setViewportSize({ width: 375, height: 667 });

  await page.goto(`${baseUrl}/chat`);
  await page.locator('#chat-input').fill('My period started yesterday.');
  await page.locator('#seed-send').click();
  await page.locator('.cycle-chat-suggestion').waitFor({ state: 'visible', timeout: 15000 });
  assert.equal(await page.getByText('Nothing is saved until you choose Log it.').isVisible(), true);
  await page.locator('.cycle-chat-suggestion [data-action="dismiss"]').click();
  assert.ok(providerShapes.some((shape) => shape.hasBodyContext), 'therapist request did not contain body context');
  log('cycle UI, explicit chat confirmation, and AI body context passed');
}

async function logout() {
  await page.goto(`${baseUrl}/api/logout`);
  await page.waitForURL((url) => new URL(url).pathname === '/login');
  const result = await whoami();
  assert.equal(result.status, 401);
}

async function inspectDatabase() {
  const result = spawnSync(join(root, 'target', 'debug', 'e2e_inspect'), [dbPath, canary], {
    cwd: root,
    env: appEnv,
    encoding: 'utf8',
  });
  process.stdout.write(result.stdout || '');
  if (result.status !== 0) {
    process.stderr.write(result.stderr || '');
    throw new Error('database encryption assertions failed');
  }
  const report = JSON.parse(result.stdout.trim());
  assert.equal(report.unkeyed_readable, false);
  assert.equal(report.plaintext_messages, 0);
  assert.equal(report.plaintext_sessions, 0);
  assert.equal(report.raw_files_contain_canary, false);
  assert.ok(report.encrypted_messages > 0);
  assert.ok(report.encrypted_memory_rows > 0);
  assert.ok(report.encrypted_episodes > 0);
  assert.ok(report.encrypted_cycle_profiles > 0);
  assert.ok(report.encrypted_cycle_events > 0);
}

function browserPath() {
  const candidates = [
    process.env.E2E_BROWSER_PATH,
    '/usr/bin/google-chrome',
    '/usr/bin/chromium',
    '/snap/bin/chromium',
  ].filter(Boolean);
  const path = candidates.find((candidate) => existsSync(candidate));
  if (!path) throw new Error('Chromium not found; set E2E_BROWSER_PATH');
  return path;
}

async function main() {
  removeTestDatabase();
  await startProvider();
  buildBinaries();
  startApp();
  await waitForApp();

  browser = await chromium.launch({
    headless: true,
    executablePath: browserPath(),
    args: ['--no-sandbox'],
  });
  context = await browser.newContext();
  page = await context.newPage();
  page.on('dialog', (dialog) => {
    if (dialog.type() === 'prompt' && dialog.message().includes('recovery key')) {
      recoveryKey = dialog.message().split('\n\n').at(-1).trim();
    }
    dialog.accept();
  });
  page.on('pageerror', (error) => log(`page error: ${error.message}`));
  const cdp = await context.newCDPSession(page);
  await cdp.send('WebAuthn.enable');
  await cdp.send('WebAuthn.addVirtualAuthenticator', {
    options: {
      protocol: 'ctap2',
      ctap2Version: 'ctap2_1',
      transport: 'internal',
      hasResidentKey: true,
      hasUserVerification: true,
      isUserVerified: true,
      hasPrf: true,
      hasLargeBlob: true,
    },
  });

  await registerPasskey();
  await testCycleTracking();
  const sessionId = await sendChat();
  log('passkey registration, chat, and encrypted memory write passed');
  await logout();
  log('logout isolation passed');
  await loginWithPasskey();
  await assertHistoryContains(sessionId);
  log('passkey login passed');

  await stopApp();
  startApp();
  await waitForApp();
  await page.goto(baseUrl);
  await assertAuthenticated();
  await assertHistoryContains(sessionId);
  log('process restart persistence passed');
  await logout();
  await loginWithPasskey();
  await assertHistoryContains(sessionId);
  await stopApp();
  await inspectDatabase();
  assert.ok(providerRequests.chat > 0, 'local completion stub was not called');
  assert.ok(providerRequests.embeddings > 0, 'local embedding stub was not called');
  log(`all assertions passed (local chat requests: ${providerRequests.chat}, embeddings: ${providerRequests.embeddings})`);
}

try {
  await main();
} catch (error) {
  process.stderr.write(`[e2e] FAILED: ${error.stack || error}\n`);
  const diagnostic = appOutput.join('').trim();
  if (diagnostic) process.stderr.write(`[e2e] app output:\n${diagnostic.slice(-8000)}\n`);
  process.stderr.write(`[e2e] provider request shapes: ${JSON.stringify(providerShapes.slice(-12))}\n`);
  process.exitCode = 1;
} finally {
  await stopApp();
  if (browser) await browser.close();
  if (provider) await new Promise((resolvePromise) => provider.close(resolvePromise));
}
