import { describe, expect, it } from 'vitest';
import { backAction } from './backNavigation';

describe('backAction', () => {
  it('leaves the open workspace first', () => {
    expect(backAction({ inWorkspace: true, sheetOpen: true })).toBe('workspace-list');
  });
  it('closes an open sheet when no workspace is open', () => {
    expect(backAction({ inWorkspace: false, sheetOpen: true })).toBe('close-sheet');
  });
  it('is not consumed otherwise', () => {
    expect(backAction({ inWorkspace: false, sheetOpen: false })).toBe('none');
  });
});
