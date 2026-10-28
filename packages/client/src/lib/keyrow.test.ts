import { describe, expect, it } from 'vitest';
import { KEY_BY_ID, KEY_CATALOGUE, KEYS, FN_KEYS, SYMBOLS } from './keys';
import {
  DEFAULT_ROW, added, available, capacity, defaultConfig, loadKeyRow, moreKeys, moved, needsMore, removed, resolveRow, rowMode,
  saveKeyRow, splitRow, validateConfig, MIN_KEY_WIDTH, type KeyRowConfig,
} from './keyrow';
import { createPress } from './press';

// The 16 keys of KeyRow.svelte before U07c, in order: accessible name (or the label where it had none) and what each does.
const TODAY: [id: string, label: string, name: string][] = [
  ['slash', '/', 'Slash'], ['ctrlC', '^C', 'Control C'], ['esc', '⎋', 'Escape'], ['newline', '↵', 'New line without sending'],
  ['ctrl', '⌃', 'Ctrl'], ['alt', '⌥', 'Alt'], ['shift', '⇧', 'Shift'], ['tab', '⇥', 'Tab'], ['fn', 'Fn', 'Fn'],
  ['up', '↑', 'Arrow up'], ['down', '↓', 'Arrow down'], ['keyboard', '⌨', 'Show or hide the keyboard'],
  ['voice', '🎤', 'Dictate'], ['voicelang', '🌐', 'Dictation language'], ['fontDown', 'A−', 'Smaller text'], ['fontUp', 'A+', 'Larger text'],
];

describe('the default configuration is today\'s row', () => {
  it('has the same 16 keys in the same order', () => {
    expect(DEFAULT_ROW).toEqual(TODAY.map(([id]) => id));
    expect(resolveRow(defaultConfig(), 'shell')).toEqual(TODAY.map(([id]) => id));
  });
  it('prints and names each key as before', () => {
    for (const [id, label, name] of TODAY) expect(KEY_BY_ID.get(id)).toMatchObject({ label, name });
  });
  it('sends what the row sent', () => {
    const seq = (id: string) => KEY_BY_ID.get(id)?.seq;
    expect(seq('slash')).toBe('/');
    expect(seq('ctrlC')).toBe(KEYS.ctrlC);
    expect(seq('esc')).toBe(KEYS.esc);
    expect(seq('tab')).toBe(KEYS.tab);
    expect(seq('up')).toBe(KEYS.up);
    expect(seq('down')).toBe(KEYS.down);
  });
  it('puts the sticky prefix first for tmux, as before, and not in a shell', () => {
    expect(resolveRow(defaultConfig(), 'tmux')).toEqual(['prefix', ...DEFAULT_ROW]);
    expect(resolveRow({ ...defaultConfig(), shell: { preset: 'custom', keys: ['prefix', 'esc'] } }, 'shell')).toEqual(['esc']);
  });
  it('loads as the default when nothing is stored', () => {
    expect(loadKeyRow({ getItem: () => null })).toEqual(defaultConfig());
  });
});

describe('the catalogue', () => {
  it('names every key, with unique ids', () => {
    expect(new Set(KEY_CATALOGUE.map((k) => k.id)).size).toBe(KEY_CATALOGUE.length);
    for (const k of KEY_CATALOGUE) { expect(k.name.length).toBeGreaterThan(0); expect(k.label.length).toBeGreaterThan(0); }
    for (const k of KEY_CATALOGUE.filter((k) => k.kind === 'send')) expect(k.seq).toBeTruthy();
  });
  it('covers every Fn panel key and symbol', () => {
    const seqs = new Set(KEY_CATALOGUE.map((k) => k.seq));
    for (const [, seq] of FN_KEYS) expect(seqs.has(seq)).toBe(true);
    for (const c of SYMBOLS) expect(seqs.has(c)).toBe(true);
    for (const seq of [KEYS.home, KEYS.end, KEYS.left, KEYS.right, KEYS.ctrlD, '\r', '\x7f', ' ', '|', '~', '-']) expect(seqs.has(seq)).toBe(true);
  });
});

describe('a custom configuration persists', () => {
  const custom: KeyRowConfig = {
    shell: { preset: 'custom', keys: ['esc', 'tab', 'ctrl', 'up', 'f5', 'sym-7b'] },
    tmux: { preset: 'minimal', keys: ['prefix', 'esc'] },
    desktop: 'full',
  };
  it('round-trips through storage in the same order', () => {
    const mem: Record<string, string> = {};
    saveKeyRow(custom, { setItem: (k, v) => { mem[k] = v; } });
    const back = loadKeyRow({ getItem: (k) => mem[k] ?? null });
    expect(back).toEqual(custom);
    expect(resolveRow(back, 'shell')).toEqual(['esc', 'tab', 'ctrl', 'up', 'f5', 'sym-7b']);
    expect(resolveRow(back, 'tmux')).toEqual(['prefix', 'esc', 'ctrlC', 'tab', 'up', 'down', 'fn']);
  });
  it('keeps working when storage throws', () => {
    expect(() => saveKeyRow(custom, { setItem: () => { throw new Error('quota'); } })).not.toThrow();
    expect(loadKeyRow({ getItem: () => { throw new Error('denied'); } })).toEqual(defaultConfig());
  });
});

describe('an invalid stored value falls back to the default', () => {
  const bad: unknown[] = [
    null, 42, 'x', [], {}, { shell: 1 },
    { ...defaultConfig(), desktop: 'huge' },
    { ...defaultConfig(), shell: { preset: 'custom', keys: ['nope'] } },
    { ...defaultConfig(), shell: { preset: 'custom', keys: [] } },
    { ...defaultConfig(), shell: { preset: 'custom', keys: ['esc', 'esc'] } },
    { ...defaultConfig(), shell: { preset: 'custom', keys: [1] } },
    { ...defaultConfig(), shell: { preset: 'wide', keys: ['esc'] } },
    { ...defaultConfig(), tmux: { preset: 'custom' } },
  ];
  it.each(bad.map((b, i) => [i, b]))('case %i', (_i, value) => {
    expect(validateConfig(value)).toEqual(defaultConfig());
  });
  it('treats unparseable text as the default', () => {
    expect(loadKeyRow({ getItem: () => '{not json' })).toEqual(defaultConfig());
  });
});

describe('removed keys stay reachable in More', () => {
  it('lists every available key that is not in the row', () => {
    const row = ['esc', 'tab'];
    const more = moreKeys(row, [], 'shell', true).map((k) => k.id);
    for (const k of available('shell', true)) expect(row.includes(k.id) || more.includes(k.id)).toBe(true);
    expect(more).toContain('ctrlC');
    expect(more).not.toContain('prefix');
    expect(moreKeys(row, [], 'tmux', true).map((k) => k.id)).toContain('prefix');
  });
  it('does not offer dictation where the browser cannot dictate', () => {
    expect(moreKeys(['esc'], [], 'shell', false).map((k) => k.id)).not.toContain('voice');
  });
  it('has no More key for today\'s row, and has one as soon as a key of it is removed or does not fit', () => {
    for (const profile of ['shell', 'tmux'] as const) {
      const row = resolveRow(defaultConfig(), profile);
      expect(needsMore(row, [], profile, true)).toBe(false);
      expect(needsMore(row.filter((id) => id !== 'ctrlC'), [], profile, true)).toBe(true);
      expect(needsMore(row, ['fontUp'], profile, true)).toBe(true);
    }
    expect(needsMore(resolveRow(defaultConfig(), 'shell').filter((id) => id !== 'voice' && id !== 'voicelang'), [], 'shell', false)).toBe(false);
  });
});

describe('keys that do not fit move to More', () => {
  it('never goes below the minimum key width', () => {
    for (const w of [240, 320, 360, 390, 430]) {
      const { shown, overflow } = splitRow(resolveRow(defaultConfig(), 'tmux'), w);
      const shownCount = shown.length + (overflow.length ? 1 : 0); // the More key takes a slot
      expect((w - 8 - (shownCount - 1) * 2) / shownCount).toBeGreaterThanOrEqual(MIN_KEY_WIDTH - 0.01);
    }
  });
  it('fits today\'s row, tmux prefix included, on a 390 pt phone', () => {
    expect(splitRow(resolveRow(defaultConfig(), 'tmux'), 390).overflow).toEqual([]);
    expect(capacity(390)).toBeGreaterThanOrEqual(17);
  });
  it('keeps the order and moves the tail', () => {
    const { shown, overflow } = splitRow(['a', 'b', 'c', 'd', 'e', 'f'], 8 + 4 * 20 - 2);
    expect([...shown, ...overflow]).toEqual(['a', 'b', 'c', 'd', 'e', 'f']);
    expect(shown).toHaveLength(3);
  });
  it('shows everything while the width is unknown', () => {
    expect(splitRow(['a', 'b'], 0).overflow).toEqual([]);
  });
});

describe('editing', () => {
  it('moves, removes and adds without dragging', () => {
    expect(moved(['a', 'b', 'c'], 1, -1)).toEqual(['b', 'a', 'c']);
    expect(moved(['a', 'b', 'c'], 0, -1)).toEqual(['a', 'b', 'c']);
    expect(moved(['a', 'b', 'c'], 2, 1)).toEqual(['a', 'b', 'c']);
    expect(removed(['a', 'b'], 'a')).toEqual(['b']);
    expect(added(['esc'], 'tab')).toEqual(['esc', 'tab']);
    expect(added(['esc'], 'esc')).toEqual(['esc']);
    expect(added(['esc'], 'unknown')).toEqual(['esc']);
  });
});

describe('desktop mode', () => {
  it('Auto follows the pointer, and the override wins either way', () => {
    expect(rowMode('auto', true)).toBe('compact');
    expect(rowMode('auto', false)).toBe('full');
    expect(rowMode('full', true)).toBe('full');
    expect(rowMode('compact', false)).toBe('compact');
  });
});

describe('createPress', () => {
  const ev = { preventDefault: () => {} };
  it('fires once for a press and its click', () => {
    let n = 0, t = 0;
    const p = createPress(() => n++, () => t);
    p.pointerdown(ev); t = 30; p.pointerup(); t = 40; p.click();
    expect(n).toBe(1);
  });
  it('fires once for a long press, measuring the echo from the lift', () => {
    let n = 0, t = 0;
    const p = createPress(() => n++, () => t);
    p.pointerdown(ev); t = 2000; p.pointerup(); t = 2010; p.click();
    expect(n).toBe(1);
  });
  it('fires once for Enter or Space, which arrive as a click alone', () => {
    let n = 0;
    const p = createPress(() => n++, () => 5000);
    p.click();
    expect(n).toBe(1);
  });
  it('counts a click after the echo as a new action, and prevents the default on press', () => {
    let n = 0, t = 0, prevented = 0;
    const p = createPress(() => n++, () => t);
    p.pointerdown({ preventDefault: () => prevented++ }); t = 10; p.click(); t = 20; p.click();
    expect(n).toBe(2);
    expect(prevented).toBe(1);
  });
  it('fires a release key on the lift, once, and not at all when the press is cancelled', () => {
    let n = 0, t = 0;
    const p = createPress(() => n++, () => t, () => true);
    p.pointerdown(ev);
    expect(n).toBe(0);
    t = 30; p.pointerup(); t = 40; p.click();
    expect(n).toBe(1);
    p.pointerdown(ev); p.pointercancel(); p.pointerup();
    expect(n).toBe(1);
  });
});
