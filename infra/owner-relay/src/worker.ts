// The owner's signaling Worker (DECISION-0140): `GET /s/<keyhash>` upgrades to a WebSocket and joins the Durable Object
// of that server. It holds no secret and answers nothing else.
export { Relay } from './relay';

interface Env { RELAY: { idFromName(n: string): unknown; get(id: unknown): { fetch(r: Request): Promise<Response> } } }

const ROUTE = /^\/s\/([A-Za-z0-9_-]{16,64})$/;

export default {
  async fetch(req: Request, env: Env): Promise<Response> {
    const url = new URL(req.url);
    const m = ROUTE.exec(url.pathname);
    if (!m || req.method !== 'GET') return new Response('not found', { status: 404 });
    if (req.headers.get('upgrade')?.toLowerCase() !== 'websocket') return new Response('expected a WebSocket', { status: 426 });
    // The source address comes from Cloudflare, never from the caller: the object rate-limits by it.
    const headers = new Headers(req.headers);
    headers.set('x-source', req.headers.get('cf-connecting-ip') ?? 'unknown');
    const stub = env.RELAY.get(env.RELAY.idFromName(m[1]));
    return stub.fetch(new Request(req.url, { method: 'GET', headers }));
  },
};
