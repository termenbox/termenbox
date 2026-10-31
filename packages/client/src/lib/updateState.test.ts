import { describe, expect, it } from 'vitest';
import { STALE_AFTER_S, checkAnswered, checkIsStale, deadlineText, lastChecked, nextStep } from './updateState';

const NOW = Date.UTC(2026, 9, 31, 12, 0, 0);
const sec = NOW / 1000;
const all = () => true;
const none = () => false;

describe('lastChecked', () => {
  it('says how long ago the last good check was', () => {
    expect(lastChecked({ checked_ok_at: sec - 30 }, NOW)).toBe('Last checked just now');
    expect(lastChecked({ checked_ok_at: sec - 600 }, NOW)).toBe('Last checked 10 min ago');
    expect(lastChecked({ checked_ok_at: sec - 3 * 3600 }, NOW)).toBe('Last checked 3 h ago');
    expect(lastChecked({ checked_ok_at: sec - 4 * 86400 }, NOW)).toBe('Last checked 4 d ago');
  });
  it('tells a server that never checked from one whose checks all failed', () => {
    expect(lastChecked(undefined, NOW)).toBe('Not checked yet');
    expect(lastChecked({ checked_at: sec - 5, error: 'offline' }, NOW)).toBe('No successful check yet');
  });
});

describe('checkIsStale', () => {
  it('warns past 72 h and not before', () => {
    expect(checkIsStale({ checked_ok_at: sec - STALE_AFTER_S + 60 }, NOW)).toBe(false);
    expect(checkIsStale({ checked_ok_at: sec - STALE_AFTER_S - 60 }, NOW)).toBe(true);
  });
  it('warns when checks have only failed, and never for a server that reports nothing', () => {
    expect(checkIsStale({ checked_at: sec - 10, error: 'offline' }, NOW)).toBe(true);
    expect(checkIsStale({ checked_at: sec - 10 }, NOW)).toBe(false);
    expect(checkIsStale(undefined, NOW)).toBe(false);
    expect(checkIsStale({}, NOW)).toBe(false);
  });
});

describe('nextStep', () => {
  it.each([
    ['installing', { phase: 'installing', available: '0.1.58' }, '0.1.58', 'Installing…'],
    ['rolled back', { phase: 'rolled-back' }, '', 'The last update was rolled back. Update now tries again.'],
    ['up to date', { phase: 'idle' }, '', 'Up to date.'],
    ['available, nothing scheduled', { phase: 'available', available: '0.1.58' }, '0.1.58', 'v0.1.58 is available.'],
    ['a server that reports nothing', undefined, '0.1.58', 'v0.1.58 is available.'],
  ])('%s', (_name, info, available, text) => {
    expect(nextStep(info, available, NOW, all)).toBe(text);
  });
  it('names the deadline and only the choices the helper offers', () => {
    const info = { phase: 'scheduled', available: '0.1.58', deadline: sec + 3600, can_postpone: true };
    expect(nextStep(info, '0.1.58', NOW, all)).toMatch(/^v0\.1\.58 installs at \d\d:\d\d unless you postpone or skip\.$/);
    expect(nextStep({ ...info, can_postpone: false }, '0.1.58', NOW, all)).toMatch(/unless you skip\.$/);
    expect(nextStep(info, '0.1.58', NOW, none)).toMatch(/installs at \d\d:\d\d\.$/);
  });
});

describe('deadlineText', () => {
  it('adds the weekday for another day', () => {
    expect(deadlineText(sec + 5 * 86400, NOW)).toMatch(/^\w+ \d\d:\d\d$/);
    expect(deadlineText(sec, NOW)).toMatch(/^\d\d:\d\d$/);
  });
});

describe('checkAnswered', () => {
  it('is true once the updater records a newer attempt', () => {
    expect(checkAnswered({ checked_at: 10 }, 10)).toBe(false);
    expect(checkAnswered({ checked_at: 11 }, 10)).toBe(true);
    expect(checkAnswered(undefined, 0)).toBe(false);
  });
});
