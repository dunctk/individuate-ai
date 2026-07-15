#!/usr/bin/env node

import assert from 'node:assert/strict';
import { spawn, spawnSync } from 'node:child_process';
import { createServer } from 'node:http';
import { existsSync, mkdirSync, rmSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { chromium } from 'playwright-core';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const port = await freePort();
const baseUrl = `http://localhost:${port}`;
const testDir = join(root, 'target', 'e2e');
const dbPath = join(testDir, 'paid-signup.sqlite');
const email = 'paid-signup@example.test';

let app;
let browser;
const appOutput = [];

function log(message) {
  process.stdout.write(`[paid-signup-e2e] ${message}\n`);
}

async function freePort() {
  const server = createServer();
  await new Promise((resolvePromise, reject) => {
    server.once('error', reject);
    server.listen(0, '127.0.0.1', resolvePromise);
  });
  const address = server.address();
  await new Promise((resolvePromise) => server.close(resolvePromise));
  return address.port;
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

const appEnv = {
  ...process.env,
  PORT: String(port),
  RP_ID: 'localhost',
  RP_ORIGIN: baseUrl,
  MEMORY_DB_PATH: dbPath,
  MEMORY_DB_KEY: 'paid-signup-e2e-database-key-2026',
  COOKIE_SECRET: 'paid-signup-e2e-cookie-secret-2026-long',
  OPENAI_API_KEY: 'e2e-openai-key',
  OPENROUTER_API_KEY: 'e2e-openrouter-key',
  BILLING_ENABLED: 'true',
  STRIPE_MODE: 'sandbox',
  APP_BASE_URL: baseUrl,
  STRIPE_SANDBOX_SECRET_KEY: 'sk_test_e2e_not_used',
  STRIPE_SANDBOX_WEBHOOK_SECRET: 'whsec_e2e_not_used',
  RUST_LOG: 'error',
};

function resetDatabase() {
  mkdirSync(testDir, { recursive: true });
  for (const suffix of ['', '-wal', '-shm', '.plaintext.bak', '.encrypting']) {
    rmSync(`${dbPath}${suffix}`, { force: true });
  }
}

function buildApp() {
  const result = spawnSync('cargo', ['build', '--quiet', '--bin', 'individuateai'], {
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
}

async function stopApp() {
  if (!app || app.exitCode !== null) return;
  const child = app;
  child.kill('SIGTERM');
  await Promise.race([
    new Promise((resolvePromise) => child.once('exit', resolvePromise)),
    new Promise((resolvePromise) => setTimeout(resolvePromise, 3000)),
  ]);
  if (child.exitCode === null) child.kill('SIGKILL');
}

async function waitForApp() {
  const deadline = Date.now() + 15000;
  while (Date.now() < deadline) {
    try {
      const response = await fetch(`${baseUrl}/signup`);
      if (response.status === 200) return;
    } catch (_) {
      // The server may still be starting.
    }
    await new Promise((resolvePromise) => setTimeout(resolvePromise, 100));
  }
  throw new Error('app did not become ready');
}

async function main() {
  resetDatabase();
  buildApp();
  startApp();
  await waitForApp();

  browser = await chromium.launch({
    headless: true,
    executablePath: browserPath(),
    args: ['--no-sandbox'],
  });
  const context = await browser.newContext();
  const page = await context.newPage();
  page.on('dialog', (dialog) => dialog.accept());

  const cdp = await context.newCDPSession(page);
  await cdp.send('WebAuthn.enable');
  await cdp.send('WebAuthn.addVirtualAuthenticator', {
    options: {
      protocol: 'ctap2',
      transport: 'internal',
      hasResidentKey: true,
      hasUserVerification: true,
      isUserVerified: true,
      hasPrf: true,
    },
  });

  await page.goto(`${baseUrl}/signup`);
  await page.locator('#signup-email').fill(email);
  await page.locator('#recovery-warning-ack').check();
  await page.locator('#create-passkey-btn').click();
  await page.waitForURL((url) => url.pathname === '/subscribe');
  assert.equal(await page.getByText('Step 2 of 2 · Choose your billing').isVisible(), true);
  assert.equal(await page.getByText('Account and passkey created').isVisible(), true);
  assert.equal(await page.getByText('$24.99 monthly').isVisible(), true);
  assert.equal(await page.getByText('€29.99 monthly').isVisible(), false);
  await page.getByRole('button', { name: 'Show euro pricing' }).click();
  assert.equal(await page.getByText('$24.99 monthly').isVisible(), false);
  assert.equal(await page.getByText('€29.99 monthly').isVisible(), true);
  assert.equal(await page.evaluate(() => localStorage.getItem('billing_currency')), 'eur');

  const whoami = await page.evaluate(async () => {
    const response = await fetch('/api/whoami');
    return { status: response.status, body: await response.json() };
  });
  assert.equal(whoami.status, 200);
  assert.equal(whoami.body.username, email);
  log('passkey registration created an authenticated account before billing');

  await page.goto(`${baseUrl}/#pricing`);
  assert.equal(await page.getByText('€29.99').isVisible(), true);
  assert.equal(await page.getByText('$24.99').isVisible(), false);
  log('USD default, EU switch, and currency persistence passed');

  await page.goto(`${baseUrl}/api/logout`);
  await page.waitForURL((url) => url.pathname === '/login');
  await page.goto(`${baseUrl}/subscribe`);
  await page.waitForURL((url) => url.pathname === '/login');

  const anonymousCheckout = await page.evaluate(async () => {
    const response = await fetch('/api/billing/checkout', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ plan: 'usd_monthly' }),
    });
    return response.status;
  });
  assert.equal(anonymousCheckout, 401);
  log('anonymous subscription and Checkout access are blocked');
  log('all paid-signup flow assertions passed');
}

try {
  await main();
} catch (error) {
  process.stderr.write(`[paid-signup-e2e] FAILED: ${error.stack || error}\n`);
  const diagnostic = appOutput.join('').trim();
  if (diagnostic) process.stderr.write(`[paid-signup-e2e] app output:\n${diagnostic.slice(-8000)}\n`);
  process.exitCode = 1;
} finally {
  await stopApp();
  if (browser) await browser.close();
}
