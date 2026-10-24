import { describe, expect, it } from 'vitest';
import { PROMPT_ROWS, ScrollbackParts, clickTarget, keyAction, type KeyInfo } from './focusTarget';

const k = (over: Partial<KeyInfo>): KeyInfo => ({ key: 'a', code: 'KeyA', ctrl: false, meta: false, shift: false, alt: false, ...over });

describe('where a click puts the user', () => {
  it('keeps the prompt around the cursor and below it, and leaves the output above', () => {
    expect(clickTarget(20, 20)).toBe('prompt');
    expect(clickTarget(22, 20)).toBe('prompt');
    expect(clickTarget(20 - PROMPT_ROWS, 20)).toBe('prompt');
    expect(clickTarget(20 - PROMPT_ROWS - 1, 20)).toBe('output');
    expect(clickTarget(0, 20)).toBe('output');
  });
});

describe('Ctrl+A', () => {
  it('goes to the agent while the user is in its prompt', () => {
    expect(keyAction(k({ ctrl: true }), 'prompt', false)).toBe('pass');
    expect(keyAction(k({ ctrl: true }), 'prompt', true)).toBe('pass');
  });
  it('selects the whole terminal once the user is out of the prompt', () => {
    expect(keyAction(k({ ctrl: true }), 'output', false)).toBe('select-all');
  });
  it('always selects the whole terminal as Ctrl+Shift+A or Cmd+A', () => {
    expect(keyAction(k({ ctrl: true, shift: true }), 'prompt', false)).toBe('select-all');
    expect(keyAction(k({ meta: true }), 'prompt', false)).toBe('select-all');
  });
  it('is read from the physical key on a non-Latin layout, and Alt is the program\'s', () => {
    expect(keyAction(k({ ctrl: true, key: 'ф', code: 'KeyA' }), 'output', false)).toBe('select-all');
    expect(keyAction(k({ ctrl: true, alt: true }), 'output', false)).toBe('pass');
  });
});

describe('Ctrl+X', () => {
  const x = (over: Partial<KeyInfo> = {}) => k({ key: 'x', code: 'KeyX', ctrl: true, ...over });
  it('copies a selection of the output and clears it', () => {
    expect(keyAction(x(), 'output', true)).toBe('cut');
    expect(keyAction(x({ ctrl: false, meta: true }), 'output', true)).toBe('cut');
  });
  it('is the agent\'s in its prompt, and with nothing selected', () => {
    expect(keyAction(x(), 'prompt', true)).toBe('pass');
    expect(keyAction(x(), 'output', false)).toBe('pass');
    expect(keyAction(x({ shift: true }), 'output', true)).toBe('pass');
  });
});

describe('the parts of a scrollback answer', () => {
  it('are joined in order and handed over with the last one', () => {
    const p = new ScrollbackParts();
    expect(p.add(0, false, 'a\n')).toBeNull();
    expect(p.add(1, false, 'b\n')).toBeNull();
    expect(p.add(2, true, 'c')).toBe('a\nb\nc');
  });
  it('start again after a gap and never hand over half an answer', () => {
    const p = new ScrollbackParts();
    p.add(0, false, 'a');
    expect(p.add(2, true, 'c')).toBeNull();
    expect(p.add(0, true, 'x')).toBe('x');
  });
});
