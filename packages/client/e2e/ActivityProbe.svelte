<script lang="ts">
  // The real Activity view over one fake server that answers like a history store: from the shared fixture, for the range
  // asked. `?silent` is a server older than the feature, which never answers. window.__activity lists the ranges asked.
  import ActivityView from '../src/shell/ActivityView.svelte';
  import { answered, asked, type HistoryMap, type Range } from '../src/lib/history';
  import { EARLIER, RUNS } from '../src/lib/historyFixture';
  import type { HubServer } from '../src/lib/hub';

  const silent = new URLSearchParams(location.search).has('silent');
  const servers = [{ id: 'alpha', state: 'connected', server: { server_name: 'alpha' } }] as unknown as HubServer[];
  let history: HistoryMap = $state({});
  let now = $state(Date.now());
  const asks: Range[] = [];
  function onask(r: Range) {
    asks.push(r);
    history = asked(history, 'alpha', r, Date.now());
    if (silent) return;
    const runs = [...RUNS, ...EARLIER].filter((x) => (x.ended_at === undefined ? x.started_at < r.to : x.ended_at >= r.from && x.ended_at < r.to));
    history = answered(history, 'alpha', { type: 'history', ...r, runs, truncated: false, keep_days: 90, max_bytes: 20_000_000 });
  }
  Object.assign(window, { __activity: { asks, tick: () => (now = Date.now()) } });
</script>

<div style="display:flex;flex-direction:column;height:100dvh">
  <ActivityView {servers} {history} {now} {onask} />
</div>
