import { describe, expect, it } from 'vitest';
import { handleOpenRequests, hyperlinkHandler, openWeb, OPEN_OSC, openRequestUrl } from './openRequest';

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
  // A browser returns null for a window opened with `noopener`, so a block can only be seen without that feature.
  const browser = (blocked: boolean) => {
    const opened: { url: string; features: string | undefined; win: { opener: unknown } | null }[] = [];
    return {
      opened,
      open: (url: string, _target?: string, features?: string) => {
        const win = blocked || features?.includes('noopener') ? null : { opener: 'page' };
        opened.push({ url, features, win });
        return win as unknown as Window | null;
      },
    };
  };

  it('counts a window the browser opened as opened and cuts its link back to the page', () => {
    const b = browser(false);
    expect(openWeb('https://a.b', b)).toBe(true);
    expect(b.opened[0].url).toBe('https://a.b');
    expect(b.opened[0].features ?? '').not.toContain('noopener');
    expect(b.opened[0].win!.opener).toBeNull();
  });

  it('reports a blocked popup, and never opens a non-web address', () => {
    expect(openWeb('https://a.b', browser(true))).toBe(false);
    const b = browser(false);
    expect(openWeb('javascript:alert(1)', b)).toBe(false);
    expect(b.opened).toEqual([]);
  });
});

describe('hyperlinkHandler', () => {
  const range = {} as never;

  it('opens the target of an OSC 8 hyperlink and takes the click', () => {
    const opened: string[] = [];
    const h = hyperlinkHandler((u) => opened.push(u));
    let prevented = false;
    h.activate({ preventDefault: () => (prevented = true) } as unknown as MouseEvent, 'https://example.com/a?b=1', range);
    expect(opened).toEqual(['https://example.com/a?b=1']);
    expect(prevented).toBe(true);
  });

  it('ignores other schemes and keeps xterm from opening them itself', () => {
    const opened: string[] = [];
    const h = hyperlinkHandler((u) => opened.push(u));
    for (const bad of ['file:///etc/passwd', 'javascript:alert(1)', 'ftp://a.b', 'example.com']) h.activate({ preventDefault() {} } as unknown as MouseEvent, bad, range);
    expect(opened).toEqual([]);
    expect(h.allowNonHttpProtocols).toBe(false);
  });
});
