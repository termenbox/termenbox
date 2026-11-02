import { describe, it, expect } from 'vitest';
import { UploadQueue, cleanUploadName, encodeUploadFrame, UPLOAD_WINDOW_BYTES, UPLOAD_CHUNK_MAX, UPLOAD_MAX_BATCH, type UploadSource } from './uploadQueue';
import type { ClientMessage, ServerMessage } from './protocol';

const src = (name: string, size: number): UploadSource => ({
  name, size,
  slice: (a, b) => ({ arrayBuffer: async () => new Uint8Array(b - a).fill(7).buffer }),
});
const tick = () => new Promise(r => setTimeout(r, 0));

function rig(files: UploadSource[] = []) {
  const sent: ClientMessage[] = [];
  const frames: { id: number; offset: number; len: number }[] = [];
  let open = true;
  const q = new UploadQueue({
    workspace: 'w', tab: 't',
    send: m => sent.push(m),
    sendChunk: f => {
      if (!open) return false;
      const v = new DataView(f.buffer);
      frames.push({ id: v.getUint32(0), offset: Number(v.getBigUint64(4)), len: f.length - 12 });
      return true;
    },
  });
  q.add(files);
  return { q, sent, frames, close: () => { open = false; }, reopen: () => { open = true; } };
}
const inflight = (frames: { offset: number; len: number }[], acked: number) =>
  Math.max(0, ...frames.map(f => f.offset + f.len)) - acked;

describe('cleanUploadName', () => {
  it('replaces what the server refuses', () => {
    expect(cleanUploadName('a/b\\c.txt')).toBe('a_b_c.txt');
    expect(cleanUploadName('.env')).toBe('_env');
    expect(cleanUploadName('  x.png ')).toBe('x.png');
    expect(cleanUploadName('a\u0000b')).toBe('a_b');
    expect(cleanUploadName('')).toBe('file');
    const long = cleanUploadName('é'.repeat(300) + '.png');
    expect(new TextEncoder().encode(long).length).toBeLessThanOrEqual(255);
    expect(long.endsWith('.png')).toBe(true);
  });
  it('is applied when a file is added', () => {
    const { q } = rig([src('../x', 1)]);
    expect(q.items[0].name).toBe('__/x'.replace('/', '_'));
  });
});

describe('encodeUploadFrame', () => {
  it('writes id and offset big-endian before the payload and refuses an oversize chunk', () => {
    const f = encodeUploadFrame(1, 258, new Uint8Array([9]));
    expect([...f]).toEqual([0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 1, 2, 9]);
    expect(() => encodeUploadFrame(1, 0, new Uint8Array(UPLOAD_CHUNK_MAX + 1))).toThrow();
  });
});

describe('UploadQueue', () => {
  it('sends nothing before the user confirms', () => {
    const { q, sent } = rig([src('a.txt', 10)]);
    expect(sent).toEqual([]);
    expect(q.items[0].state).toBe('confirm');
    q.start();
    expect(sent).toEqual([{ type: 'upload_start', id: 1, workspace: 'w', tab: 't', name: 'a.txt', size: 10, collision: 'keep_both' }]);
  });

  it('never has more than the window unacknowledged, then finishes once all is acknowledged', async () => {
    const size = UPLOAD_WINDOW_BYTES * 2 + 5;
    const { q, sent, frames } = rig([src('big.bin', size)]);
    q.start();
    q.handle({ type: 'upload_ready', id: 1, dir: '/home/u', offset: 0 });
    await tick(); await tick();
    expect(inflight(frames, 0)).toBe(UPLOAD_WINDOW_BYTES);
    let acked = 0;
    while (acked < size) {
      acked = Math.min(size, acked + 100_000);
      q.handle({ type: 'upload_progress', id: 1, received: acked });
      for (let i = 0; i < 20; i++) await tick();
      expect(inflight(frames, acked)).toBeLessThanOrEqual(UPLOAD_WINDOW_BYTES);
    }
    expect(sent.filter(m => m.type === 'upload_finish')).toEqual([{ type: 'upload_finish', id: 1 }]);
    expect(frames.every(f => f.len <= UPLOAD_CHUNK_MAX)).toBe(true);
    q.handle({ type: 'upload_done', id: 1, path: '/home/u/big.bin', size });
    expect(q.items[0]).toMatchObject({ state: 'done', path: '/home/u/big.bin' });
  });

  it('resumes from the server offset after a drop', async () => {
    const { q, sent, frames, close, reopen } = rig([src('a.bin', 100_000)]);
    q.start();
    q.handle({ type: 'upload_ready', id: 1, dir: '/d', offset: 0 });
    await tick(); await tick();
    q.handle({ type: 'upload_progress', id: 1, received: 40_000 });
    q.pause();
    close(); reopen();
    frames.length = 0;
    q.resume();
    expect(sent.at(-1)).toEqual({ type: 'upload_resume', id: 1 });
    q.handle({ type: 'upload_ready', id: 1, dir: '/d', offset: 32_768 });
    await tick(); await tick();
    expect(frames[0].offset).toBe(32_768);
    expect(q.items[0].state).toBe('uploading');
  });

  it('pauses when the transport cannot carry a chunk', async () => {
    const { q, close } = rig([src('a.bin', 100)]);
    q.start();
    close();
    q.handle({ type: 'upload_ready', id: 1, dir: '/d', offset: 0 });
    await tick();
    expect(q.items[0].state).toBe('paused');
  });

  it('applies "same action for the rest" to later files, not earlier ones', () => {
    const { q, sent } = rig([src('a', 1), src('b', 1), src('c', 1)]);
    q.setCollision(2, 'replace', true);
    q.add([src('d', 1)]);
    expect(q.items.map(i => i.collision)).toEqual(['keep_both', 'replace', 'replace', 'replace']);
    q.start();
    expect((sent[0] as { collision: string }).collision).toBe('keep_both');
  });

  it('continues with the next file after a skip (denied) and after a failure', () => {
    const { q, sent } = rig([src('a', 1), src('b', 1), src('c', 1)]);
    q.start();
    q.handle({ type: 'upload_denied', id: 1, reason: 'A file with that name is already there; skipped' });
    expect(q.items[0]).toMatchObject({ state: 'denied' });
    expect(sent.at(-1)).toMatchObject({ type: 'upload_start', id: 2 });
    q.handle({ type: 'upload_failed', id: 2, reason: 'disk' });
    expect(sent.at(-1)).toMatchObject({ type: 'upload_start', id: 3 });
  });

  it('cancel stops the active upload and clears the queue', () => {
    const { q, sent } = rig([src('a', 1), src('b', 1)]);
    q.start();
    q.cancel();
    expect(sent.at(-1)).toEqual({ type: 'upload_cancel', id: 1 });
    expect(q.items.map(i => i.state)).toEqual(['cancelled', 'cancelled']);
    q.handle({ type: 'upload_cancelled', id: 1 });
    expect(sent.filter(m => m.type === 'upload_start')).toHaveLength(1);
  });

  it('sends finish for an empty file and retries a failed one', () => {
    const { q, sent } = rig([src('e', 0)]);
    q.start();
    q.handle({ type: 'upload_ready', id: 1, dir: '/d', offset: 0 });
    expect(sent.at(-1)).toEqual({ type: 'upload_finish', id: 1 });
    q.handle({ type: 'upload_failed', id: 1, reason: 'x' });
    q.retry(1);
    expect(sent.at(-1)).toMatchObject({ type: 'upload_start', id: 1 });
  });

  it('keeps at most one batch', () => {
    const { q } = rig();
    const n = q.add(Array.from({ length: UPLOAD_MAX_BATCH + 5 }, (_, i) => src(`f${i}`, 1)));
    expect(n).toBe(UPLOAD_MAX_BATCH);
  });

  it('ignores messages that are not about uploads', () => {
    const { q } = rig();
    expect(q.handle({ type: 'dir_error', reason: 'x' } as ServerMessage)).toBe(false);
  });
});
