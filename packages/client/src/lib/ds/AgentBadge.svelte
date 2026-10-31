<script lang="ts">
  import AgentIcon from '../../AgentIcon.svelte';
  import { STATES, type FleetState } from './states';
  import { cn } from '../ui/utils';
  /** The agent's icon inside a ring that shows its state; the state is also in the accessible name. */
  let { agent, command, state = 'idle', size = 32, class: className = '' }: { agent?: string; command?: string; state?: FleetState; size?: number; class?: string } = $props();
  const s = $derived(STATES[state]);
</script>

<span class={cn('inline-flex shrink-0 rounded-[8px] p-0.5 ring-2', s.ring, className)} role="img" aria-label={s.label}>
  <AgentIcon {agent} {command} {size} />
</span>
