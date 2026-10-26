import { describe, expect, it } from 'vitest';
import { renameOutcome } from './rename';

describe('committing a name', () => {
  it('trims what was typed and sends it', () => {
    expect(renameOutcome('  build server  ', 'alpha')).toEqual({ kind: 'rename', name: 'build server' });
  });
  it('keeps the field open for an empty or blank name', () => {
    expect(renameOutcome('', 'alpha')).toEqual({ kind: 'empty' });
    expect(renameOutcome('   \t', 'alpha')).toEqual({ kind: 'empty' });
  });
  it('sends nothing when the name is the same, even with stray spaces', () => {
    expect(renameOutcome('alpha', 'alpha')).toEqual({ kind: 'unchanged' });
    expect(renameOutcome(' alpha ', 'alpha')).toEqual({ kind: 'unchanged' });
  });
  it('treats a change of case as a new name', () => {
    expect(renameOutcome('Alpha', 'alpha')).toEqual({ kind: 'rename', name: 'Alpha' });
  });
});
