// Everything the terminal sends to the server goes through here. xterm answers a palette query (tmux asks
// for all 256 colours right after an attach) with one reply per colour; sent one by one that is a burst of hundreds
// of messages. Replies that arrive in the same moment travel as one message instead, which the server reads as the
// same bytes in the same order.
// A data-channel message may not exceed 64 KiB (the SDP says so and the server drops the connection on more). A UTF-16
// unit is at most 3 bytes of UTF-8, so 16k units stay under 48 KiB before the transport adds anything.
export const MAX_CHUNK_CHARS = 16 * 1024;

export class InputBatcher {
  private pending = '';
  private timer: unknown = null;

  constructor(
    private send: (data: string) => void,
    private setTimer: (fn: () => void, ms: number) => unknown = (fn, ms) => setTimeout(fn, ms),
    private clearTimer: (id: unknown) => void = (id) => clearTimeout(id as ReturnType<typeof setTimeout>),
    private windowMs = 4,
  ) {}

  /** Queues `data`; it goes out within `windowMs`, together with anything queued meanwhile. */
  add(data: string): void {
    this.pending += data;
    if (this.timer === null) this.timer = this.setTimer(() => this.flush(), this.windowMs);
  }

  /** Sends what is queued now (before a resize or a detach, so the order with other messages holds). */
  flush(): void {
    if (this.timer !== null) this.clearTimer(this.timer);
    this.timer = null;
    if (!this.pending) return;
    const data = this.pending;
    this.pending = '';
    for (let at = 0; at < data.length; ) {
      let end = Math.min(at + MAX_CHUNK_CHARS, data.length);
      // Never cut between the halves of a surrogate pair.
      if (end < data.length && data.charCodeAt(end - 1) >= 0xd800 && data.charCodeAt(end - 1) <= 0xdbff) end--;
      this.send(data.slice(at, end));
      at = end;
    }
  }
}
