<script lang="ts">
  import { reading } from './lib/attentionMeter';
  import type { AttentionDay } from './lib/protocol';

  /** The owner's own number, one line (decision 0169): human seconds per agent-hour, and the share of asks answered without them. */
  let { days, now, window = 7 }: { days: AttentionDay[]; now: number; window?: number } = $props();
  const r = $derived(reading(days, now, window));
</script>

<p class="text-sm text-soft" data-attention-meter>
  {#if r.secPerAgentHour === null}
    Attention: no agent time measured yet
  {:else}
    Attention: {r.secPerAgentHour} s per agent-hour ({window} days){#if r.autoPercent !== null}, {r.autoPercent}% of asks answered without you{/if}
  {/if}
</p>
