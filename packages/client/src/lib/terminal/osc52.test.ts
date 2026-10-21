import { describe, expect, it } from 'vitest';
import { handleClipboardWrites, MAX_OSC52_BASE64, osc52Text } from './osc52';

const b64 = (s: string) => btoa(String.fromCharCode(...new TextEncoder().encode(s)));

describe('osc52Text', () => {
  it('decodes what a program set, including non-ASCII', () => {
    expect(osc52Text(`c;${b64('hello')}`)).toBe('hello');
    expect(osc52Text(`;${b64('Привет 😀')}`)).toBe('Привет 😀');
    expect(osc52Text(`pc;${b64('x')}`)).toBe('x');
  });

  it('never answers a query and ignores clears, garbage and oversize payloads', () => {
    expect(osc52Text('c;?')).toBeNull();
    expect(osc52Text('c;')).toBeNull();
    expect(osc52Text('c')).toBeNull();
    expect(osc52Text('c;!!not base64!!')).toBeNull();
    expect(osc52Text(`c;${'A'.repeat(MAX_OSC52_BASE64 + 1)}`)).toBeNull();
  });
});

describe('handleClipboardWrites', () => {
  it('copies a write, swallows a query without copying, and stops after dispose', () => {
    let handler: ((d: string) => boolean) | null = null;
    const parser = {
      registerOscHandler: (id: number, cb: (d: string) => boolean) => {
        expect(id).toBe(52);
        handler = cb;
        return { dispose: () => (handler = null) };
      },
    };
    const copied: string[] = [];
    const h = handleClipboardWrites(parser, (t) => copied.push(t));
    expect(handler!(`c;${b64('one')}`)).toBe(true);
    expect(handler!('c;?')).toBe(true);
    expect(copied).toEqual(['one']);
    h.dispose();
    expect(handler).toBeNull();
  });
});
