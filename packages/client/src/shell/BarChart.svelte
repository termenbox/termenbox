<script lang="ts">
  /** Bars on a baseline for the Activity charts. Every bar says its value in text, a failed part is hatched, and the whole
   *  chart has a spoken summary: colour never carries the meaning alone. Heights share one scale. */
  import { shownValues, type Bar } from '../lib/history';
  let { bars, label }: { bars: Bar[]; label: string } = $props();
  const total = (b: Bar) => b.parts.reduce((t, p) => t + p.n, 0);
  const max = $derived(Math.max(1, ...bars.map(total)));
  // A value is text above its bar; where bars are too narrow for their neighbours' text, only the taller ones keep theirs.
  let width = $state(0);
  const radius = $derived(width ? Math.max(0, Math.ceil((Math.max(0, ...bars.map((b) => b.value.length)) * 6.6 + 4) / (width / bars.length)) - 1) : 0);
  const shown = $derived(shownValues(bars, radius));
  const summary = $derived(bars.filter((b) => b.spoken).map((b) => b.spoken).join('; '));
  const FILL = { ok: 'bg-ok', bad: 'bg-bad', warn: 'bg-warn' } as const;
  const HATCH = 'background-image: repeating-linear-gradient(135deg, transparent 0 3px, rgb(0 0 0 / 0.4) 3px 5px)';
</script>

<div role="img" aria-label={summary ? `${label}: ${summary}` : `${label}: nothing yet`} class="flex flex-col gap-1" data-bar-chart>
  <div class="flex h-28 items-stretch gap-0.5" bind:clientWidth={width}>
    {#each bars as b, i (i)}
      <div class="flex min-w-0 flex-1 flex-col items-center justify-end" data-bar={b.spoken ? 'on' : 'off'}>
        <span class="h-4 text-xs leading-4 text-soft" aria-hidden="true">{shown[i] ? b.value : ''}</span>
        <div class="flex w-full flex-col-reverse overflow-hidden rounded-t-control border-b border-line" style:height="{Math.round((total(b) / max) * 88)}px">
          {#each b.parts as p}
            {#if p.n > 0}<div class={FILL[p.tone]} style:flex="{p.n} 1 0" style={p.tone === 'bad' ? HATCH : undefined}></div>{/if}
          {/each}
        </div>
      </div>
    {/each}
  </div>
  <div class="flex gap-0.5" aria-hidden="true">
    {#each bars as b, i (i)}<span class="min-w-0 flex-1 text-center text-xs text-soft">{b.label}</span>{/each}
  </div>
</div>
