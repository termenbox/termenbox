import { describe, expect, it } from 'vitest';
import { InputBatcher, MAX_CHUNK_CHARS } from './inputBatch';

function setup() {
  const sent: string[] = [];
  let fire: (() => void) | null = null;
  const b = new InputBatcher(
    (d) => sent.push(d),
    (fn) => ((fire = fn), 1),
    () => (fire = null),
  );
  return { b, sent, tick: () => fire?.() };
}

describe('InputBatcher', () => {
  it('sends 256 palette replies that arrive together as one message, in order', () => {
    const { b, sent, tick } = setup();
    const replies = Array.from({ length: 256 }, (_, i) => `\x1b]4;${i};rgb:eeee/eeee/eeee\x1b\\`);
    for (const r of replies) b.add(r);
    expect(sent).toEqual([]);
    tick();
    expect(sent).toEqual([replies.join('')]);
  });

  it('keeps ordinary typing as one message per pause', () => {
    const { b, sent, tick } = setup();
    b.add('l');
    tick();
    b.add('s');
    tick();
    expect(sent).toEqual(['l', 's']);
  });

  it('flush sends at once and leaves nothing for the timer', () => {
    const { b, sent, tick } = setup();
    b.add('a');
    b.add('b');
    b.flush();
    tick();
    expect(sent).toEqual(['ab']);
    b.flush();
    expect(sent).toEqual(['ab']);
  });

  it('splits a large paste into messages that fit a data channel, in order and without cutting a character', () => {
    const { b, sent } = setup();
    const text = '😀'.repeat(MAX_CHUNK_CHARS) + 'tail'; // two UTF-16 units per emoji, so a cut would land mid-pair
    b.add(text);
    b.flush();
    expect(sent.length).toBeGreaterThan(1);
    expect(sent.every((s) => s.length <= MAX_CHUNK_CHARS && new TextEncoder().encode(s).length < 48 * 1024)).toBe(true);
    expect(sent.join('')).toBe(text);
    expect(sent.every((s) => !/^[\udc00-\udfff]/.test(s) && !/[\ud800-\udbff]$/.test(s))).toBe(true);
  });
});
