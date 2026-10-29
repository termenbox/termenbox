import { describe, expect, it } from 'vitest';
import { render } from 'svelte/server';
import ServerStatusIcon from './ServerStatusIcon.svelte';
import type { HubServer } from './lib/hub';

const out = (state: HubServer['state']) => render(ServerStatusIcon, { props: { state, name: 'box' } }).body;

describe('the server status icon', () => {
  it('names every state for a screen reader and a tooltip', () => {
    const words: Record<HubServer['state'], string> = {
      connecting: 'box: Connecting', connected: 'box: Connected', checking: 'box: Checking',
      reconnecting: 'box: Reconnecting', unreachable: 'box: Unreachable', closed: 'box: Closed', paused: 'box: Paused',
    };
    for (const [state, word] of Object.entries(words)) {
      const body = out(state as HubServer['state']);
      expect(body).toContain(`aria-label="${word}"`);
      expect(body).toContain(`title="${word}"`);
    }
  });
  it('does not put the word in the row as text', () => {
    const body = out('connected');
    expect(body).not.toContain('>Connected<');
  });
});
