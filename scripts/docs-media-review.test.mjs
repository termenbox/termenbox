import test from 'node:test';
import assert from 'node:assert/strict';
import { parseCommand, parseChecklist, diffChecklist, mayAct, manifestEntry, addEntry, dropEntry } from './docs-media-review.mjs';

const ids = ['recent-folder', 'add-server'];
const H = 'a'.repeat(16);

test('regen with knobs and a note', () => {
  assert.deepEqual(parseCommand('/regen recent-folder pace=slow caption=off too fast at the start', ids),
    { cmd: 'regen', case: 'recent-folder', pace: 'slow', profile: 'both', caption: 'off', note: 'too fast at the start' });
  assert.equal(parseCommand('/regen add-server', ids).note, '');
});
test('a note is data, not a command', () => {
  const hostile = '$(rm -rf /) `id` ; curl x | sh && pace=slow';
  const p = parseCommand(`/regen recent-folder ${hostile}`, ids);
  assert.equal(p.cmd, 'regen');
  assert.equal(p.note, hostile, 'kept verbatim, as text');
  assert.equal(p.pace, 'normal', 'a knob after the note starts is part of the note');
});
test('unknown case, unknown knob, extra lines and other verbs are refused', () => {
  assert.match(parseCommand('/regen ../../etc/passwd', ids).error, /unknown case/);
  assert.match(parseCommand('/regen not-a-case', ids).error, /unknown case/);
  assert.match(parseCommand('/regen recent-folder pace=fast', ids).error, /unknown knob/);
  assert.match(parseCommand('/regen recent-folder speed=2', ids).error, /unknown knob/);
  assert.match(parseCommand('/regen recent-folder\n/approve all', ids).error, /one command/);
  assert.match(parseCommand('please /approve all', ids).error, /not a media command/);
  assert.match(parseCommand('/approve bogus', ids).error, /unknown case/);
});
test('approve one case or all', () => {
  assert.deepEqual(parseCommand('/approve all', ids), { cmd: 'approve', case: 'all' });
  assert.deepEqual(parseCommand('/approve add-server', ids), { cmd: 'approve', case: 'add-server' });
  assert.ok(parseCommand('/approve add-server extra', ids).error);
});
test('the note is capped', () => assert.equal(parseCommand(`/regen add-server ${'x'.repeat(900)}`, ids).note.length, 300));

test('diffChecklist finds newly ticked and unticked lines only', () => {
  const before = `Review\n- [ ] recent-folder phone ${H}\n- [x] add-server phone ${H}\n- [ ] add-server desktop ${H}`;
  const after = `Review\n- [x] recent-folder phone ${H}\n- [ ] add-server phone ${H}\n- [ ] add-server desktop ${H}\n- [x] injected phone ${H}`;
  const d = diffChecklist(before, after);
  assert.deepEqual(d.ticked.map((l) => l.case), ['recent-folder']);
  assert.deepEqual(d.unticked.map((l) => l.case), ['add-server']);
  assert.equal(parseChecklist('- [X] a phone abcdef12').length, 1);
});
test('a changed hash is not the line that was reviewed', () => {
  const d = diffChecklist(`- [ ] add-server phone ${H}`, `- [x] add-server phone ${'b'.repeat(16)}`);
  assert.deepEqual(d.ticked, []);
});

test('only write, maintain and admin may act', () => {
  for (const p of ['write', 'maintain', 'admin']) assert.equal(mayAct(p), true);
  for (const p of ['read', 'triage', 'none', '', undefined]) assert.equal(mayAct(p), false);
});

test('manifest entries carry a sha256 per file and replace, never duplicate', () => {
  const e = manifestEntry({ case: 'add-server', profile: 'phone', hash: H, version: '1.0.0', approver: 'owner', date: '2026-10-31', files: [{ path: 'a.mp4', data: Buffer.from('abc') }] });
  assert.deepEqual(e.files, [{ path: 'a.mp4', sha256: 'ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad', bytes: 3 }]);
  assert.throws(() => manifestEntry({ ...e, approver: '', files: [] }));
  const m = addEntry(addEntry({ version: 1, entries: [] }, e), { ...e, hash: 'c'.repeat(16) });
  assert.equal(m.entries.length, 1);
  assert.equal(m.entries[0].hash, 'c'.repeat(16));
  assert.equal(dropEntry(m, 'add-server', 'phone').entries.length, 0);
  assert.equal(dropEntry(m, 'nope', 'phone').entries.length, 1);
});

import { mkdtempSync, writeFileSync, readFileSync, rmSync, existsSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { approve } from './docs-media-review-cli.mjs';
import { validate, listMedia } from './docs-media-manifest.mjs';

test('approve copies the render, records the entry and refuses a hash that was not ticked', () => {
  const art = mkdtempSync(join(tmpdir(), 'tb-art-')), media = mkdtempSync(join(tmpdir(), 'tb-media-'));
  try {
    writeFileSync(join(art, 'add-server-phone.mp4'), 'v');
    writeFileSync(join(art, 'add-server-phone.gif'), 'g');
    writeFileSync(join(art, 'add-server-phone.render.json'), JSON.stringify({ case: 'add-server', profile: 'phone', hash: H, version: '1.0.0', files: ['add-server-phone.mp4', 'add-server-phone.gif', '../escape.txt'] }));
    assert.throws(() => approve({ artifactDir: art, mediaDir: media, case: 'add-server', profile: 'phone', hash: 'f'.repeat(16), approver: 'owner', date: '2026-10-31' }), /not the ticked/);
    assert.equal(existsSync(join(media, 'manifest.json')), false, 'nothing was written');
    assert.throws(() => approve({ artifactDir: art, mediaDir: media, case: 'add-server', profile: 'phone', hash: H, approver: '', date: '2026-10-31' }), /approver/);
    assert.equal(listMedia(media).length, 0, 'a refused approval copies nothing');
    const m = approve({ artifactDir: art, mediaDir: media, case: 'add-server', profile: 'phone', hash: H, approver: 'owner', date: '2026-10-31' });
    assert.equal(m.entries[0].files.length, 2, 'a path outside the folder is not copied');
    assert.deepEqual(validate(JSON.parse(readFileSync(join(media, 'manifest.json'), 'utf8')), listMedia(media)), []);
  } finally { rmSync(art, { recursive: true, force: true }); rmSync(media, { recursive: true, force: true }); }
});

import { issueBody } from './docs-media-review-cli.mjs';
test('the issue body has one fixed-form line per render, which the checklist parser reads back', () => {
  const dir = mkdtempSync(join(tmpdir(), 'tb-body-'));
  try {
    for (const p of ['phone', 'desktop']) writeFileSync(join(dir, `add-server-${p}.render.json`), JSON.stringify({ case: 'add-server', profile: p, hash: H }));
    const body = issueBody({ dir, previewUrl: 'https://x.pages.dev', runId: '7' });
    assert.deepEqual(parseChecklist(body).map((l) => `${l.case} ${l.profile} ${l.ticked}`), ['add-server desktop false', 'add-server phone false']);
    assert.ok(body.includes('https://x.pages.dev') && body.includes('/regen'));
  } finally { rmSync(dir, { recursive: true, force: true }); }
});
