import { describe, expect, it } from 'vitest';
import { applyMods, clampFont, pinchFont, prefixBytes, KEYS } from './keys';

describe('applyMods', () => {
  it('passes data through when no modifier is armed', () => {
    expect(applyMods('a', { ctrl: false, alt: false })).toEqual({ out: 'a', used: false });
  });
  it('turns Ctrl+letter into the control code, either case', () => {
    expect(applyMods('c', { ctrl: true, alt: false })).toEqual({ out: '\x03', used: true });
    expect(applyMods('D', { ctrl: true, alt: false })).toEqual({ out: '\x04', used: true });
    expect(applyMods('[', { ctrl: true, alt: false }).out).toBe('\x1b');
  });
  it('maps Ctrl+space and Ctrl+? to NUL and DEL', () => {
    expect(applyMods(' ', { ctrl: true, alt: false }).out).toBe('\0');
    expect(applyMods('?', { ctrl: true, alt: false }).out).toBe('\x7f');
  });
  it('prefixes Alt with escape, and combines with Ctrl', () => {
    expect(applyMods('b', { ctrl: false, alt: true }).out).toBe('\x1bb');
    expect(applyMods('a', { ctrl: true, alt: true }).out).toBe('\x1b\x01');
  });
  it('leaves pastes and escape sequences alone and keeps the modifier armed', () => {
    expect(applyMods('hello', { ctrl: true, alt: false })).toEqual({ out: 'hello', used: false });
    expect(applyMods(KEYS.up, { ctrl: true, alt: false }).used).toBe(false);
  });
  it('leaves a character with no control form unchanged but still consumes the modifier', () => {
    expect(applyMods('1', { ctrl: true, alt: false })).toEqual({ out: '1', used: true });
  });
});

describe('font sizing', () => {
  it('clamps to a readable range', () => {
    expect(clampFont(3)).toBe(9);
    expect(clampFont(99)).toBe(28);
    expect(clampFont(13.6)).toBe(14);
  });
  it('scales with the pinch ratio', () => {
    expect(pinchFont(14, 1.5)).toBe(21);
    expect(pinchFont(14, 0.5)).toBe(9);
  });
});

import { FN_KEYS, SYMBOLS, bufferText } from './keys';

describe('Fn panel', () => {
  it('sends the xterm sequences for navigation and function keys', () => {
    const k = Object.fromEntries(FN_KEYS);
    expect(k.PgUp).toBe('\x1b[5~');
    expect(k.PgDn).toBe('\x1b[6~');
    expect(k.Del).toBe('\x1b[3~');
    expect(k.F1).toBe('\x1bOP');
    expect(k.F5).toBe('\x1b[15~');
    expect(k.F12).toBe('\x1b[24~');
    expect(new Set(FN_KEYS.map(([, s]) => s)).size).toBe(FN_KEYS.length);
  });

  it('offers the symbols a phone keyboard hides', () => {
    for (const c of ['{', '}', '\\', '`', '<', '>']) expect(SYMBOLS).toContain(c);
  });
});

describe('bufferText', () => {
  it('keeps the last lines, trims right edges and trailing blanks', () => {
    const lines = ['a  ', 'b', '', 'c   ', '', ''];
    expect(bufferText((i) => ({ text: lines[i] }), lines.length, 80)).toBe('a\nb\n\nc');
    expect(bufferText((i) => ({ text: lines[i] }), lines.length, 80, 2)).toBe('');
    expect(bufferText((i) => ({ text: lines[i] }), lines.length, 80, 4)).toBe('\nc');
  });
});

describe('tmux prefix notation', () => {
  it('turns tmux notation into the bytes the terminal gets', () => {
    expect(prefixBytes('C-b')).toBe('\x02');
    expect(prefixBytes('C-a')).toBe('\x01');
    expect(prefixBytes('C-Space')).toBe('\0');
    expect(prefixBytes('M-x')).toBe('\x1bx');
    expect(prefixBytes('`')).toBe('`');
    expect(prefixBytes('None')).toBe('');
    expect(prefixBytes('')).toBe('');
    expect(prefixBytes('C-nonsense')).toBe('');
  });
});

describe('prefixLabel and newlineKeys', () => {
  it('prints tmux prefixes as keycaps', async () => {
    const { prefixLabel } = await import('./keys');
    expect(prefixLabel('C-b')).toBe('^B');
    expect(prefixLabel('C-Space')).toBe('^Space');
    expect(prefixLabel('M-a')).toBe('M-a');
    expect(prefixLabel('C-M-b')).toBe('^M-B');
    expect(prefixLabel('None')).toBe('');
  });

  it('picks the multiline key per agent', async () => {
    const { newlineKeys } = await import('./keys');
    expect(newlineKeys('claude')).toEqual({ seq: '\x1b\r', hint: 'Alt+Enter' });
    expect(newlineKeys(undefined).seq).toBe('\x1b\r');
    expect(newlineKeys('codex')).toEqual({ seq: '\n', hint: 'Ctrl+J' });
  });
});

describe('Shift', () => {
  it('upper-cases one typed character and leaves pastes alone', () => {
    expect(applyMods('a', { ctrl: false, alt: false, shift: true })).toEqual({ out: 'A', used: true });
    expect(applyMods('привет', { ctrl: false, alt: false, shift: true })).toEqual({ out: 'привет', used: false });
  });
  it('turns Tab into back-tab and cursor keys into modifier 2', async () => {
    const { shiftSeq } = await import('./keys');
    expect(shiftSeq(KEYS.tab)).toBe('\x1b[Z');
    expect(shiftSeq(KEYS.up)).toBe('\x1b[1;2A');
    expect(shiftSeq(KEYS.home)).toBe('\x1b[1;2H');
    expect(shiftSeq(KEYS.esc)).toBeNull();
    expect(shiftSeq('|')).toBeNull();
  });
});
