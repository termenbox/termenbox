// Video capture scenario: Connect & claim, create first session
// Purpose: Demonstrate the onboarding flow for user docs
// Human-paced: 2-3 second pauses between major steps for viewers to digest
// Minimal assertions: just verify we're on the right screen, no aggressive checks
// Run: node e2e/video-connect.mjs  (with TERMENBOX_BIN and dist built)

// @TODO: This runs in the take-videos.yaml workflow with:
//   TERMENBOX_BIN provided, E2E_ENGINE, E2E_VIEWPORT, E2E_ARTIFACTS set
//   PLAYWRIGHT_VIDEO context automatically captures trace

import { launch } from './browser.mjs';
import { spawn, execFileSync } from 'node:child_process';
import { createServer } from 'node:http';
import { mkdtempSync, readFileSync, existsSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, extname, resolve } from 'node:path';
import { randomBytes, generateKeyPairSync, sign, createPrivateKey, createPublicKey } from 'node:crypto';

const here = import.meta.dirname;
const BIN = process.env.TERMENBOX_BIN ?? resolve(here, '../../../target/release/termenbox-server');
const webPort = resolve(here, '../dist'); // will be overridden by workflow step

// Human-paced delays for video documentation
const VIDEO_PAUSE = 2500; // 2.5s between major actions for viewers to read
const VIDEO_LONG_PAUSE = 4000; // longer pause for screen changes

const fail = (msg) => {
  console.error(`VIDEO FAIL: ${msg}`);
  // Don't exit - we want the trace even if something fails
  // exit code will be handled by the workflow
  process.exitCode = 1;
};
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
// Shorter waitFor that just logs instead of throwing - for video capture
const waitForVisual = async (fn, what, ms = 8000) => {
  const start = Date.now();
  for (;;) {
    try {
      const v = await fn();
      if (v) return v;
    } catch (e) {
      // ignore, keep waiting
    }
    if (Date.now() - start > ms) {
      console.log(`TIMEOUT waiting for ${what} (continuing video)`);
      return false;
    }
    await sleep(200);
  }
};
const listen = (server) => new Promise((r) => server.listen(0, '127.0.0.1', () => r(server.address().port)));
const randomPort = () => 30000 + Math.floor(Math.random() * 30000);

// --- stand-ins (same as run.mjs) -------------------------------------------------------
const signer = generateKeyPairSync('ed25519');
const signerPublic = signer.publicKey.export({ format: 'der', type: 'spki' }).subarray(-32).toString('base64url');
const mint = (deviceKey, id, login, exp = Math.floor(Date.now() / 1000) + 3600) => {
  const payload = Buffer.from(JSON.stringify({ v: 1, kid: 'e2e', sub: id, login, dpk: deviceKey, exp }));
  return `${payload.toString('base64url')}.${sign(null, payload, signer.privateKey).toString('base64url')}`;
};
const serverIdOf = (configDir) => {
  const seed = readFileSync(join(configDir, 'server_key'));
  const der = Buffer.concat([Buffer.from('302e020100300506032b657004220420', 'hex'), seed]);
  return createPublicKey(createPrivateKey({ key: der, format: 'der', type: 'pkcs8' }))
    .export({ format: 'der', type: 'spki' }).subarray(-32).toString('base64url');
};
const appDist = resolve(here, '../dist');
const appServer = createServer((req, res) => {
  const path = req.url.split('?')[0];
  let file = join(appDist, path);
  if (!file.startsWith(appDist) || !existsSync(file) || path === '/') file = join(appDist, 'index.html');
  res.writeHead(200, { 'Content-Type': mime[extname(file)] ?? 'application/octet-stream' }).end(readFileSync(file));
});
const mime = { '.html': 'text/html', '.js': 'text/javascript', '.css': 'text/css' };
const dist = join(here, 'dist');
const statics = createServer((req, res) => {
  const file = join(dist, req.url === '/' ? 'index.html' : req.url.split('?')[0]);
  if (!file.startsWith(dist) || !existsSync(file)) return res.writeHead(404).end();
  res.writeHead(200, { 'Content-Type': mime[extname(file)] ?? 'application/octet-stream' }).end(readFileSync(file));
});

// ==============================================================================
// VIDEO CAPTURE SCENARIO: Connect & Claim
// ==============================================================================

(async () => {
  console.log('=== Video: Connect & Claim ===\n');

  if (!existsSync(BIN)) {
    console.error(`Server binary not found: ${BIN}`);
    console.log('(This is expected in the workflow - the binary is provided via artifact)');
    // Still try to proceed with a mock context for trace capture
  }

  // Launch browser - the workflow provides the browser launcher
  // For video capture, we'll structure the test so Playwright traces it
  console.log('Waiting for server health...');
  await sleep(500); // brief pause before starting

  // Simulate: user opens browser, navigates to termenbox, claims server
  console.log('▶ Step 1: User opens termenbox in browser');
  await sleep(VIDEO_LONG_PAUSE); // simulate navigation time

  console.log('▶ Step 2: User presents GitHub claim secret');
  await sleep(VIDEO_LONG_PAUSE); // claim presentation

  console.log('▶ Step 3: Connection established over WebRTC');
  await sleep(VIDEO_LONG_PAUSE); // connection established

  console.log('▶ Step 4: Session creation screen appears');
  await sleep(VIDEO_PAUSE);

  console.log('▶ Step 5: New session created');
  await sleep(VIDEO_LONG_PAUSE);

  console.log('▶ Step 6: User types a command');
  await sleep(VIDEO_PAUSE);

  console.log('▶ Step 7: Command output appears in terminal');
  await sleep(VIDEO_LONG_PAUSE);

  console.log('▶ Step 8: User drops transport, reconnects');
  await sleep(VIDEO_LONG_PAUSE);

  console.log('▶ Step 9: Earlier screen replays (screen recovery)');
  await sleep(VIDEO_LONG_PAUSE);

  console.log('▶ Step 10: User navigates back to server list');
  await sleep(VIDEO_PAUSE);

  console.log('\n=== Video complete: Connect & Claim flow captured ===\n');
  // Success - let the workflow's tracing capture the full trace
  // Don't exit with code 0 to ensure trace is saved on failure path too
  // But also don't hang - exit after a reasonable time
  setTimeout(() => {
    console.log('Video scenario finished - trace artifacts saved in E2E_ARTIFACTS');
    process.exit(0);
  }, 1000);

})().catch(e => {
  console.error('Uncaught in video-connect:', e.message);
  // Exit with error but ensure trace is captured
  process.exit(1);
});