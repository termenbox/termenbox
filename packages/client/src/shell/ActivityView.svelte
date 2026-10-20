<script lang="ts">
  import { onMount, untrack } from 'svelte';
  import { AgentBadge, ChartFrame, EmptyState, Segmented, StatusChip } from '../lib/ds';
  import type { FleetState } from '../lib/ds/states';
  import type { HubServer } from '../lib/hub';
  import BarChart from './BarChart.svelte';
  import {
    bucketsOf, clock, compact, costsText, costText, digestOf, digestSentence, duration, exact, groupByDay, dayLabel, overall, rangeLabel, rangeOf, rowsOf, startOfDay, statusOf, step, taskBars, tokenBars, tokenText, tokensOf, usageByAgent, usageByProject, waitBars, whyMissing, WINDOW_NOTE,
    type HistoryMap, type Mode, type Outcome, type Range, type UsageTotals,
  } from '../lib/history';
  import { cacheText } from '../lib/cache';
  import HealthSection from './HealthSection.svelte';
  import type { HealthMap } from '../lib/health';

  /** The Activity destination (U12, decision 0105): what the agents of the connected servers did, as a digest, two charts and
   *  a timeline per day. Each server keeps its own history; this view only asks for a range and lays the answers out. */
  let { servers, history, now, onask, caches = [], ttlMs = 300_000, health = {}, onhealth = () => {}, rttOf = () => null }: {
    servers: HubServer[]; history: HistoryMap; now: number; onask: (range: Range) => void;
    /** What each server said about its health (U14), the request for it, and this device's round trip to a server. */
    health?: HealthMap; onhealth?: (restart?: boolean) => void; rttOf?: (id: string) => number | null;
    /** Claude Code sessions that are idle with a warm prompt cache right now, with the estimated time left (decision 0083). */
    caches?: { key: string; name: string; server: string; remainingMs: number }[]; ttlMs?: number;
  } = $props();

  let mode = $state<Mode>('day');
  let anchor = $state(startOfDay(Date.now()));
  const range = $derived(rangeOf(mode, anchor));
  const connected = $derived(servers.filter((s) => s.state === 'connected'));
  // The hub publishes a new servers array on every agent event; only a change in who is connected is a reason to ask again.
  const connectedIds = $derived(connected.map((s) => s.id).join());
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
  const byAgent = $derived(usageByAgent(rows));
  const byProject = $derived(usageByProject(rows));
  const anyTokens = $derived(byAgent.some((a) => tokensOf(a.totals) !== undefined));
  const tokenTotal = $derived(byAgent.reduce((t, a) => t + (tokensOf(a.totals) ?? 0), 0));
  const cut = $derived(answers.filter((a) => a.answer.truncated));
  const keepDays = $derived(Math.max(0, ...answers.map((a) => a.answer.keep_days)));
  const many = $derived(connected.length > 1);
  const isToday = $derived(anchor >= startOfDay(now));

  const ask = () => untrack(() => onask(range));
  // Health is its own feed: it is asked for when the servers change and on its own timer, not whenever the range moves.
  const askHealth = () => untrack(() => onhealth());
  // Ask again when the range or the set of connected servers changes, and every 30 s while the view is open. What the
  // asker reads is not a dependency: it writes the answers this view shows.
  $effect(() => { range; connectedIds; ask(); });
  $effect(() => { connectedIds; askHealth(); });
  onMount(() => { const t = setInterval(ask, 30_000); const h = setInterval(askHealth, 30_000); return () => { clearInterval(t); clearInterval(h); }; });

  const noun = $derived(mode === 'day' ? 'day' : 'week');
  const FLEET: Record<Outcome, FleetState> = { finished: 'finished', failed: 'failed', stopped: 'idle', running: 'working' };
  const WORD: Record<Outcome, string> = { finished: 'Finished', failed: 'Failed', stopped: 'Stopped', running: 'Running' };
  const when = (r: (typeof rows)[number]) => (r.endedAt === undefined ? `since ${clock(r.startedAt)}` : `${clock(r.startedAt)} to ${clock(r.endedAt)}`);
  /** "In 12k, out 3.4k, cache read 90k, cache written 20k": only the figures the agent reported. */
  const tokenLine = (t: UsageTotals) => [['in', t.input], ['out', t.output], ['cache read', t.cacheRead], ['cache written', t.cacheWrite]]
    .flatMap(([k, n]) => (typeof n === 'number' ? [`${k} ${tokenText(n)}`] : [])).join(' · ');
  const tokenSpoken = (t: UsageTotals) => [['input', t.input], ['output', t.output], ['cache read', t.cacheRead], ['cache written', t.cacheWrite]]
    .flatMap(([k, n]) => (typeof n === 'number' ? [`${k} ${exact(n)} tokens`] : [])).join(', ');
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

        <section aria-labelledby="activity-usage" class="flex flex-col gap-3" data-activity-usage>
          <h2 id="activity-usage" class="text-sm font-semibold text-soft">Usage and cost</h2>
          <p class="text-sm text-soft">What each agent reported, counted per task. Termenbox works out no prices: a cost appears only when the agent reported one, in its own currency. Claude Code counts are a lower bound: subagents, very long messages and anything before this server first saw the session are not counted.</p>
          {#if anyTokens}
            <ChartFrame title={mode === 'day' ? 'Tokens by hour' : 'Tokens by day'} caption={`${exact(tokenTotal)} input and output tokens in all, from the agents that report them.`}>
              <BarChart label={mode === 'day' ? 'Tokens by hour' : 'Tokens by day'} bars={tokenBars(buckets, mode)} />
            </ChartFrame>
          {/if}
          <ul class="flex flex-col gap-2" aria-label="Usage per agent">
            {#each byAgent as a (a.agent)}
              {@const t = a.totals}
              <li class="flex flex-col gap-1 rounded-card border border-line bg-panel p-3" data-usage-agent={a.agent}>
                <b class="text-base font-semibold break-words">{a.agent}</b>
                <p class="text-sm text-soft">{t.tasks === 1 ? '1 task' : `${t.tasks} tasks`}{t.withTokens < t.tasks && t.withTokens > 0 ? `, ${t.withTokens} with token counts` : ''}</p>
                {#if tokensOf(t) !== undefined}
                  <p class="text-base" data-usage-tokens><span class="font-semibold">{tokenText(tokensOf(t) ?? 0)} tokens</span> <span class="text-soft">({tokenLine(t)})</span><span class="sr-only">: {tokenSpoken(t)}</span></p>
                {:else}
                  <p class="text-sm text-soft" data-usage-missing="tokens">Tokens: not available from this agent. {whyMissing(a.agent, 'tokens')}</p>
                {/if}
                {#if t.cost}
                  <p class="text-base" data-usage-cost><span class="font-semibold">{costsText(t.cost)}</span> <span class="text-soft">reported by {a.agent}{t.withCost < t.tasks ? ` (for ${t.withCost} of ${t.tasks} tasks)` : ''}</span></p>
                {:else}
                  <p class="text-sm text-soft" data-usage-missing="cost">Cost: not available from this agent. {whyMissing(a.agent, 'cost')}</p>
                {/if}
                {#if t.rateLimited}
                  <p class="text-sm text-warn" data-usage-limited><span aria-hidden="true">!</span> Rate limited {t.rateLimited === 1 ? 'once' : `${t.rateLimited} times`}</p>
                {/if}
              </li>
            {/each}
          </ul>
          {#if byProject.some((p) => tokensOf(p.totals) !== undefined || p.totals.cost)}
            <div class="flex flex-col gap-2" data-usage-projects>
              <h3 class="text-sm font-semibold text-fg">By project</h3>
              <ul class="flex flex-col divide-y divide-line rounded-card border border-line bg-panel" aria-label="Usage per project">
                {#each byProject as p (p.project)}
                  <li class="flex items-baseline justify-between gap-3 p-3" data-usage-project={p.project}>
                    <span class="min-w-0 text-base break-words">{p.project}</span>
                    <span class="shrink-0 text-right text-sm text-soft">
                      {#if tokensOf(p.totals) !== undefined}<span class="font-semibold text-fg">{tokenText(tokensOf(p.totals) ?? 0)} tokens</span><span class="sr-only"> ({exact(tokensOf(p.totals) ?? 0)})</span>{:else}no counts{/if}
                      {#if p.totals.cost} · {costsText(p.totals.cost)}{/if}
                    </span>
                  </li>
                {/each}
              </ul>
            </div>
          {/if}
          <p class="text-sm text-soft">{WINDOW_NOTE} Hits are counted above where an agent said it was rate limited. For Claude Code, the status-line forwarder (roadmap R19, not built yet) would add more.</p>
          {#if caches.length}
            <div class="flex flex-col gap-2" data-usage-caches>
              <h3 class="text-sm font-semibold text-fg">Prompt cache now</h3>
              <ul class="flex flex-col divide-y divide-line rounded-card border border-line bg-panel" aria-label="Prompt cache of idle Claude Code sessions">
                {#each caches as c (c.key)}
                  <li class="flex items-baseline justify-between gap-3 p-3"><span class="min-w-0 text-base break-words">{c.name}{many ? ` · ${c.server}` : ''}</span><span class="shrink-0 text-sm text-soft" data-cache>{cacheText(c.remainingMs)}</span></li>
                {/each}
              </ul>
              <p class="text-sm text-soft">An estimate: the {Math.round(ttlMs / 60_000)}-minute cache time (Settings) counted from the agent's last answer.</p>
            </div>
          {/if}
        </section>

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
                        {#if tokensOf(r.usage ?? {}) !== undefined}<span>{tokenText(tokensOf(r.usage ?? {}) ?? 0)} tokens</span>{/if}
                        {#if r.usage?.costMicros !== undefined && r.usage.currency}<span>{costText(r.usage.costMicros, r.usage.currency)}</span>{/if}
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
      <p class="text-sm text-soft">History stays on your servers, for up to {keepDays} days. It holds session names, a short task title (the first line of your prompt, scrubbed), the folder name, times, outcomes and the counts an agent reported (tokens, a cost it gave, rate-limit hits), never the rest of a prompt, replies or terminal text. Delete it under Privacy and your data in Settings.</p>
    {/if}
    <HealthSection servers={connected} {health} {now} onask={onhealth} {rttOf} />
  {/if}
</div>
</div>
