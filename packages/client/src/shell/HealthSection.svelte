<script lang="ts">
  import { ChartFrame, StatusChip } from '../lib/ds';
  import type { HubServer } from '../lib/hub';
  import { clock } from '../lib/history';
  import { CHART_H, healthStatus, metricView, METRICS, rttText, scopeNote, sinceText, unhealthyText, verdictOf, type HealthMap, type MetricView } from '../lib/health';

  /** Server health in Activity (U14, decision 0105): per connected server, the verdict the server itself reached, CPU, RAM
   *  and disk over the last six hours, and this device's round trip to it. Every state is said in words; the lines are
   *  drawn on a shared 0 to 100% scale with the level as a dashed line, and read out as a sentence. */
  let { servers, health, now, onask, rttOf }: {
    servers: HubServer[]; health: HealthMap; now: number; onask: () => void; rttOf: (id: string) => number | null;
  } = $props();
  const nameOf = (s: HubServer) => s.server.server_name || 'Server';
  const mins = (ms: number) => Math.max(1, Math.round(ms / 60_000));
  const overText = (m: MetricView) => (m.overMs ? `At or above ${m.enter}% for about ${mins(m.overMs)} min.` : `Never at ${m.enter}%.`);
</script>

<section aria-labelledby="activity-health" class="flex flex-col gap-3" data-activity-health>
  <h2 id="activity-health" class="text-sm font-semibold text-soft">Server health</h2>
  <p class="text-sm text-soft">CPU, RAM and disk of each server for the last six hours, sampled every 10 seconds. The samples live in the server's memory only: nothing is written to disk, and a restart clears them.</p>
  {#each servers as s (s.id)}
    {@const entry = health[s.id]}
    {@const status = healthStatus(entry, now)}
    {@const answer = entry?.answer}
    {@const v = verdictOf(s.health, answer)}
    <div data-health-server={s.id} data-health-state={status === 'ready' && !answer?.samples.length ? 'empty' : status}>
      {#if status === 'loading'}
        <ChartFrame title={nameOf(s)}><p class="text-sm text-soft" role="status">Loading health…</p></ChartFrame>
      {:else if status === 'unavailable'}
        <ChartFrame title={nameOf(s)}>
          <p class="text-sm text-soft">{nameOf(s)} did not report its health. A server older than this feature sends none: update it, then try again.</p>
          <button type="button" onclick={onask} class="min-h-11 self-start rounded-card bg-raised px-4 text-base font-semibold text-fg">Try again</button>
        </ChartFrame>
      {:else if !answer || !answer.samples.length}
        <ChartFrame title={nameOf(s)}><p class="text-sm text-soft" role="status">{nameOf(s)} has just started. Its first sample arrives within 10 seconds.</p></ChartFrame>
      {:else}
        {@const metrics = METRICS.map((id) => metricView(answer, id))}
        {@const secs = answer.confirm * answer.interval_s}
        <ChartFrame title={nameOf(s)} caption={v?.healthy === false ? `${unhealthyText(v.unhealthy)}, held for ${secs} seconds. It counts as healthy again once ${v.unhealthy.length > 1 ? 'they have' : 'it has'} stayed below the lower level for ${secs} seconds.` : `Healthy: nothing has stayed at its level for ${secs} seconds.`}>
          {#snippet legend()}<span data-health-verdict={v?.healthy === false ? 'unhealthy' : 'healthy'}><StatusChip state={v?.healthy === false ? 'failed' : 'finished'} label={v?.healthy === false ? 'Unhealthy' : 'Healthy'} /></span>{/snippet}
          <div class="flex flex-col gap-4">
            {#each metrics as m (m.id)}
              <div class="flex flex-col gap-1" data-health-metric={m.id}>
                <div class="flex items-baseline justify-between gap-2">
                  <b class="text-sm font-semibold">{m.label}</b>
                  {#if m.now !== undefined}<span class="text-sm font-semibold" data-health-now>{m.now}%</span>{:else}<span class="text-sm text-soft">not available</span>{/if}
                </div>
                {#if m.known}
                  <p class="text-xs text-soft">{m.now}% {scopeNote(m.id, answer.scope)}</p>
                  <div role="img" aria-label={m.spoken} class="relative border-b border-line">
                    <svg viewBox="0 0 100 {CHART_H}" preserveAspectRatio="none" class="block h-16 w-full" aria-hidden="true" data-health-chart>
                      <line x1="0" x2="100" y1={m.levelY} y2={m.levelY} class="stroke-bad" stroke-width="1.5" stroke-dasharray="4 3" vector-effect="non-scaling-stroke" />
                      {#each m.segments as d, i (i)}
                        <path {d} class="fill-none stroke-accent-text" stroke-width="2" stroke-linejoin="round" vector-effect="non-scaling-stroke" />
                      {/each}
                    </svg>
                  </div>
                  <div class="flex justify-between text-xs text-soft" aria-hidden="true"><span>{sinceText(answer)}</span><span>now</span></div>
                  <p class="text-xs text-soft">Highest {m.peak!.value}% at {clock(m.peak!.ts)}. {overText(m)} <span class="text-bad" aria-hidden="true">- -</span> Level {m.enter}%, healthy again at {m.exit}% or less.</p>
                {/if}
              </div>
            {/each}
            <p class="text-sm text-soft" data-health-rtt>Round trip from this device: <span class="font-semibold text-fg">{rttText(rttOf(s.id))}</span>. It depends on your connection, so each device sees its own and it does not count towards the verdict.</p>
          </div>
        </ChartFrame>
      {/if}
    </div>
  {/each}
</section>
