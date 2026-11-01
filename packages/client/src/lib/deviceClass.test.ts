import { describe, expect, it } from 'vitest';
import { deviceClass } from './deviceClass';

const win = (coarse: boolean, w: number, h: number) => ({ matchMedia: () => ({ matches: coarse }), screen: { width: w, height: h } }) as unknown as Window;

describe('deviceClass', () => {
  it('is phone for a small touch screen, in either orientation', () => {
    expect(deviceClass(win(true, 390, 844))).toBe('phone');
    expect(deviceClass(win(true, 844, 390))).toBe('phone');
  });
  it('is desktop for a mouse, and for a large touch screen', () => {
    expect(deviceClass(win(false, 1440, 900))).toBe('desktop');
    expect(deviceClass(win(true, 820, 1180))).toBe('desktop');
  });
  it('is other when the browser cannot say', () => {
    expect(deviceClass(undefined)).toBe('other');
    expect(deviceClass({} as Window)).toBe('other');
  });
});
