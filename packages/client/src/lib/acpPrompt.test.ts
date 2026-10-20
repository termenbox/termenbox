import { describe, expect, it } from 'vitest';
import { History, PROMPT_KEYS, complete, completions, enterCompletes, insertAt, keyEffect, promptsOf } from './acpPrompt';
import { KEY_BY_ID } from './keys';

const cmds = [{ name: 'compact', description: 'Compact' }, { name: 'context' }, { name: 'init' }, { name: 'review', hint: 'branch' }, { name: 'design' }];

describe('slash commands from the agent’s own list', () => {
  it('offers names that start with what was typed, then names that contain it', () => {
    expect(completions('/co', 3, cmds)?.items.map((c) => c.name)).toEqual(['compact', 'context']);
    expect(completions('/', 1, cmds)?.items).toHaveLength(5);
    expect(completions('/si', 3, cmds)?.items.map((c) => c.name)).toEqual(['design']);
  });
  it('offers nothing off the first word, for a plain prompt, or when nothing matches', () => {
    expect(completions('hello /co', 9, cmds)).toBeNull();
    expect(completions('/compact now', 12, cmds)).toBeNull();
    expect(completions('/zzz', 4, cmds)).toBeNull();
    expect(completions('/co', 3, [])).toBeNull();
  });
  it('choosing one keeps any argument already typed', () => {
    expect(complete('/rev', cmds[3])).toEqual({ text: '/review ', cursor: 8 });
    expect(complete('/rev main', cmds[3])).toEqual({ text: '/review main', cursor: 8 });
  });
  it('Enter completes a half-typed name and sends a finished one', () => {
    expect(enterCompletes('/co', cmds[0])).toBe(true);
    expect(enterCompletes('/compact', cmds[0])).toBe(false);
    expect(enterCompletes('/compact now', cmds[0])).toBe(false);
    expect(enterCompletes('hi', cmds[0])).toBe(false);
    expect(enterCompletes('/co', undefined)).toBe(false);
  });
});

describe('prompt history', () => {
  it('walks earlier prompts like a shell and comes back to the draft', () => {
    const h = new History(['one', 'two', 'three']);
    expect(h.previous('draft')).toBe('three');
    expect(h.previous('x')).toBe('two');
    expect(h.next()).toBe('three');
    expect(h.next()).toBe('draft');
    expect(h.browsing).toBe(false);
    expect(h.next()).toBeNull();
  });
  it('stops at the oldest and handles an empty history', () => {
    const h = new History(['only']);
    expect(h.previous('')).toBe('only');
    expect(h.previous('')).toBeNull();
    expect(new History([]).previous('x')).toBeNull();
  });
  it('is built from the prompts the owner sent, without repeats in a row', () => {
    expect(promptsOf([{ kind: 'user', text: 'a' }, { kind: 'message', text: 'm' }, { kind: 'user', text: 'a' }, { kind: 'user', text: 'b' }])).toEqual(['a', 'b']);
  });
});

describe('the key row at the prompt', () => {
  const st = (over = {}) => ({ working: false, popup: false, draft: '', cursor: 0, ...over });
  it('Esc and ^C interrupt a running turn; a popup takes Esc first', () => {
    expect(keyEffect('\x1b', st({ working: true }))).toEqual({ do: 'interrupt' });
    expect(keyEffect('\x03', st({ working: true }))).toEqual({ do: 'interrupt' });
    expect(keyEffect('\x1b', st({ working: true, popup: true }))).toEqual({ do: 'close-popup' });
    expect(keyEffect('\x1b', st())).toEqual({ do: 'none' });
  });
  it('^C clears a draft when nothing is running', () => {
    expect(keyEffect('\x03', st({ draft: 'abc' }))).toEqual({ do: 'clear' });
    expect(keyEffect('\x03', st())).toEqual({ do: 'none' });
  });
  it('Tab accepts, arrows move the popup or walk the history', () => {
    expect(keyEffect('\t', st({ popup: true }))).toEqual({ do: 'accept' });
    expect(keyEffect('\t', st())).toEqual({ do: 'none' });
    expect(keyEffect('\x1b[A', st({ popup: true }))).toEqual({ do: 'move', by: -1 });
    expect(keyEffect('\x1b[B', st())).toEqual({ do: 'history', by: 1 });
    expect(keyEffect('\x1b[D', st())).toEqual({ do: 'cursor', to: 'left' });
  });
  it('printable keys are typed, anything else is ignored', () => {
    expect(keyEffect('/', st())).toEqual({ do: 'insert', text: '/' });
    expect(keyEffect('\x1b[99~', st())).toEqual({ do: 'none' });
    expect(keyEffect('', st())).toEqual({ do: 'none' });
  });
  it('the row only names keys that exist in the catalogue', () => {
    for (const id of PROMPT_KEYS) expect(KEY_BY_ID.has(id), id).toBe(true);
  });
  it('inserts at the cursor', () => {
    expect(insertAt('abcd', 2, '/')).toEqual({ text: 'ab/cd', cursor: 3 });
    expect(insertAt('abcd', 99, 'x')).toEqual({ text: 'abcdx', cursor: 5 });
  });
});
