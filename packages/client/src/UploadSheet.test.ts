import { describe, expect, it } from 'vitest';
import { render } from 'svelte/server';
import UploadSheet from './UploadSheet.svelte';
import type { UploadItem } from './lib/uploadQueue';

const item = (o: Partial<UploadItem>): UploadItem => ({ id: 1, name: 'a.txt', size: 2048, collision: 'keep_both', state: 'confirm', received: 0, ...o });
const noop = () => {};
const html = (items: UploadItem[], started = false) =>
  render(UploadSheet, { props: { tabName: 'Shell', items, started, onpick: noop, onconfirm: noop, oncollision: noop, onretry: noop, oncancel: noop, onclose: noop } }).body;

describe('the upload sheet', () => {
  it('asks for files when empty and cannot start', () => {
    const out = html([]);
    expect(out).toContain('Upload to Shell');
    expect(out).toContain('Choose files');
    expect(out).toMatch(/<button[^>]*disabled[^>]*>Upload/);
  });
  it('before confirming, shows the choices, the unresolved folder and the same-action box, and no progress', () => {
    const out = html([item({}), item({ id: 2, name: 'b.txt' })]);
    expect(out).toContain('Keep both');
    expect(out).toContain('Replace');
    expect(out).toContain('Skip');
    expect(out).toContain('The server picks the folder');
    expect(out).toContain('Same action for the rest');
    expect(out).toContain('Upload 2 files');
    expect(out).not.toContain('<progress');
  });
  it('shows the pinned folder and progress while uploading', () => {
    const out = html([item({ state: 'uploading', received: 1024, dir: '/home/u/app' })], true);
    expect(out).toContain('/home/u/app');
    expect(out).toContain('>Uploading<');
    expect(out).toContain(': 50%');
    expect(out).toContain('<progress');
    expect(out).toContain('Cancel upload');
  });
  it('renders every other state with its text', () => {
    expect(html([item({ state: 'queued' })], true)).toContain('Queued');
    expect(html([item({ state: 'paused' })], true)).toContain('Paused: reconnecting');
    expect(html([item({ state: 'denied', reason: 'Outside your home' })], true)).toContain('Outside your home');
    const failed = html([item({ state: 'failed', reason: 'Disk full' })], true);
    expect(failed).toContain('Disk full');
    expect(failed).toContain('Retry');
    expect(html([item({ state: 'cancelled' })], true)).toContain('Cancelled');
    const done = html([item({ state: 'done', path: '/home/u/a (1).txt', dir: '/home/u' })], true);
    expect(done).toContain('/home/u/a (1).txt');
    expect(done).toContain('>Close<');
  });
});
