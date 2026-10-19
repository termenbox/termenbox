<script lang="ts">
  import { onMount, untrack } from 'svelte';
  import { AgentBadge, ChartFrame, EmptyState, Segmented, StatusChip } from '../lib/ds';
  import type { FleetState } from '../lib/ds/states';
  import type { HubServer } from '../lib/hub';
  import BarChart from './BarChart.svelte';
  import {
    bucketsOf, clock, compact, digestOf, digestSentence, duration, groupByDay, dayLabel, overall, rangeLabel, rangeOf, rowsOf, startOfDay, statusOf, step, taskBars, waitBars,
    type HistoryMap, type Mode, type Outcome, type Range,
  } from '../lib/history';

  /** The Activity destination (U12, decision 0105): what the agents of the connected servers did, as a digest, two charts and
   *  a timeline per day. Each server keeps its own history; this view only asks for a range and lays the answers out. */
  let { servers, history, now, onask }: { servers: HubServer[]; history: HistoryMap; now: number; onask: (range: Range) => void } = $props();

  let mode = $state<Mode>('day');
  let anchor = $state(startOfDay(Date.now()));
  const range = $derived(rangeOf(mode, anchor));
  const connected = $derived(servers.filter((s) => s.state === 'connected'));
  const nameOf = (s: HubServer) => s.server.server_name || 'Server';
  // An answer for another range than the one on screen is not an answer yet.
  const entry = (s: HubServer) => (history[s.id]?.from === range.from && history[s.id]?.to === range.to ? history[s.id] : undefined);
  const states = $derived(connected.map((s) => ({ server: s, status: statusOf(entry(s), now) })));
  const view = $derived(overall(states.map((s) => s.status)));
  const missing = $derived(states.filter((s) => s.status === 'unavailable').map((s) => nameOf(s.server)));
  const answers = $derived(states.flatMap((s) => { const a = entry(s.server)?.answer; return a ? [{ server: s.server, answer: a }] : []; }));
  const rows = $derived(rowsOf(answers.map(({ server, answer }) => ({ serverId: server.id, serverName: nameOf(server), runs: answer.runs })), now));
  const digest = $derived(digestOf(rows));
  const buckets = $derived(bucketsOf(rows, mode, range));
  const groups = $derived(groupByDay(rows, range));
  const cut = $derived(answers.filter((a) => a.answer.truncated));
  const keepDays = $derived(Math.max(0, ...answers.map((a) => a.answer.keep_days)));
  const many = $derived(connected.length > 1);
  const isToday = $derived(anchor >= startOfDay(now));

  const ask = () => untrack(() => onask(range));
  // Ask again when the range or the set of connected servers changes, and every 30 s while the view is open. What the
  // asker reads is not a dependency: it writes the answers this view shows.
  $effect(() => { range; connected.map((s) => s.id).join(); ask(); });
  onMount(() => { const t = setInterval(ask, 30_000); return () => clearInterval(t); });

  const noun = $derived(mode === 'day' ? 'day' : 'week');
  const FLEET: Record<Outcome, FleetState> = { finished: 'finished', failed: 'failed', stopped: 'idle', running: 'working' };
  const WORD: Record<Outcome, string> = { finished: 'Finished', failed: 'Failed', stopped: 'Stopped', running: 'Running' };
  const when = (r: (typeof rows)[number]) => (r.endedAt === undefined ? `since ${clock(r.startedAt)}` : `${clock(r.startedAt)} to ${clock(r.endedAt)}`);
  const btn = 'flex min-h-11 min-w-11 items-center justify-center rounded-card bg-raised text-base font-semibold text-fg disabled:opacity-40';
</script>

<div class="flex min-h-0 flex-1 flex-col overflow-y-auto p-4" data-activity={connected.length ? view : 'none'}>
<div class="mx-auto flex w-full max-w-3xl flex-col gap-4">
  <h1 class="text-title font-bold">Activity</h1>

  {#if connected.length === 0}
    <EmptyState title="No server connected" body="Activity is kept on each of your servers. Connect one to see what its agents did." />
  {:else}
    <Segmented label="Range" value={mode} onchange={(v) => (mode = v as Mode)} options={[{ value: 'day', label: 'Day' }, { value: 'week', label: 'Week' }]} />
    <div class="grid grid-cols-[44px_1fr_44px] items-center gap-2">
      <button type="button" class={btn} aria-label={`Previous ${noun}`} onclick={() => (anchor = step(mode, anchor, -1, now))}><span aria-hidden="true">‹</span></button>
      <p class="text-center text-base font-semibold" aria-live="polite" data-activity-range>{rangeLabel(mode, range, now)}</p>
      <button type="button" class={btn} aria-label={`Next ${noun}`} disabled={isToday} onclick={() => (anchor = step(mode, anchor, 1, now))}><span aria-hidden="true">›</span></button>
    </div>

    {#if view === 'loading'}
      <p class="py-8 text-center text-base text-soft" role="status">Loading activity…</p>
    {:else if view === 'unavailable'}
      <EmptyState title="Activity is not available" body={`${missing.join(', ')} did not answer. A server older than this feature keeps no history: update it, then try again.`}>
        <button type="button" onclick={ask} class="min-h-11 rounded-card bg-accent px-4 text-base font-semibold text-on-accent">Try again</button>
      </EmptyState>
    {:else}
      {#if missing.length}
        <p class="rounded-card border border-line bg-panel p-3 text-sm text-soft" role="status">{missing.join(', ')} did not answer, so {missing.length === 1 ? 'its' : 'their'} activity is not included.
          <button type="button" onclick={ask} class="min-h-11 font-semibold text-accent-text underline">Try again</button></p>
      {/if}

      <section aria-labelledby="activity-digest" class="flex flex-col gap-2" data-activity-digest>
        <h2 id="activity-digest" class="text-sm font-semibold text-soft">{mode === 'day' ? 'Daily digest' : 'Weekly digest'}</h2>
        <div class="grid grid-cols-3 gap-2">
          <div class="flex h-19 min-w-0 flex-col items-center justify-center gap-1 rounded-card border border-line bg-panel"><b class="text-title leading-none font-bold text-ok" data-digest="finished">{digest.finished}</b><span class="text-xs font-semibold text-soft"><span aria-hidden="true" class="text-ok">✓</span> Finished</span></div>
          <div class="flex h-19 min-w-0 flex-col items-center justify-center gap-1 rounded-card border border-line bg-panel"><b class="text-title leading-none font-bold text-bad" data-digest="failed">{digest.failed}</b><span class="text-xs font-semibold text-soft"><span aria-hidden="true" class="text-bad">✕</span> Failed</span></div>
          <div class="flex h-19 min-w-0 flex-col items-center justify-center gap-1 rounded-card border border-line bg-panel"><b class="text-title leading-none font-bold text-warn" data-digest="waited">{compact(digest.blockedMs)}</b><span class="text-xs font-semibold text-soft"><span aria-hidden="true" class="text-warn">!</span> Waited on you</span></div>
        </div>
        {#if digest.stopped || digest.running}
          <p class="text-sm text-soft">{[digest.stopped ? `${digest.stopped} stopped` : '', digest.running ? `${digest.running} still running` : ''].filter(Boolean).join(' · ')}</p>
        {/if}
        <p class="sr-only" role="status">{digestSentence(digest)}</p>
      </section>

      {#if rows.length === 0}
        <EmptyState title={mode === 'day' ? 'Nothing happened this day' : 'Nothing happened this week'} body="A task shows up here once an agent reports it. For Claude Code, turn on hooks in Settings; without them the server cannot tell when a task starts or ends." />
      {:else}
        <ChartFrame title={mode === 'day' ? 'Tasks by hour' : 'Tasks by day'} caption={`${digest.finished} finished, ${digest.failed} failed.`}>
          {#snippet legend()}<span aria-hidden="true"><span class="text-ok">■</span> finished <span class="text-bad">▨</span> failed</span>{/snippet}
          <BarChart label={mode === 'day' ? 'Tasks by hour' : 'Tasks by day'} bars={taskBars(buckets, mode)} />
        </ChartFrame>
        <ChartFrame title="Waiting on you" caption={`Agents waited ${duration(digest.blockedMs)} on you in all.`}>
          <BarChart label="Time agents waited on you" bars={waitBars(buckets, mode)} />
        </ChartFrame>

        <section aria-labelledby="activity-timeline" class="flex flex-col gap-3" data-activity-timeline>
          <h2 id="activity-timeline" class="text-sm font-semibold text-soft">Timeline</h2>
          {#each groups as g (g.day)}
            <div class="flex flex-col gap-2">
              <h3 class="text-sm font-semibold text-fg">{dayLabel(g.day, now)}</h3>
              <ol class="flex flex-col gap-2">
                {#each g.rows as r, i (`${r.serverId}|${r.session}|${r.startedAt}|${i}`)}
                  <li class="flex gap-3 rounded-card border border-line bg-panel p-3" data-outcome={r.outcome}>
                    <AgentBadge agent={r.agent} state={FLEET[r.outcome]} class="self-start" />
                    <div class="flex min-w-0 flex-1 flex-col gap-1">
                      <b class="text-base font-semibold break-words">{r.task || r.session}</b>
                      <p class="text-sm break-words text-soft">{[r.task ? r.session : '', r.agent ?? '', many ? r.serverName : ''].filter(Boolean).join(' · ')}</p>
                      <p class="flex flex-wrap items-center gap-x-2 gap-y-1 text-sm text-soft">
                        <StatusChip state={FLEET[r.outcome]} label={WORD[r.outcome]} />
                        <span>{when(r)}</span><span>{duration(r.durationMs)}</span>
                        {#if r.blockedMs}<span>waited {duration(r.blockedMs)}</span>{/if}
                      </p>
                    </div>
                  </li>
                {/each}
              </ol>
            </div>
          {/each}
        </section>
      {/if}

      {#each cut as c (c.server.id)}
        <p class="text-sm text-soft">{nameOf(c.server)} had more tasks in this {noun} than fit in one answer: showing the newest {c.answer.runs.length}.</p>
      {/each}
      <p class="text-sm text-soft">History stays on your servers, for up to {keepDays} days. It holds titles, times and outcomes, never prompts or terminal text. Delete it under Privacy and your data in Settings.</p>
    {/if}
  {/if}
</div>
</div>
