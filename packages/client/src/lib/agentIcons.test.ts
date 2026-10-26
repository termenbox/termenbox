import { describe, expect, it } from 'vitest';
import { iconFor, isAgentIcon, ALL_ICONS } from './agentIcons';
import { chipFor } from './cards';

describe('icons', () => {
  it('has a colour icon for every agent the server recognises', () => {
    for (const a of ['claude', 'codex', 'copilot', 'opencode', 'kilo', 'gemini', 'aider', 'cursor-agent', 'amp', 'goose', 'qwen']) {
      const i = iconFor(a, a);
      expect(i.color).toMatch(/^#[0-9A-F]{6}$/i);
      expect(isAgentIcon(a, a)).toBe(true);
    }
  });
  it('knows common programs, ignores a login dash, and falls back for the rest', () => {
    expect(iconFor('bash').label).toBe('bash');
    expect(iconFor('-bash').label).toBe('bash');
    expect(iconFor('lazygit').glyph).toBe('⎇');
    expect(iconFor('mc').label).toBe('Midnight Commander');
    expect(iconFor('weirdtool').label).toBe('weirdtool');
    expect(isAgentIcon('bash')).toBe(false);
  });
  it('lets the server-detected agent win over the command name', () => {
    expect(iconFor('node', 'gemini').label).toBe('Gemini');
  });
  // iOS draws some symbols (✳ ⚡ ❗ ...) as colour emoji, which hides the product colour; a badge may not rely on one.
  const EMOJI = /\p{Emoji_Presentation}|\p{Extended_Pictographic}/u;
  const FORCED_TEXT = /^[^\uFE0F]*\uFE0E?$/u;
  it('draws no agent badge with a glyph that can become a colour emoji', () => {
    for (const [name, spec] of Object.entries(ALL_ICONS)) {
      if (spec.emoji || spec.shape) continue;
      expect(EMOJI.test(spec.glyph), `${name} ${spec.glyph}`).toBe(false);
    }
    for (const a of ['claude', 'codex', 'gemini', 'cursor-agent', 'amp']) expect(iconFor(a, a).shape, a).toMatch(/^<(path|circle)/);
  });
  it('keeps the card state marks to text', () => {
    for (const state of ['needs_input', 'working', 'done', 'error', 'idle', 'unknown'] as const) {
      const icon = chipFor({ state, ts: 0, seq: 0, stale: false } as never, 0).icon;
      expect(icon.replace(/\uFE0E/g, ''), state).not.toMatch(/\p{Emoji_Presentation}/u);
      expect(FORCED_TEXT.test(icon), state).toBe(true);
      if (/\p{Extended_Pictographic}/u.test(icon)) expect(icon.endsWith('\uFE0E'), state).toBe(true);
    }
  });
});
