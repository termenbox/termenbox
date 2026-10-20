<script lang="ts">
  // The real Activity view over one fake server that answers like a history store: from the shared fixture, for the range
  // asked. `?silent` is a server older than the feature, which never answers. window.__activity lists the ranges asked.
  import ActivityView from '../src/shell/ActivityView.svelte';
  import { answered, asked, type HistoryMap, type Range } from '../src/lib/history';
  import { at, EARLIER, REPORTED, RUNS } from '../src/lib/historyFixture';
  import type { HubServer } from '../src/lib/hub';

  const silent = new URLSearchParams(location.search).has('silent');
  const dense = new URLSearchParams(location.search).has('dense');
  // `?reported`: an ACP agent that reported its own cost joins the day, and one idle Claude Code session has a warm cache.
  const reported = new URLSearchParams(location.search).has('reported');
  let servers = $state([{ id: 'alpha', state: 'connected', server: { server_name: 'alpha' } }] as unknown as HubServer[]);
  let history: HistoryMap = $state({});
  let now = $state(Date.now());
  const asks: Range[] = [];
  // `?dense`: a busy morning, one task every hour from 08:00 to 13:00, each waiting over an hour, so neighbouring bars carry long labels.
  const busy = dense ? Array.from({ length: 6 }, (_, i) => ({ session: `s${i}`, started_at: at(14, 8 + i, 0), ended_at: at(14, 8 + i, 50), outcome: 'finished' as const, blocked_ms: (65 + i) * 60_000 })) : [];
  function onask(r: Range) {
    asks.push(r);
    history = asked(history, 'alpha', r, Date.now());
    if (silent) return;
    const runs = [...RUNS, ...EARLIER, ...busy, ...(reported ? [REPORTED] : [])].filter((x) => (x.ended_at === undefined ? x.started_at < r.to : x.ended_at >= r.from && x.ended_at < r.to));
    history = answered(history, 'alpha', { type: 'history', ...r, runs, truncated: false, keep_days: 90, max_bytes: 20_000_000 });
  }
  Object.assign(window, { __activity: { asks, tick: () => (now = Date.now()), rebuild: () => (servers = servers.map((s) => ({ ...s }))) } });
</script>

<div style="display:flex;flex-direction:column;height:100dvh">
  <ActivityView {servers} {history} {now} {onask} caches={reported ? [{ key: 'alpha/api', name: 'api', server: 'alpha', remainingMs: 150_000 }] : []} />
</div>
