<script lang="ts">
  import { kindWord, silentNodes, silentText, type FleetItem } from './lib/fleet';
  import { ageText } from './lib/cards';
  import type { ConductorNodeInfo } from './lib/protocol';

  /** The merged Needs-you list across nodes (decision 0170). Props only: no store, no wire (build-an-agent-view-slice-template-first). */
  let { items, nodes, now, onopen }: { items: FleetItem[]; nodes: ConductorNodeInfo[]; now: number; onopen: (card: string) => void } = $props();
  const silent = $derived(silentNodes(nodes));
</script>

<section class="flex flex-col gap-2" aria-label="Needs you across nodes" data-fleet-needs-you>
  {#each silent as n (n.node)}
    <p class="rounded-card border border-line bg-panel p-3 text-sm text-fg" role="status" data-fleet-silent={n.node}>{silentText(n, now)}</p>
  {/each}
  {#each items as it (it.card)}
    <button type="button" onclick={() => onopen(it.card)} disabled={it.silent} data-fleet-item={it.card}
      class="flex min-h-11 w-full flex-col items-start rounded-card border border-line bg-panel p-3 text-left text-fg">
      <span class="flex w-full items-center gap-2">
        <b class="min-w-0 flex-1 truncate text-base font-semibold">{it.workspace}</b>
        <span class="rounded-card border border-line px-2 text-xs" data-fleet-chip>{it.nodeName}</span>
      </span>
      {#if it.summary}<span class="w-full truncate text-sm text-soft">{it.summary}</span>{/if}
      <small class="text-xs text-muted">{kindWord(it.kind)} {ageText(it.since, now)}{#if it.silent} · node silent{/if}</small>
    </button>
  {:else}
    <p class="py-6 text-center text-base text-soft">Nothing needs you on any node.</p>
  {/each}
</section>
