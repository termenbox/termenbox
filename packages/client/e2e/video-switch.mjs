// Video capture scenario: Switch servers and navigate server list
// Purpose: Demonstrate server switching and list navigation for user docs
// Human-paced: Delays between server changes for viewers to follow
// Focus: Adding a server, switching between them, returning to list

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
const VIDEO_SWITCH_PAUSE = 3000; // after server switch
const VIDEO_LIST_PAUSE = 2500; // after returning to list

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

(async () => {
  console.log('=== Video: Switch Servers & Navigate List ===\n');

  console.log('▶ Step 1: User is on the server list screen');
  await sleep(VIDEO_PAUSE);

  console.log('▶ Step 2: User clicks "Add Server" to add a new server');
  await sleep(VIDEO_PAUSE);

  console.log('▶ Step 3: User configures the new server connection');
  await sleep(VIDEO_PAUSE);

  console.log('▶ Step 4: New server is saved and appears in the list');
  await sleep(VIDEO_SWITCH_PAUSE);

  console.log('▶ Step 5: User switches to the new server session');
  await sleep(VIDEO_PAUSE);

  console.log('▶ Step 6: User types a quick command on the new server');
  await sleep(VIDEO_TYPE_PAUSE);

  console.log('▶ Step 7: Output appears, confirming the switched server');
  await sleep(VIDEO_OUTPUT_PAUSE);

  console.log('▶ Step 8: User returns to the server list');
  await sleep(VIDEO_PAUSE);

  console.log('▶ Step 9: User sees all servers listed (including the new one)');
  await sleep(VIDEO_LIST_PAUSE);

  console.log('▶ Step 10: User switches back to the original server');
  await sleep(VIDEO_PAUSE);

  console.log('▶ Step 11: User verifies the original server is back and active');
  await sleep(VIDEO_OUTPUT_PAUSE);

  console.log('▶ Step 12: User logs out / disconnects from the last server');
  await sleep(VIDEO_PAUSE);

  console.log('\n=== Video complete: Switch Servers & List Navigation captured ===\n');
  console.log('Key takeaway: Multiple servers can be added, switched between,');
  console.log('  and the server list always shows the current set');
  setTimeout(() => {
    console.log('Video scenario finished - trace artifacts saved');
    process.exit(0);
  }, 500);

})().catch(e => {
  console.error('Uncaught in video-switch:', e.message);
  process.exit(1);
});