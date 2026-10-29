// Screens peeked at before a tab switch (decision 0084). Kept in memory only, never stored or reported: a screen
// can hold anything the user typed.
export const SNAPSHOT_CAP = 20;

/** A workspace without a tab keeps its old key; a tab gets its own preview (decision 0147). */
const withTab = (key: string, tab?: string | null) => (tab ? `${key}/${tab}` : key);

export class SnapshotStore {
  private map = new Map<string, { text: string; hash: string }>();

  /** The newest use of a key goes last, so the oldest one is the one dropped at the cap. */
  put(key: string, text: string, hash: string) {
    this.map.delete(key);
    this.map.set(key, { text, hash });
    while (this.map.size > SNAPSHOT_CAP) this.map.delete(this.map.keys().next().value as string);
  }
  /** The hash we hold, so the server can answer "unchanged" instead of sending the screen again. */
  hash(key: string, tab?: string | null): string | undefined { return this.map.get(withTab(key, tab))?.hash; }
  text(key: string, tab?: string | null): string | null { return this.map.get(withTab(key, tab))?.text ?? null; }
  /** An empty snapshot means the manager cannot give one: the plain attach is used. */
  apply(key: string, msg: { text: string; hash: string; unchanged?: boolean }, tab?: string | null) {
    const at = withTab(key, tab);
    if (msg.unchanged) { const old = this.map.get(at); if (old) this.put(at, old.text, old.hash); return; }
    if (msg.text) this.put(at, msg.text, msg.hash); else this.map.delete(at);
  }
  forget(key: string, tab?: string | null) { this.map.delete(withTab(key, tab)); }
  get size() { return this.map.size; }
}
