import { describe, expect, it } from 'vitest';
import { handleOpenRequests, openWeb, OPEN_OSC, openRequestUrl } from './openRequest';

describe('openRequestUrl', () => {
  it('takes the web address of an open request', () => {
    expect(openRequestUrl('open;https://example.com/a?b=1#c')).toBe('https://example.com/a?b=1#c');
    expect(openRequestUrl('open;http://localhost:8080/cb')).toBe('http://localhost:8080/cb');
  });

  it('refuses other schemes, other verbs, bare domains and control characters', () => {
    for (const bad of ['open;file:///etc/passwd', 'open;javascript:alert(1)', 'open;example.com', 'copy;https://a.b', 'https://a.b', 'open;', 'open;https://a.b/\u001b[0m', 'open;https://a.b/ x']) {
      expect(openRequestUrl(bad), bad).toBeNull();
    }
  });
});

describe('handleOpenRequests', () => {
  it('opens a request, always consumes the sequence, and stops after dispose', () => {
    let handler: ((d: string) => boolean) | null = null;
    const parser = {
      registerOscHandler: (id: number, cb: (d: string) => boolean) => {
        expect(id).toBe(OPEN_OSC);
        handler = cb;
        return { dispose: () => (handler = null) };
      },
    };
    const opened: string[] = [];
    const h = handleOpenRequests(parser, (u) => opened.push(u));
    expect(handler!('open;https://example.com')).toBe(true);
    expect(handler!('open;file:///x')).toBe(true);
    expect(opened).toEqual(['https://example.com']);
    h.dispose();
    expect(handler).toBeNull();
  });
});

describe('openWeb', () => {
  it('opens a window without an opener', () => {
    const calls: unknown[][] = [];
    expect(openWeb('https://a.b', { open: (...a: unknown[]) => (calls.push(a), {}) as Window })).toBe(true);
    expect(calls).toEqual([['https://a.b', '_blank', 'noopener,noreferrer']]);
  });

  it('reports a blocked popup, and never opens a non-web address', () => {
    expect(openWeb('https://a.b', { open: () => null })).toBe(false);
    let called = false;
    expect(openWeb('javascript:alert(1)', { open: () => ((called = true), {} as Window) })).toBe(false);
    expect(called).toBe(false);
  });
});
