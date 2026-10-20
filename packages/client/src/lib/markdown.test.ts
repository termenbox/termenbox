import { describe, expect, it } from 'vitest';
import { countHits, decorate, hitsIn, parseBlocks, parseInline, runsOf, safeHref, type Block } from './markdown';

describe('Markdown reader', () => {
  it('reads the blocks agents write', () => {
    const b = parseBlocks('# Title\n\nSome **bold** and `code` and _it_.\n\n- one\n- two\n  - nested\n\n1. a\n2. b\n\n> quoted\n\n---\n\n```rs\nfn x() {}\n```');
    expect(b.map((x) => x.t)).toEqual(['h', 'p', 'ul', 'ol', 'quote', 'hr', 'code']);
    const p = b[1] as Extract<Block, { t: 'p' }>;
    expect(p.inl.map((n) => n.t)).toEqual(['text', 'b', 'text', 'code', 'text', 'i', 'text']);
    const ul = b[2] as Extract<Block, { t: 'ul' }>;
    expect(ul.items).toHaveLength(2);
    expect(ul.items[1].map((x) => x.t)).toEqual(['p', 'ul']);
    expect((b[6] as Extract<Block, { t: 'code' }>)).toMatchObject({ lang: 'rs', text: 'fn x() {}' });
  });

  it('keeps an unclosed fence (a reply still streaming) as code to the end', () => {
    const b = parseBlocks('Look:\n```\nline 1\nline 2');
    expect(b[1]).toMatchObject({ t: 'code', text: 'line 1\nline 2' });
  });

  it('never produces markup: html stays text, and only web links are links', () => {
    const b = parseBlocks('<img src=x onerror=alert(1)> [click](javascript:alert(1)) [ok](https://example.com/a) https://example.org/x.');
    const inl = (b[0] as Extract<Block, { t: 'p' }>).inl;
    expect(inl.filter((n) => n.t === 'link').map((n) => (n as { href: string }).href)).toEqual(['https://example.com/a', 'https://example.org/x']);
    expect(JSON.stringify(inl)).toContain('<img src=x onerror=alert(1)>');
    expect(safeHref('javascript:alert(1)')).toBeNull();
    expect(safeHref('data:text/html,x')).toBeNull();
    expect(safeHref('https://example.com')).toBe('https://example.com/');
  });

  it('does not eat underscores in identifiers or stars in math', () => {
    const t = (s: string) => runsOf(parseBlocks(s)).join('');
    expect(t('call snake_case_name now')).toBe('call snake_case_name now');
    expect(t('2 * 3 * 4')).toBe('2 * 3 * 4');
  });

  it('bounds hostile nesting and size', () => {
    const deep = '> '.repeat(50) + 'x';
    expect(() => parseBlocks(deep)).not.toThrow();
    const many = Array.from({ length: 5000 }, (_, i) => `para ${i}`).join('\n\n');
    expect(parseBlocks(many).length).toBeLessThanOrEqual(600);
    expect(() => parseInline('*'.repeat(10_000))).not.toThrow();
  });

  it('counts and marks search hits in drawing order, per run, case-insensitively', () => {
    const b = parseBlocks('Find **Foo** and foo\n\n```\nFOO bar\n```');
    expect(runsOf(b)).toEqual(['Find ', 'Foo', ' and foo', 'FOO bar']);
    expect(countHits(b, 'foo')).toBe(3);
    expect(hitsIn('aaaa', 'aa')).toEqual([[0, 2], [2, 4]]);
    const d = decorate(b, 'foo', 1);
    const p = d[0] as Extract<Block, { t: 'p' }>;
    const bold = p.inl[1] as { inl: { parts?: { hit: boolean; current: boolean }[] }[] };
    expect(bold.inl[0].parts).toEqual([{ text: 'Foo', hit: true, current: false }]);
    const tail = p.inl[2] as { parts?: { text: string; hit: boolean; current: boolean }[] };
    expect(tail.parts).toEqual([{ text: ' and ', hit: false, current: false }, { text: 'foo', hit: true, current: true }]);
    expect(decorate(b, '', 0)).toBe(b);
  });
});
