import { test } from 'node:test';
import assert from 'node:assert/strict';
import { accepted, body, ticked } from './template-review.mjs';

test('the body links each page and has an unticked line', () => {
  const b = body('abc1234', 'https://x.pages.dev/', ['fleet', 'ops']);
  assert.match(b, /- fleet: https:\/\/x\.pages\.dev\/fleet\.html/);
  assert.match(b, /^- \[ \] accept ops abc1234$/m);
});

test('only a newly ticked line counts', () => {
  const before = body('abc1234', 'https://x', ['fleet', 'ops']).replace('- [ ] accept ops', '- [x] accept ops');
  const after = before.replace('- [ ] accept fleet', '- [X] accept fleet');
  assert.deepEqual(ticked(before, after), [{ page: 'fleet', sha: 'abc1234' }]);
  assert.deepEqual(ticked(after, after), []);
});

test('text outside the checklist cannot tick anything', () => {
  const b = body('abc1234', 'https://x', ['fleet']);
  assert.deepEqual(ticked(b, `${b}\n- [x] accept fleet abc1234 and more\n`), []);
  assert.deepEqual(ticked(undefined, undefined), []);
});

test('a page is accepted once any review issue has its line ticked', () => {
  const b = body('abc1234', 'https://x', ['fleet']);
  assert.equal(accepted([b], 'fleet'), false);
  assert.equal(accepted([b, b.replace('- [ ] accept', '- [x] accept')], 'fleet'), true);
  assert.equal(accepted([b.replace('- [ ] accept', '- [x] accept')], 'ops'), false);
});
