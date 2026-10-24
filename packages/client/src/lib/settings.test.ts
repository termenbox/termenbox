import { describe, it, expect } from 'vitest';
import { loadSettings, saveSettings } from './settings';

describe('device settings', () => {
  it('persists theme and cache TTL together without replacing the other setting', () => {
    const values = new Map<string, string>();
    const storage = { getItem: (k: string) => values.get(k) ?? null, setItem: (k: string, v: string) => { values.set(k, v); } };
    saveSettings({ theme: 'light', skin: 'teal', cacheTtlMs: 3600000, keepWarm: '', renderer: 'dom', alertSound: true, alertSoundId: 'bloop', alertVibrate: true }, storage);
    expect(loadSettings(storage)).toEqual({ theme: 'light', skin: 'teal', cacheTtlMs: 3600000, keepWarm: '', renderer: 'dom', alertSound: true, alertSoundId: 'bloop', alertVibrate: true });
  });
  it('falls back to the navy skin for an unknown palette', () => {
    expect(loadSettings({ getItem: () => '{"skin":"neon"}' }).skin).toBe('navy');
    expect(loadSettings({ getItem: () => '{"skin":"graphite"}' }).skin).toBe('graphite');
  });
  it('validates corrupt or unsupported settings and tolerates blocked storage', () => {
    expect(loadSettings({ getItem: () => '{' })).toEqual({ theme: 'dark', skin: 'navy', cacheTtlMs: 300000, keepWarm: '', renderer: 'webgl', alertSound: true, alertSoundId: 'bloop', alertVibrate: true });
    expect(loadSettings({ getItem: () => '{"theme":"bogus","cacheTtlMs":1}' })).toEqual({ theme: 'dark', skin: 'navy', cacheTtlMs: 300000, keepWarm: '', renderer: 'webgl', alertSound: true, alertSoundId: 'bloop', alertVibrate: true });
    expect(() => saveSettings({ theme: 'system', skin: 'navy', cacheTtlMs: 300000, keepWarm: '', renderer: 'webgl', alertSound: true, alertSoundId: 'bloop', alertVibrate: true }, { setItem: () => { throw new Error('private'); } })).not.toThrow();
  });

  it('keeps the alert flags and defaults them to on', () => {
    const storage = { getItem: () => JSON.stringify({ alertSound: false, alertVibrate: false }) };
    expect(loadSettings(storage)).toMatchObject({ alertSound: false, alertVibrate: false });
    expect(loadSettings({ getItem: () => '{}' })).toMatchObject({ alertSound: true, alertSoundId: 'bloop', alertVibrate: true });
  });
});
