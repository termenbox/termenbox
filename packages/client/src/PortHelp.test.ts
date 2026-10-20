import { describe, expect, it } from 'vitest';
import { render } from 'svelte/server';
import PortHelp from './PortHelp.svelte';

const html = (port: number | null) => render(PortHelp as never, { props: { port } }).body;

describe('the help for a blocked port', () => {
  it('names the server\'s own port in both firewall commands and the cloud firewall', () => {
    const out = html(31863);
    expect(out).toContain('ufw allow 31863/udp');
    expect(out).toContain('--add-port=31863/udp');
    expect(out).toContain('security group');
  });

  it('falls back to the range when the port is not known', () => {
    expect(html(null)).toContain('ufw allow 4443:4542/udp');
  });
});
