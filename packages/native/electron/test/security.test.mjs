import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';
import { contentSecurityPolicy, isOwnUrl } from '../build/src/security.js';

const SCHEME = 'capacitor-electron';

test('isOwnUrl accepts the app scheme and its paths', () => {
  assert.equal(isOwnUrl(`${SCHEME}://-/index.html`, SCHEME), true);
  assert.equal(isOwnUrl(`${SCHEME}://-/`, SCHEME), true);
});

test('isOwnUrl rejects lookalike hosts, schemes and junk', () => {
  for (const url of [
    'capacitor-electron-evil://-/',
    'https://capacitor-electron.example/',
    'https://example.com/capacitor-electron',
    'javascript:alert(1)',
    'not a url',
  ]) {
    assert.equal(isOwnUrl(url, SCHEME), false, url);
  }
});

test('the packaged policy keeps scripts strict and lets the client connect', () => {
  const csp = contentSecurityPolicy(SCHEME, false);
  assert.match(csp, /script-src capacitor-electron:\/\/\*/);
  assert.doesNotMatch(csp, /script-src[^;]*'unsafe-inline'/);
  assert.match(csp, /style-src[^;]*'unsafe-inline'/);
  // The WebSocket fallback and the assisted signaling path.
  assert.match(csp, /connect-src[^;]*wss:/);
  assert.match(csp, /object-src 'none'/);
});

test('the dev policy allows the live runner', () => {
  const csp = contentSecurityPolicy(SCHEME, true);
  assert.match(csp, /http:\/\/localhost:\*/);
  assert.match(csp, /connect-src[^;]*ws:\/\/localhost:\*/);
  assert.doesNotMatch(csp, /object-src/);
});

test('the packaged policy matches the web client _headers apart from the scheme', () => {
  const headers = readFileSync(new URL('../../../client/public/_headers', import.meta.url), 'utf8');
  const web = /Content-Security-Policy:\s*(.+)/.exec(headers)[1];
  const parse = (policy) => new Map(policy.split(';').map((d) => d.trim().split(/\s+/)).map(([name, ...v]) => [name, v]));
  const app = parse(contentSecurityPolicy(SCHEME, false));
  // form-action differs on purpose: the wrapper has no form to submit.
  for (const [name, values] of parse(web)) {
    if (name === 'form-action') continue;
    const expected = values.map((v) => (v === "'self'" ? `${SCHEME}://*` : v));
    assert.deepEqual(app.get(name), expected, name);
  }
});
