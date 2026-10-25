// Drives the real client Connection from a test: no Svelte UI, same transport, auth and reconnect code.
import { mount } from 'svelte';
import '../src/app.css';
import Terminal from '../src/Terminal.svelte';
import { Connection, b64ToBytes, textToB64, type ConnState } from '../src/lib/connection';
import { deviceKey, publicKeyB64, signChallenge } from '../src/lib/device';
import type { ClientMessage, ServerMessage } from '../src/lib/protocol';

// Keep every peer connection so a test can cut the transport the way a network drop would.
const peers: RTCPeerConnection[] = [];
const NativePeer = window.RTCPeerConnection;
window.RTCPeerConnection = class extends NativePeer {
  constructor(...args: ConstructorParameters<typeof NativePeer>) {
    super(...args);
    peers.push(this);
  }
} as typeof RTCPeerConnection;

interface Opts {
  record: { ip: string; udp_port: number; fingerprint: string };
  /** Certificate for the key `deviceKey()` returned, issued by the test's stand-in function. */
  cert: string;
  serverId: string;
  claim?: string;
}

const decoder = new TextDecoder();
let connection: Connection | null = null;
let activeWorkspace: string | null = null; // re-attached after a reconnect, as the app does
let mounted = false; // the Terminal component sizes the workspace itself

const api = {
  /** Public key of this browser's device key, for the test to have a certificate issued. */
  deviceKey: async () => publicKeyB64(await deviceKey()),
  state: 'idle' as ConnState | 'idle',
  ready: 0,
  fatal: '',
  output: '',
  /** Every `input` message the client sent, decoded, so a test can see which key or mouse reports a gesture produced. */
  sent: [] as string[],
  /** Every other message the client sent (attach, resize, ...). */
  control: [] as string[],
  /** Every server message except terminal output, for diagnosing a test. */
  received: [] as string[],
  workspaces: [] as { id: string; name: string }[],
  kind: () => connection?.kind ?? null,
  start(opts: Opts) {
    connection = new Connection({
      target: async () => ({ record: opts.record as never }),
      credentials: async () => {
        const pair = await deviceKey();
        return { cert: opts.cert, sign: (id, nonce) => signChallenge(pair, id, nonce) };
      },
      serverId: opts.serverId,
      claim: opts.claim,
      onState: (s) => (api.state = s),
      onReady: (_msg, isReconnect) => {
        api.ready++;
        if (isReconnect) {
          connection?.send({ type: 'list_workspaces' });
          if (activeWorkspace) connection?.send({ type: 'attach', workspace_id: activeWorkspace });
        }
      },
      onFatal: (m) => (api.fatal = m),
    });
    connection.subscribe((m: ServerMessage) => {
      if (m.type !== 'output' && m.type !== 'term') api.received.push(JSON.stringify(m).slice(0, 160));
      if (m.type === 'output') api.output += decoder.decode(b64ToBytes(m.data), { stream: true });
      if (m.type === 'term') api.output += decoder.decode(m.bytes, { stream: true });
      if (m.type === 'workspaces') api.workspaces = m.workspaces;
      if (m.type === 'workspace_attached' && !mounted) connection?.send({ type: 'resize', cols: 120, rows: 30 });
    });
    const realSend = connection.send.bind(connection);
    connection.send = (m: ClientMessage) => {
      if (m.type === 'input') api.sent.push(atob(m.data));
      else {
        api.control.push(JSON.stringify(m));
        if (m.type === 'attach') activeWorkspace = m.workspace_id;
      }
      return realSend(m);
    };
    connection.start();
  },
  /** The real Terminal component on this connection, full screen, for touch and mouse tests. */
  mountTerminal(sessionId: string) {
    const host = document.createElement('div');
    host.id = 'termhost';
    host.style.cssText = 'position:fixed;inset:0;display:flex;flex-direction:column;background:#000';
    document.body.append(host);
    mounted = true;
    mount(Terminal, { target: host, props: { connection: connection!, activeWorkspaceId: sessionId, connState: 'connected' } });
  },
  send: (msg: ClientMessage) => connection?.send(msg),
  type: (text: string) => connection?.send({ type: 'input', data: textToB64(text) }),
  clear: () => (api.output = ''),
  dropTransport: () => peers.forEach((p) => p.close()),
};

(window as unknown as { __e2e: typeof api }).__e2e = api;
