// WebRTC-direct client (DECISION-0057): connects to the server's single UDP port with no signaling.
// The remote description is built locally from the signed directory record, and the server's DTLS
// fingerprint from that record is the only thing the browser trusts.
import type { DirRecord } from './directory';
import type { Transport } from './transport';

/** Fixed by the protocol; must match packages/server/src/rtc.rs. */
export const SERVER_UFRAG = 'termenbox';
export const SERVER_PWD = 'termenbox-direct-v1-0123456789';
export const CHANNEL_ID = 0;
/** The channel for terminal bytes; an older server never creates it, and nothing is sent on it unless `auth_ok` lists it. */
export const TERM_CHANNEL_ID = 1;
/** The channel for control messages (cancel, answers, window and workspace changes); used only when `auth_ok` says `control_channel`. */
export const CONTROL_CHANNEL_ID = 2;
/** The channel for upload frames, binary and client to server only; an older server never opens it, so nothing is sent. */
export const UPLOAD_CHANNEL_ID = 3;
/** How long an ICE 'disconnected' may last before the connection counts as dropped. */
export const DISCONNECT_GRACE_MS = 3000;

export function randomUfrag(): string {
  const chars = 'abcdefghijklmnopqrstuvwxyz0123456789';
  return Array.from(crypto.getRandomValues(new Uint8Array(32)), (b) => chars[b % chars.length]).join('');
}

/** Gives the offer our own ICE credentials: one random string for both ufrag and pwd (at least 22 chars). */
export function setIceCredentials(sdp: string, ufrag: string): string {
  return sdp.replace(/a=ice-ufrag:.*\r\n/, `a=ice-ufrag:${ufrag}\r\n`).replace(/a=ice-pwd:.*\r\n/, `a=ice-pwd:${ufrag}\r\n`);
}

/** The answer a signaling server would have sent, built from the record instead. */
export function buildAnswer(r: RtcTarget, mid: string): string {
  const v6 = r.ip.includes(':');
  return [
    'v=0',
    `o=- 0 0 IN ${v6 ? 'IP6' : 'IP4'} ${r.ip}`,
    's=-',
    't=0 0',
    'a=ice-lite',
    `a=group:BUNDLE ${mid}`,
    `m=application ${r.udp_port} UDP/DTLS/SCTP webrtc-datachannel`,
    `c=IN ${v6 ? 'IP6' : 'IP4'} ${r.ip}`,
    `a=mid:${mid}`,
    `a=ice-ufrag:${SERVER_UFRAG}`,
    `a=ice-pwd:${SERVER_PWD}`,
    `a=fingerprint:${r.fingerprint}`,
    'a=setup:passive',
    'a=sctp-port:5000',
    'a=max-message-size:65536',
    `a=candidate:1 1 UDP 2113929471 ${r.ip} ${r.udp_port} typ host`,
    'a=end-of-candidates',
    '',
  ].join('\r\n');
}

export class RtcTransport implements Transport {
  readonly kind = 'rtc' as const;
  onmessage: ((text: string) => void) | null = null;
  onclose: (() => void) | null = null;
  readonly lanes = true;
  onterm: ((bytes: Uint8Array) => void) | null = null;
  private closed = false;
  private dropTimer: ReturnType<typeof setTimeout> | null = null;

  constructor(private pc: RTCPeerConnection, private dc: RTCDataChannel, private term?: RTCDataChannel, private ctl?: RTCDataChannel, private upload?: RTCDataChannel) {
    dc.onmessage = (e) => typeof e.data === 'string' && this.onmessage?.(e.data);
    if (ctl) ctl.onmessage = (e) => typeof e.data === 'string' && this.onmessage?.(e.data);
    if (term) {
      term.binaryType = 'arraybuffer';
      term.onmessage = (e) => e.data instanceof ArrayBuffer && this.onterm?.(new Uint8Array(e.data));
    }
    dc.onclose = () => this.finish();
    pc.oniceconnectionstatechange = () => {
      const state = pc.iceConnectionState;
      if (state === 'failed' || state === 'closed') this.finish();
      // Browsers report 'disconnected' about 5 s after the peer stops answering consent checks and only
      // 'failed' after about 30 s. Give a brief network switch a moment to recover, then treat it as a drop.
      if (this.dropTimer) clearTimeout(this.dropTimer);
      this.dropTimer = state === 'disconnected' ? setTimeout(() => this.finish(), DISCONNECT_GRACE_MS) : null;
    };
  }

  send(text: string) {
    if (this.dc.readyState === 'open') this.dc.send(text);
  }

  sendControl(text: string): boolean {
    if (this.ctl?.readyState !== 'open') return false;
    this.ctl.send(text);
    return true;
  }

  sendUpload(frame: Uint8Array): boolean {
    if (this.upload?.readyState !== 'open') return false;
    this.upload.send(new Uint8Array(frame));
    return true;
  }

  sendTerm(bytes: Uint8Array): boolean {
    if (this.term?.readyState !== 'open') return false;
    this.term.send(new Uint8Array(bytes));
    return true;
  }

  close() {
    this.closed = true;
    this.pc.close();
  }

  private finish() {
    if (this.closed) return;
    this.closed = true;
    if (this.dropTimer) clearTimeout(this.dropTimer);
    this.pc.close();
    this.onclose?.();
  }
}

/** What a WebRTC-direct attempt needs: an address and the pinned fingerprint. A directory record has all three. */
export type RtcTarget = Pick<DirRecord, 'ip' | 'udp_port' | 'fingerprint'>;

/** Resolves when the data channel is open; rejects on timeout, ICE/DTLS failure or `signal` (another path won). */
export async function openRtc(record: RtcTarget, timeoutMs: number, signal?: AbortSignal): Promise<Transport> {
  const pc = new RTCPeerConnection({ iceServers: [] });
  const dc = pc.createDataChannel('t', { negotiated: true, id: CHANNEL_ID });
  const term = pc.createDataChannel('term', { negotiated: true, id: TERM_CHANNEL_ID });
  const ctl = pc.createDataChannel('ctl', { negotiated: true, id: CONTROL_CHANNEL_ID });
  const upload = pc.createDataChannel('upload', { negotiated: true, id: UPLOAD_CHANNEL_ID });
  try {
    const offer = await pc.createOffer();
    const sdp = setIceCredentials(offer.sdp!, randomUfrag());
    await pc.setLocalDescription({ type: 'offer', sdp });
    const mid = /a=mid:(\S+)/.exec(sdp)?.[1] ?? '0';
    await pc.setRemoteDescription({ type: 'answer', sdp: buildAnswer(record, mid) });
    await new Promise<void>((resolve, reject) => {
      const timer = setTimeout(() => reject(new Error(`no data channel after ${timeoutMs} ms`)), timeoutMs);
      signal?.addEventListener('abort', () => { clearTimeout(timer); reject(new Error('cancelled')); }, { once: true });
      dc.onopen = () => {
        clearTimeout(timer);
        resolve();
      };
      pc.oniceconnectionstatechange = () => {
        if (pc.iceConnectionState === 'failed') {
          clearTimeout(timer);
          reject(new Error('ICE failed'));
        }
      };
    });
    return new RtcTransport(pc, dc, term, ctl, upload);
  } catch (e) {
    pc.close();
    throw e;
  }
}
