import { test, expect } from 'vitest';
import { paceFactor } from './lib.mjs';

test('normal leaves delays unchanged and is the default', () => {
  expect(paceFactor('normal')).toBe(1);
  expect(paceFactor()).toBe(1);
});
test('slow stretches every delay by 1.6', () => expect(paceFactor('slow')).toBe(1.6));
test('an unknown pace is rejected', () => {
  expect(() => paceFactor('fast')).toThrow(/pace must be slow or normal/);
  expect(() => paceFactor('')).toThrow(/pace must be/);
});
