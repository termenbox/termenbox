import { describe, expect, it } from 'vitest';
import { readFileSync } from 'node:fs';
// @ts-expect-error plain ESM shared with vite.config.ts
import { bakeInstaller, channelKeys } from '../../installer/bake.mjs';

const source = readFileSync(new URL('../../installer/install.sh', import.meta.url), 'utf8');
const keysText = readFileSync(new URL('../../../server/release-keys.txt', import.meta.url), 'utf8');
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

  it('bakes each channel and exactly the keys it trusts (r1; r1 and b1; n1)', () => {
    const kidsOf = (channel: string) => channelKeys(keysText, channel).map((l: string) => l.split(' ')[0]);
    expect(kidsOf('stable')).toEqual(['r1']);
    expect(kidsOf('beta')).toEqual(['r1', 'b1']);
    expect(kidsOf('nightly')).toEqual(['n1']);
    for (const [channel, trusted] of [['stable', ['r1']], ['beta', ['r1', 'b1']], ['nightly', ['n1']]] as const) {
      const baked: string = bakeInstaller(source, 'https://x.example.com', channel, keysText);
      expect(baked).toContain(`DEFAULT_CHANNEL="${channel}" # default-channel`);
      const block = baked.slice(baked.indexOf('RELEASE_KEYS='), baked.indexOf('# release-keys'));
      for (const kid of ['r1', 'b1', 'n1']) expect(new RegExp(`(^|\\n|:-)${kid} ssh-ed25519`).test(block)).toBe(trusted.includes(kid as never));
    }
  });

  it('is unchanged without a channel, and refuses an unknown channel or a lost marker', () => {
    expect(bakeInstaller(source, undefined, undefined, keysText)).toBe(source);
    expect(() => bakeInstaller(source, undefined, 'edge', keysText)).toThrow(/channel/);
    expect(() => bakeInstaller(source.replace('# default-channel', ''), undefined, 'beta', keysText)).toThrow(/default-channel/);
    expect(() => bakeInstaller(source.replace('# release-keys', ''), undefined, 'beta', keysText)).toThrow(/release-keys/);
    expect(() => channelKeys('r1 ssh-ed25519 AAAA\n', 'nightly')).toThrow(/nightly/);
    expect(() => channelKeys('r1 ssh-ed25519 AAAA"; rm -rf /; "\n', 'stable')).toThrow();
  });
});
