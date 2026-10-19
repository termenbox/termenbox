<script lang="ts">
  import AgentBadge from './AgentBadge.svelte';
  import StatusChip from './StatusChip.svelte';
  import type { FleetState } from './states';
  import { cn } from '../ui/utils';
  /** A session in the queue or the fleet list: name, one line of what it is doing, its state. */
  let { name, summary, meta, state, agent, command, onclick, unhealthy, class: className = '' }:
    { name: string; summary?: string; meta?: string; state: FleetState; agent?: string; command?: string; onclick?: () => void; unhealthy?: boolean; class?: string } = $props();
</script>

<button type="button" {onclick} class={cn('flex min-h-20 w-full items-center gap-3 rounded-card border border-line bg-panel p-3 text-left text-fg', className)}>
  <AgentBadge {agent} {command} {state} />
  <span class="flex min-w-0 flex-1 flex-col">
    <b class="text-base font-semibold">{name}</b>
    {#if summary}<span class="truncate text-sm text-soft">{summary}</span>{/if}
    {#if meta}<small class="text-xs text-muted">{meta}</small>{/if}
  </span>
  {#if unhealthy}
    <span class="flex items-center gap-1 text-xs text-red-500" aria-label="Server unhealthy">
      <span class="relative top-[1px] w-1.5 h-1.5 rounded-full bg-red-500" aria-hidden="true"></span>
    </span>
  {/if}
  <StatusChip {state} />
</button>
