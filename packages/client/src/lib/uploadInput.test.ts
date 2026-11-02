import { describe, it, expect } from 'vitest';
import { dragHasFiles, filesOf, pastedName, takesPaste } from './uploadInput';

const file = { name: 'a.png', size: 1, slice: () => ({ arrayBuffer: async () => new ArrayBuffer(0) }) };

describe('upload input', () => {
  it('takes a paste only when it holds a file and no text', () => {
    expect(takesPaste({ files: [file], getData: () => '' })).toBe(true);
    expect(takesPaste({ files: [file], getData: () => 'some text' })).toBe(false);
    expect(takesPaste({ files: [], getData: () => 'ls -la' })).toBe(false);
    expect(takesPaste(null)).toBe(false);
  });
  it('recognises a drag that carries files', () => {
    expect(dragHasFiles({ types: ['Files'] })).toBe(true);
    expect(dragHasFiles({ types: ['text/plain'] })).toBe(false);
    expect(filesOf({ files: [file] })).toHaveLength(1);
  });
  it('names a pasted screenshot by time', () => {
    expect(pastedName('image.png', 'image/png', new Date(2026, 10, 2, 3, 4, 5))).toBe('pasted-20261102-030405.png');
    expect(pastedName('report.pdf', 'application/pdf')).toBe('report.pdf');
  });
});
