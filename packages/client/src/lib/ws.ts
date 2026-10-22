import { wsUrl } from './servers';
import type { Transport } from './transport';
import { LANE_TERM } from './protocol';

class WsTransport implements Transport {
  readonly kind = 'ws' as const;
  onmessage: ((text: string) => void) | null = null;
  onclose: (() => void) | null = null;
  readonly lanes = true;
  onterm: ((bytes: Uint8Array) => void) | null = null;
  private closed = false;

  constructor(private ws: WebSocket) {
    ws.binaryType = 'arraybuffer';
    ws.onmessage = (e) => {
      if (typeof e.data === 'string') this.onmessage?.(e.data);
      else if (e.data instanceof ArrayBuffer) {
        // Binary frames start with a lane tag; a tag this client does not know is ignored.
        const frame = new Uint8Array(e.data);
        if (frame[0] === LANE_TERM) this.onterm?.(frame.subarray(1));
      }
    };
    ws.onclose = () => this.finish();
    ws.onerror = () => ws.close();
  }

  send(text: string) {
    if (this.ws.readyState === WebSocket.OPEN) this.ws.send(text);
  }

  sendTerm(bytes: Uint8Array): boolean {
    if (this.ws.readyState !== WebSocket.OPEN) return false;
    const frame = new Uint8Array(bytes.length + 1);
    frame[0] = LANE_TERM;
    frame.set(bytes, 1);
    this.ws.send(frame);
    return true;
  }

  close() {
    this.closed = true;
    this.ws.close();
  }

  private finish() {
    if (this.closed) return;
    this.closed = true;
    this.onclose?.();
  }
}

export function openWs(url: string, timeoutMs: number, signal?: AbortSignal): Promise<Transport> {
  return new Promise((resolve, reject) => {
    let ws: WebSocket;
    try {
      ws = new WebSocket(wsUrl(url));
    } catch {
      reject(new Error('invalid WebSocket URL'));
      return;
    }
    const onAbort = () => {
      clearTimeout(timer);
      ws.onopen = ws.onerror = null;
      ws.close();
      reject(new Error('cancelled'));
    };
    // Whichever way this attempt ends, the race's signal must not reach it any more: a winner would otherwise be
    // closed by the abort that declares it the winner.
    const settle = () => {
      clearTimeout(timer);
      signal?.removeEventListener('abort', onAbort);
    };
    const timer = setTimeout(() => {
      settle();
      ws.close();
      reject(new Error(`no WebSocket after ${timeoutMs} ms`));
    }, timeoutMs);
    // Another path won the race: stop this one.
    signal?.addEventListener('abort', onAbort, { once: true });
    ws.onopen = () => {
      settle();
      resolve(new WsTransport(ws));
    };
    ws.onerror = () => {
      settle();
      reject(new Error('WebSocket failed'));
    };
  });
}
