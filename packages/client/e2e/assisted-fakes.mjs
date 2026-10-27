// Loopback stand-ins for everything the assisted path (DECISION-0141) reaches outside the machine, for e2e/assisted.mjs:
//   * `startNat`    a STUN server whose answers put each host behind a public alias, and the aliases that forward between
//                   hosts like a cone NAT. `nat.cut = true` drops every forwarded packet: the punch fails.
//   * `startTurn`   a TURN server (RFC 5766, long-term credentials, UDP) with Send/Data indications and channels.
//   * `startWorker` the owner's signaling Worker: the real Switchboard from infra/owner-relay behind a minimal WebSocket
//                   server that sends binary frames, as the Durable Object does. Listens on plain ws (the server dials it,
//                   TERMENBOX_SIGNAL_DIAL) and on wss with a throwaway certificate (the browser dials it).
//   * `startCredentials` Cloudflare's credential endpoint (TERMENBOX_RELAY_API).
import dgram from 'node:dgram';
import { createServer as createHttp } from 'node:http';
import { createServer as createHttps } from 'node:https';
import { createHash, createHmac } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import { mkdtempSync, readFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { Switchboard } from '../../../infra/owner-relay/src/switchboard.ts';

const COOKIE = 0x2112a442;
const bind = (type = 'udp4') => new Promise((resolve) => { const s = dgram.createSocket(type); s.bind(0, '127.0.0.1', () => resolve(s)); });

// --- STUN and the NAT between the hosts -----------------------------------------------------------------------

function xorAddress(attr, ip, port, txid) {
  const v = Buffer.alloc(8);
  v[1] = 1;
  v.writeUInt16BE(port ^ (COOKIE >>> 16), 2);
  v.writeUInt32BE((ip.split('.').reduce((a, b) => (a << 8) + Number(b), 0) ^ COOKIE) >>> 0, 4);
  return attribute(attr, v);
}
function attribute(type, value) {
  const h = Buffer.alloc(4);
  h.writeUInt16BE(type, 0);
  h.writeUInt16BE(value.length, 2);
  return Buffer.concat([h, value, Buffer.alloc((4 - (value.length % 4)) % 4)]);
}
function message(type, txid, ...attrs) {
  const body = Buffer.concat(attrs);
  const h = Buffer.alloc(20);
  h.writeUInt16BE(type, 0);
  h.writeUInt16BE(body.length, 2);
  h.writeUInt32BE(COOKIE, 4);
  txid.copy(h, 8);
  return Buffer.concat([h, body]);
}

/** A STUN server and the NATs it implies: a host that asks "what is my address" gets the address of an alias socket, and
 *  a packet one host sends to another host's alias leaves through its own alias, so each side sees the other's alias as
 *  the source. Nothing else is reachable through them. */
export async function startNat() {
  const stun = await bind();
  const nat = { cut: false, port: stun.address().port, aliases: 0, forwarded: 0, byReal: new Map(), stop };
  const key = (r) => `${r.address}:${r.port}`;
  async function aliasFor(real) {
    const k = key(real);
    if (nat.byReal.has(k)) return nat.byReal.get(k);
    const sock = await bind();
    const entry = { real, sock };
    nat.byReal.set(k, entry);
    nat.aliases++;
    sock.on('message', (data, from) => {
      // `from` sent to this alias: send it on to the host behind it, from the sender's own alias.
      const sender = nat.byReal.get(key(from));
      if (nat.cut || !sender) return;
      nat.forwarded++;
      sender.sock.send(data, real.port, real.address);
    });
    return entry;
  }
  stun.on('message', async (data, from) => {
    if (data.length < 20 || data.readUInt16BE(0) !== 0x0001) return;
    const alias = await aliasFor(from);
    const a = alias.sock.address();
    stun.send(message(0x0101, data.subarray(8, 20), xorAddress(0x0020, a.address, a.port)), from.port, from.address);
  });
  function stop() { stun.close(); for (const { sock } of nat.byReal.values()) sock.close(); }
  return nat;
}

// --- TURN -----------------------------------------------------------------------------------------------------

export const TURN_USER = 'e2e-user';
export const TURN_PASSWORD = 'e2e-secret';
const REALM = 'termenbox.e2e';
const NONCE = 'e2e-nonce-0123456789';

function parse(buf) {
  const attrs = new Map();
  let off = 20;
  const end = 20 + buf.readUInt16BE(2);
  while (off + 4 <= end) {
    const type = buf.readUInt16BE(off);
    const len = buf.readUInt16BE(off + 2);
    attrs.set(type, { value: buf.subarray(off + 4, off + 4 + len), at: off });
    off += 4 + len + ((4 - (len % 4)) % 4);
  }
  return { type: buf.readUInt16BE(0), txid: buf.subarray(8, 20), attrs };
}
const decodeXor = (v) => ({ port: v.readUInt16BE(2) ^ (COOKIE >>> 16), address: [24, 16, 8, 0].map((s) => ((v.readUInt32BE(4) ^ COOKIE) >>> s) & 255).join('.') });
const lifetime = (n) => attribute(0x000d, Buffer.from([n >>> 24, (n >>> 16) & 255, (n >>> 8) & 255, n & 255]));

/** Appends MESSAGE-INTEGRITY computed with the long-term key. */
function withIntegrity(buf) {
  const key = createHash('md5').update(`${TURN_USER}:${REALM}:${TURN_PASSWORD}`).digest();
  const sized = Buffer.from(buf);
  sized.writeUInt16BE(buf.length - 20 + 24, 2);
  return Buffer.concat([sized, attribute(0x0008, createHmac('sha1', key).update(sized).digest())]);
}
function verified(buf, msg) {
  const mi = msg.attrs.get(0x0008);
  if (!mi || msg.attrs.get(0x0006)?.value.toString() !== TURN_USER) return false;
  const key = createHash('md5').update(`${TURN_USER}:${REALM}:${TURN_PASSWORD}`).digest();
  const head = Buffer.from(buf.subarray(0, mi.at));
  head.writeUInt16BE(mi.at - 20 + 24, 2);
  return createHmac('sha1', key).update(head).digest().equals(mi.value);
}

export async function startTurn() {
  const sock = await bind();
  const turn = { port: sock.address().port, allocations: 0, relayed: 0, stop };
  const byClient = new Map(); // "ip:port" of the client -> { relay, channels: Map<number, peer> }
  const key = (r) => `${r.address}:${r.port}`;
  const reply = (to, type, txid, ...attrs) => sock.send(withIntegrity(message(type, txid, ...attrs)), to.port, to.address);
  const error = (to, method, txid, code, text) => {
    const v = Buffer.concat([Buffer.from([0, 0, Math.floor(code / 100), code % 100]), Buffer.from(text)]);
    sock.send(message(method | 0x0110, txid, attribute(0x0009, v), attribute(0x0014, Buffer.from(REALM)), attribute(0x0015, Buffer.from(NONCE))), to.port, to.address);
  };

  sock.on('message', async (buf, from) => {
    if (buf.length >= 4 && (buf[0] & 0xc0) === 0x40) { // ChannelData
      const a = byClient.get(key(from));
      const peer = a?.channels.get(buf.readUInt16BE(0));
      if (peer) a.relay.send(buf.subarray(4, 4 + buf.readUInt16BE(2)), peer.port, peer.address);
      return;
    }
    if (buf.length < 20 || buf.readUInt32BE(4) !== COOKIE) return;
    const msg = parse(buf);
    const method = msg.type & 0x3eef;
    const alloc = byClient.get(key(from));
    if (msg.type === 0x0016) { // Send indication
      const peer = msg.attrs.get(0x0012), data = msg.attrs.get(0x0013);
      if (alloc && peer && data) { const p = decodeXor(peer.value); alloc.relay.send(data.value, p.port, p.address); }
      return;
    }
    if ((msg.type & 0x0110) !== 0) return; // only requests below
    if (!verified(buf, msg)) return error(from, method, msg.txid, 401, 'Unauthorized');
    if (method === 0x003) { // Allocate
      if (alloc) return error(from, method, msg.txid, 437, 'Allocation Mismatch');
      const relay = await bind();
      const entry = { relay, channels: new Map() };
      byClient.set(key(from), entry);
      turn.allocations++;
      relay.on('message', (data, peer) => {
        turn.relayed += data.length;
        const channel = [...entry.channels].find(([, p]) => p.address === peer.address && p.port === peer.port)?.[0];
        if (channel !== undefined) {
          const h = Buffer.alloc(4);
          h.writeUInt16BE(channel, 0);
          h.writeUInt16BE(data.length, 2);
          sock.send(Buffer.concat([h, data, Buffer.alloc((4 - (data.length % 4)) % 4)]), from.port, from.address);
        } else {
          sock.send(message(0x0017, crypto.getRandomValues(Buffer.alloc(12)), xorAddress(0x0012, peer.address, peer.port), attribute(0x0013, data)), from.port, from.address);
        }
      });
      const r = relay.address();
      return reply(from, 0x0103, msg.txid, xorAddress(0x0016, r.address, r.port), xorAddress(0x0020, from.address, from.port), lifetime(600));
    }
    if (!alloc) return error(from, method, msg.txid, 437, 'Allocation Mismatch');
    if (method === 0x004) { // Refresh
      const wanted = msg.attrs.get(0x000d)?.value.readUInt32BE(0) ?? 600;
      if (wanted === 0) { alloc.relay.close(); byClient.delete(key(from)); }
      return reply(from, 0x0104, msg.txid, lifetime(wanted));
    }
    if (method === 0x008) return reply(from, 0x0108, msg.txid); // CreatePermission
    if (method === 0x009) { // ChannelBind
      const channel = msg.attrs.get(0x000c).value.readUInt16BE(0);
      alloc.channels.set(channel, decodeXor(msg.attrs.get(0x0012).value));
      return reply(from, 0x0109, msg.txid);
    }
    return error(from, method, msg.txid, 400, 'Bad Request');
  });
  function stop() { sock.close(); for (const { relay } of byClient.values()) relay.close(); }
  return turn;
}

// --- the signaling Worker --------------------------------------------------------------------------------------

const GUID = '258EAFA5-E914-47DA-95CA-C5AB0DC85B11';

function frame(opcode, payload) {
  const n = payload.length;
  const head = n < 126 ? Buffer.from([0x80 | opcode, n]) : Buffer.from([0x80 | opcode, 126, n >> 8, n & 255]);
  return Buffer.concat([head, payload]);
}

/** One WebSocket connection: decodes masked client frames, answers pings, sends unmasked binary frames. */
function accept(req, socket, onMessage, onClose) {
  const hash = createHash('sha1').update(req.headers['sec-websocket-key'] + GUID).digest('base64');
  socket.write(`HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: ${hash}\r\n\r\n`);
  const conn = {
    send: (data) => socket.writable && socket.write(frame(0x2, Buffer.from(data))),
    close: () => { if (socket.writable) { socket.write(frame(0x8, Buffer.alloc(0))); socket.end(); } },
  };
  let buf = Buffer.alloc(0);
  socket.on('data', (chunk) => {
    buf = Buffer.concat([buf, chunk]);
    for (;;) {
      if (buf.length < 2) return;
      const opcode = buf[0] & 0x0f;
      let len = buf[1] & 0x7f, off = 2;
      if (len === 126) { if (buf.length < 4) return; len = buf.readUInt16BE(2); off = 4; }
      else if (len === 127) { if (buf.length < 10) return; len = Number(buf.readBigUInt64BE(2)); off = 10; }
      const masked = (buf[1] & 0x80) !== 0;
      const start = off + (masked ? 4 : 0);
      if (buf.length < start + len) return;
      const data = Buffer.from(buf.subarray(start, start + len));
      if (masked) for (let i = 0; i < data.length; i++) data[i] ^= buf[off + (i % 4)];
      buf = buf.subarray(start + len);
      if (opcode === 0x8) { conn.close(); return; }
      if (opcode === 0x9) socket.write(frame(0xa, data));
      else if (opcode === 0x1 || opcode === 0x2) onMessage(data);
    }
  });
  socket.on('close', onClose);
  socket.on('error', () => {});
  return conn;
}

/** The owner's Worker for one server route. `stop()` closes the sockets and refuses new ones: the Worker is gone. */
export async function startWorker() {
  const dir = mkdtempSync(join(tmpdir(), 'termenbox-worker-'));
  execFileSync('openssl', ['req', '-x509', '-newkey', 'ec', '-pkeyopt', 'ec_paramgen_curve:prime256v1', '-nodes', '-days', '2', '-subj', '/CN=127.0.0.1',
    '-addext', 'subjectAltName=IP:127.0.0.1', '-keyout', join(dir, 'key.pem'), '-out', join(dir, 'cert.pem')], { stdio: 'ignore' });
  const board = new Switchboard();
  const worker = { messages: [], conns: new Set(), up: true, stop };
  const upgrade = (req, socket) => {
    const url = new URL(req.url, 'http://x');
    if (!worker.up || !url.pathname.startsWith('/s/') || req.headers.upgrade?.toLowerCase() !== 'websocket') return socket.destroy();
    const role = url.searchParams.get('role') === 'server' ? 'server' : 'client';
    let id = 0;
    const source = req.socket.remoteAddress ?? 'unknown';
    let conn;
    const sock = { send: (d) => conn.send(d), close: () => conn.close() };
    conn = accept(req, socket,
      (data) => {
        worker.messages.push({ role, bytes: data.length, text: data.toString('utf8', role === 'server' ? 1 : 0).slice(0, 2000) });
        if (role === 'server') board.onServer(data); else board.onClient(id, data, Date.now());
      },
      () => { board.remove(sock); worker.conns.delete(conn); });
    worker.conns.add(conn);
    if (role === 'server') board.addServer(sock);
    else { id = board.addClient(sock, source, Date.now()); if (id === null) conn.close(); }
  };
  const plain = createHttp((_, res) => res.writeHead(426).end());
  const secure = createHttps({ key: readFileSync(join(dir, 'key.pem')), cert: readFileSync(join(dir, 'cert.pem')) }, (_, res) => res.writeHead(426).end());
  plain.on('upgrade', upgrade);
  secure.on('upgrade', upgrade);
  await Promise.all([[plain], [secure]].map(([s]) => new Promise((r) => s.listen(0, '127.0.0.1', r))));
  worker.plainPort = plain.address().port;
  worker.securePort = secure.address().port;
  function stop() {
    worker.up = false;
    for (const c of worker.conns) c.close();
    plain.close();
    secure.close();
  }
  return worker;
}

// --- Cloudflare's credential endpoint --------------------------------------------------------------------------

/** Answers `POST /v1/turn/keys/<id>/credentials/generate` with the TURN server above. `refuse = true` answers 403. */
export async function startCredentials(turnPort) {
  const api = { calls: 0, refuse: false, port: 0, stop: () => server.close() };
  const server = createHttp((req, res) => {
    req.resume();
    if (req.method !== 'POST' || !/^\/v1\/turn\/keys\/[^/]+\/credentials\/generate$/.test(req.url)) return res.writeHead(404).end();
    api.calls++;
    if (api.refuse) return res.writeHead(403).end();
    res.writeHead(201, { 'Content-Type': 'application/json' }).end(JSON.stringify({
      iceServers: { urls: [`turn:127.0.0.1:${turnPort}?transport=udp`], username: TURN_USER, credential: TURN_PASSWORD },
    }));
  });
  await new Promise((r) => server.listen(0, '127.0.0.1', r));
  api.port = server.address().port;
  return api;
}
