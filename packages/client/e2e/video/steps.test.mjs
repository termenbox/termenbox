import { test, expect } from 'vitest';
import { stepTracker, checkDeclared } from './steps.mjs';

const steps = [
  { id: 'open', caption: 'Start a new workspace', label: '+ New' },
  { id: 'look', caption: 'Read the summary', label: null },
  { id: 'create', caption: 'Create it', label: 'Create' },
];

test('steps reached in order return the declared step and complete', () => {
  const t = stepTracker(steps);
  expect(t.reach('open').label).toBe('+ New');
  t.reach('look'); t.reach('create');
  expect(() => t.assertComplete()).not.toThrow();
});
test('a step out of order throws', () => {
  const t = stepTracker(steps);
  expect(() => t.reach('create')).toThrow(/out of order: expected open/);
});
test('an unknown step throws', () => {
  expect(() => stepTracker(steps).reach('nope')).toThrow(/not declared/);
});
test('a repeated step throws', () => {
  const t = stepTracker(steps);
  t.reach('open');
  expect(() => t.reach('open')).toThrow(/out of order: expected look/);
});
test('a missing step makes the run fail', () => {
  const t = stepTracker(steps);
  t.reach('open');
  expect(t.missing()).toEqual(['look', 'create']);
  expect(() => t.assertComplete()).toThrow(/never reached: look, create/);
});
test('declarations need id, caption and label, and unique ids', () => {
  expect(() => checkDeclared('c', [])).toThrow(/no steps/);
  expect(() => checkDeclared('c', [{ id: 'a', caption: 'x' }])).toThrow(/label/);
  expect(() => checkDeclared('c', [steps[0], steps[0]])).toThrow(/twice/);
  expect(() => checkDeclared('c', steps)).not.toThrow();
});
