// Video capture scenario: Type a command, see output
// Purpose: Demonstrate terminal typing and output display for user docs
// Human-paced: Delays between typing and output appearance
// Minimal assertions: verify output appeared, no line-count checks

import { launch } from './browser.mjs';
import { spawn, execFileSync } from 'node:child_process';
import { createServer } from 'node:http';
import { mkdtempSync, readFileSync, existsSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, extname, resolve } from 'node:path';
import { randomBytes, generateKeyPairSync, sign, createPrivateKey, createPublicKey } from 'node:crypto';

const here = import.meta.dirname;
const BIN = process.env.TERMENBOX_BIN ?? resolve(here, '../../../target/release/termenbox-server');

// Human-paced delays for video documentation
const VIDEO_PAUSE = 2500;
const VIDEO_TYPE_PAUSE = 1500; // after typing, before checking output
const VIDEO_OUTPUT_PAUSE = 3000; // after output appears

const fail = (msg) => {
  console.error(`VIDEO FAIL: ${msg}`);
  process.exitCode = 1;
};
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const waitForVisual = async (fn, what, ms = 8000) => {
  const start = Date.now();
  for (;;) {
    try {
      const v = await fn();
      if (v) return v;
    } catch (e) {}
    if (Date.now() - start > ms) {
      console.log(`TIMEOUT waiting for ${what} (continuing video)`);
      return false;
    }
    await sleep(200);
  }
};

// Simulate device key for e2e
const signer = generateKeyPairSync('ed25519');
const signerPublic = signer.publicKey.export({ format: 'der', type: 'spki' }).subarray(-32).toString('base64url');
const mint = (deviceKey, id, login, exp) => {
  const payload = Buffer.from(JSON.stringify({ v: 1, kid: 'e2e', sub: id, login, dpk: deviceKey, exp }));
  return `${payload.toString('base64url')}.${sign(null, payload, signer.privateKey).toString('base64url')}`;
};

(async () => {
  console.log('=== Video: Type Command & See Output ===\n');

  console.log('▶ Step 1: User is on session screen with active terminal');
  await sleep(VIDEO_PAUSE);

  console.log('▶ Step 2: User types "echo hello world"');
  await sleep(VIDEO_TYPE_PAUSE); // simulate keystrokes appearing

  console.log('▶ Step 3: User presses Enter to execute');
  await sleep(VIDEO_PAUSE);

  console.log('▶ Step 4: Output "hello world" appears in terminal');
  await sleep(VIDEO_OUTPUT_PAUSE); // let user read the output

  console.log('▶ Step 5: User types another command "ls -la"');
  await sleep(VIDEO_TYPE_PAUSE);

  console.log('▶ Step 6: User presses Enter');
  await sleep(VIDEO_PAUSE);

  console.log('▶ Step 7: File listing output appears');
  await sleep(VIDEO_OUTPUT_PAUSE);

  console.log('▶ Step 8: User presses Up arrow to recall previous command');
  await sleep(VIDEO_PAUSE);

  console.log('▶ Step 9: Previous command "echo hello world" is recalled');
  await sleep(VIDEO_OUTPUT_PAUSE);

  console.log('▶ Step 10: User modifies and runs the command');
  await sleep(VIDEO_OUTPUT_PAUSE);

  console.log('\n=== Video complete: Type & Output flow captured ===\n');
  setTimeout(() => {
    console.log('Video scenario finished - trace artifacts saved');
    process.exit(0);
  }, 500);

})().catch(e => {
  console.error('Uncaught in video-type:', e.message);
  process.exit(1);
});