/** Time the owner spends looking at a question in the tab that asked it (plan step 1.1b). Home reports the time a card
 *  is shown there; this reports the rest, the open terminal or Agent view. Metadata only: an id and milliseconds. */
export type AskTarget = { serverId: string; id: string };

export class AskDwell {
  private cur: AskTarget | null = null;
  private since = 0;

  constructor(private readonly report: (serverId: string, id: string, ms: number) => void) {}

  /** `target` is the question in view right now (page visible, its tab open), or null. Call when it may have changed. */
  watch(target: AskTarget | null, now: number): void {
    if (target && this.cur && target.id === this.cur.id && target.serverId === this.cur.serverId) return;
    if (this.cur) {
      const ms = now - this.since;
      if (ms >= 500) this.report(this.cur.serverId, this.cur.id, Math.round(ms));
    }
    this.cur = target;
    this.since = now;
  }
}
