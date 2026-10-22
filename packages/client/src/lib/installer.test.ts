import { describe, expect, it } from 'vitest';
import { readFileSync } from 'node:fs';
// @ts-expect-error plain ESM shared with vite.config.ts
import { bakeInstaller } from '../../installer/bake.mjs';

const source = readFileSync(new URL('../../installer/install.sh', import.meta.url), 'utf8');
const defaultLine = (text: string) => /^ORIGIN=.*# client-origin.*$/m.exec(text)?.[0];

describe('the installer served at /i', () => {
  it('keeps the hosted address when the client build names none', () => {
    expect(bakeInstaller(source, undefined)).toBe(source);
    expect(defaultLine(source)).toContain('https://app.termenbox.dev');
  });

  it('bakes the address of a self-hosted client in as the default, and nowhere else', () => {
    const baked: string = bakeInstaller(source, 'https://tb.example.com/');
    expect(defaultLine(baked)).toBe('ORIGIN="${TERMENBOX_CLIENT_URL:-https://tb.example.com}" # client-origin: the client build bakes its own address here');
    expect(baked.replace(defaultLine(baked)!, '')).toBe(source.replace(defaultLine(source)!, ''));
  });

  it('refuses an address that could reach the shell or is not https', () => {
    for (const bad of ['http://tb.example.com', 'https://tb.example.com/x', 'https://a.b"; rm -rf /; "', 'tb.example.com']) {
      expect(() => bakeInstaller(source, bad)).toThrow();
    }
  });

  it('fails the build if the marked line is lost', () => {
    expect(() => bakeInstaller(source.replace('# client-origin', ''), 'https://tb.example.com')).toThrow(/client-origin/);
  });

  it('takes no secret on the command line', () => {
    expect(source).not.toMatch(/^SECRET="\$\{1/m);
    expect(source).toMatch(/^MODE="\$\{1:-host-me\}"/m);
  });
});
