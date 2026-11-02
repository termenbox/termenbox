import type { ClientMessage, ServerMessage, UploadCollision } from './protocol';

// Mirrors the limits in packages/core/src/protocol.rs.
export const UPLOAD_WINDOW_BYTES = 1024 * 1024;
export const UPLOAD_CHUNK_MAX = 16 * 1024;
export const UPLOAD_FRAME_HEADER = 12;
export const UPLOAD_MAX_BATCH = 100;
const NAME_MAX = 255;

/** The binary frame of the upload lane: upload id (u32) and byte offset (u64), big-endian, then the payload. */
export function encodeUploadFrame(id: number, offset: number, data: Uint8Array): Uint8Array {
  if (data.length > UPLOAD_CHUNK_MAX) throw new Error('upload chunk too large');
  const out = new Uint8Array(UPLOAD_FRAME_HEADER + data.length);
  const view = new DataView(out.buffer);
  view.setUint32(0, id);
  view.setBigUint64(4, BigInt(offset));
  out.set(data, UPLOAD_FRAME_HEADER);
  return out;
}

/** Replaces what the server would refuse in a file name (separators, control characters, a leading dot, edge spaces, length). */
export function cleanUploadName(name: string): string {
  let n = name.replace(/[/\\\u0000-\u001f\u007f-\u009f]/g, '_').trim().replace(/^\.+/, m => '_'.repeat(m.length));
  const enc = new TextEncoder();
  if (enc.encode(n).length > NAME_MAX) {
    const dot = n.lastIndexOf('.');
    const ext = dot > 0 && n.length - dot <= 16 ? n.slice(dot) : '';
    let stem = [...n.slice(0, ext ? dot : n.length)];
    while (stem.length && enc.encode(stem.join('')).length + enc.encode(ext).length > NAME_MAX) stem.pop();
    n = (stem.join('') + ext).trim();
  }
  return n || 'file';
}

export type UploadState = 'confirm' | 'queued' | 'uploading' | 'paused' | 'denied' | 'failed' | 'cancelled' | 'done';

/** A file as the queue needs it: a browser `File` fits. */
export interface UploadSource { name: string; size: number; slice(start: number, end: number): { arrayBuffer(): Promise<ArrayBuffer> } }

export interface UploadItem {
  id: number;
  name: string;
  size: number;
  collision: UploadCollision;
  state: UploadState;
  received: number;
  /** The folder the server pinned; known after `upload_ready`. */
  dir?: string;
  /** The final path, after `upload_done`. */
  path?: string;
  reason?: string;
}

export interface UploadDeps {
  workspace: string;
  tab: string;
  send: (m: ClientMessage) => void;
  /** Sends one binary upload frame; false when the transport cannot carry it now. */
  sendChunk: (frame: Uint8Array) => boolean;
  firstId?: number;
}

/**
 * Files for one tab, uploaded one at a time. Nothing starts before `start()` (the user's confirmation). At most
 * `UPLOAD_WINDOW_BYTES` are sent and not yet acknowledged; after a dropped link `resume()` asks the server where it got to.
 */
export class UploadQueue {
  items: UploadItem[] = [];
  private sources = new Map<number, UploadSource>();
  private nextId: number;
  private started = false;
  private sent = 0;
  private pumping = false;
  private defaultCollision: UploadCollision = 'keep_both';
  private listeners = new Set<() => void>();

  constructor(private deps: UploadDeps) { this.nextId = deps.firstId ?? 1; }

  subscribe(fn: () => void): () => void { this.listeners.add(fn); return () => this.listeners.delete(fn); }
  private changed() { for (const l of this.listeners) l(); }

  /** Adds files in confirm state; returns how many fit under the batch limit. */
  add(files: UploadSource[]): number {
    let n = 0;
    for (const f of files) {
      if (this.items.length >= UPLOAD_MAX_BATCH) break;
      const id = this.nextId++;
      this.items.push({ id, name: cleanUploadName(f.name), size: f.size, collision: this.defaultCollision, state: this.started ? 'queued' : 'confirm', received: 0 });
      this.sources.set(id, f);
      n++;
    }
    this.changed();
    if (this.started) this.next();
    return n;
  }

  /** Sets the collision choice for one file; with `rest`, for every later waiting file and for files added after. */
  setCollision(id: number, collision: UploadCollision, rest = false) {
    const i = this.items.findIndex(x => x.id === id);
    if (i < 0) return;
    this.items[i].collision = collision;
    if (rest) {
      this.defaultCollision = collision;
      for (const it of this.items.slice(i + 1)) if (it.state === 'confirm' || it.state === 'queued') it.collision = collision;
    }
    this.changed();
  }

  /** The user confirmed: queue everything and begin. */
  start() {
    if (this.started) return;
    this.started = true;
    for (const it of this.items) if (it.state === 'confirm') it.state = 'queued';
    this.changed();
    this.next();
  }

  private get active(): UploadItem | undefined { return this.items.find(x => x.state === 'uploading' || x.state === 'paused'); }

  private next() {
    if (!this.started || this.active) return;
    const it = this.items.find(x => x.state === 'queued');
    if (!it) return;
    it.state = 'uploading';
    this.sent = 0;
    this.deps.send({ type: 'upload_start', id: it.id, workspace: this.deps.workspace, tab: this.deps.tab, name: it.name, size: it.size, collision: it.collision });
    this.changed();
  }

  /** Feed every server message; true when it was an upload reply. */
  handle(msg: ServerMessage): boolean {
    if (!('type' in msg) || !msg.type.startsWith('upload_')) return false;
    const it = this.items.find(x => x.id === (msg as { id: number }).id);
    if (!it) return true;
    switch (msg.type) {
      case 'upload_ready':
        it.dir = msg.dir;
        it.received = this.sent = msg.offset;
        it.state = 'uploading';
        void this.pump(it);
        break;
      case 'upload_progress':
        it.received = Math.max(it.received, msg.received);
        if (it.size > 0 && it.received >= it.size && this.sent >= it.size) this.finish(it);
        else void this.pump(it);
        break;
      case 'upload_done':
        it.state = 'done'; it.path = msg.path; it.received = it.size;
        this.next();
        break;
      case 'upload_denied': it.state = 'denied'; it.reason = msg.reason; this.next(); break;
      case 'upload_failed': it.state = 'failed'; it.reason = msg.reason; this.next(); break;
      case 'upload_cancelled': it.state = 'cancelled'; this.next(); break;
    }
    this.changed();
    return true;
  }

  private finished = new Set<number>();
  private finish(it: UploadItem) {
    if (this.finished.has(it.id)) return;
    this.finished.add(it.id);
    this.deps.send({ type: 'upload_finish', id: it.id });
  }

  private async pump(it: UploadItem) {
    if (this.pumping) return;
    this.pumping = true;
    try {
      const src = this.sources.get(it.id);
      if (!src) return;
      if (it.size === 0) { this.finish(it); return; }
      while (it.state === 'uploading' && this.sent < it.size && this.sent - it.received < UPLOAD_WINDOW_BYTES) {
        const start = this.sent;
        const end = Math.min(start + UPLOAD_CHUNK_MAX, it.size, it.received + UPLOAD_WINDOW_BYTES);
        let data: Uint8Array;
        try { data = new Uint8Array(await src.slice(start, end).arrayBuffer()); }
        catch { it.state = 'failed'; it.reason = 'The file could not be read'; this.deps.send({ type: 'upload_cancel', id: it.id }); this.next(); this.changed(); return; }
        // A resume or cancel may have moved on while the read was pending.
        if (it.state !== 'uploading' || this.sent !== start) continue;
        if (!this.deps.sendChunk(encodeUploadFrame(it.id, start, data))) { it.state = 'paused'; this.changed(); return; }
        this.sent = end;
      }
      this.changed();
    } finally { this.pumping = false; }
  }

  /** The connection dropped: keep the file and wait for `resume()`. */
  pause() {
    const it = this.active;
    if (it) { it.state = 'paused'; this.changed(); }
  }

  /** The connection is back: ask the server for its offset; `upload_ready` continues from there. */
  resume() {
    const it = this.active;
    if (!it) return;
    it.state = 'uploading';
    this.finished.delete(it.id);
    this.deps.send({ type: 'upload_resume', id: it.id });
    this.changed();
  }

  /** Stops the active upload, deleting its partial, and drops everything still waiting. */
  cancel() {
    const it = this.active;
    for (const x of this.items) if (x.state === 'queued' || x.state === 'confirm') x.state = 'cancelled';
    if (it) { it.state = 'cancelled'; this.deps.send({ type: 'upload_cancel', id: it.id }); }
    this.changed();
  }

  /** Puts a denied or failed file back at the end of the queue (the Retry button). */
  retry(id: number) {
    const it = this.items.find(x => x.id === id);
    if (!it || (it.state !== 'failed' && it.state !== 'denied' && it.state !== 'cancelled')) return;
    it.state = 'queued'; it.received = 0; it.reason = undefined;
    this.finished.delete(id);
    this.changed();
    this.next();
  }
}
