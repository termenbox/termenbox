<script lang="ts">
  // The real AcpView over a live list of 40 entries and a scripted "server" that answers `window` from a fixture (R78 step 9).
  // window.__acpHistory: `land(turn, query)` is a tap on a hit; `push(n)` delivers a window that nobody asked for (an
  // answer arriving late); `asks` lists the turns asked.
  import AcpView from '../src/AcpView.svelte';
  import { emptyStream, type Entry, type StreamState } from '../src/lib/acpStream';
  import { withWindow, type WindowMap } from '../src/lib/acpHistory';
  import type { SessionWindowMessage } from '../src/lib/protocol';

  const live: Entry[] = Array.from({ length: 40 }, (_, i) => i % 2 === 0
    ? { id: 100 + i, rev: 1, kind: 'user', text: `live question ${i}` }
    : { id: 100 + i, rev: 1, kind: 'message', text: `live answer ${i}. ${'It goes on for a while so that the list is taller than the screen. '.repeat(3)}`, truncated: false });
  const stream: StreamState = { ...emptyStream('w'), epoch: 1, rev: 1, entries: live, info: { status: 'running', agent: 'claude', dir: '/w', working: false, modes: [], commands: [] } };

  const filed = (n: number, anchor: number): SessionWindowMessage => ({
    type: 'session_window', workspace: 'w', tab: '', turn: `h${anchor}`, anchor, fingerprint: 'f1',
    entries: Array.from({ length: n }, (_, i) => ({ turn: `h${i}`, kind: i % 2 ? 'message' : 'user', text: i === anchor ? `the needle is here in turn ${i}` : `older turn ${i} ${'padding '.repeat(20)}` })),
  });
  // The tail of this window is the first live entry: the live list wins, so it stops before it.
  const overlapping = (): SessionWindowMessage => {
    const w = filed(6, 2);
    w.entries.push({ turn: 'joined', kind: 'user', text: 'live question 0' });
    w.turn = 'joined'; w.anchor = 6;
    return w;
  };

  let windows: WindowMap = $state({});
  let focus = $state<{ turn: string; query: string; nonce: number } | null>(null);
  let nonce = 0;
  const asks: string[] = [];
  const api = {
    asks,
    land(turn: string, query: string) { focus = { turn, query, nonce: ++nonce }; },
    push(n: number) { windows = withWindow(windows, 'w', filed(n, 1)); },
    held() { return Object.keys(windows).length; },
  };
  (window as unknown as { __acpHistory: typeof api }).__acpHistory = api;

  function answer(turn: string) {
    asks.push(turn);
    setTimeout(() => { windows = withWindow(windows, 'w', turn === 'joined' ? overlapping() : filed(12, Number(turn.slice(1)) || 5)); }, 30);
  }
</script>

<div style="height: 100dvh; display: flex; flex-direction: column;">
  <AcpView workspaceKey="e2e/w" name="work" agent="claude" {stream} window={windows['w']} {focus} onwindow={answer}
    onprompt={() => {}} oninterrupt={() => {}} onanswer={() => {}} onmode={() => {}} onshell={() => {}} />
</div>
