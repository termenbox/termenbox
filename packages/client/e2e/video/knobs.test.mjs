import test from 'node:test';
import assert from 'node:assert/strict';
import { paceFactor } from './lib.mjs';

test('normal leaves delays unchanged and is the default', () => {
  assert.equal(paceFactor('normal'), 1);
  assert.equal(paceFactor(), 1);
});
test('slow stretches every delay by 1.6', () => assert.equal(paceFactor('slow'), 1.6));
test('an unknown pace is rejected', () => {
  assert.throws(() => paceFactor('fast'), /pace must be slow or normal/);
  assert.throws(() => paceFactor(''), /pace must be/);
});
