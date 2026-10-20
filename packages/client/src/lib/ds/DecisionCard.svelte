<script lang="ts">
  import type { Snippet } from 'svelte';
  import AgentBadge from './AgentBadge.svelte';
  import HealthDot from './HealthDot.svelte';
  import type { FleetState } from './states';
  import { cn } from '../ui/utils';
  /** The frame of a decision (decision 0104): who asks, the task, how long it has waited, the question with its
   *  context, then the options. The options and the free-text field are children. `task`, `step` and `context` are
   *  text from an agent or a terminal: they are rendered as text and nothing else. `tier` says how much to trust the options. */
  let { agent, command, session, waited, task, context, unhealthy, step, tier, question, state = 'needs-you', children, class: className = '' }:
    { agent?: string; command?: string; session: string; waited?: string; task?: string; context?: string; unhealthy?: boolean; step?: { name: string; summary: string };
      tier?: { name: string; detail: string }; question: string; state?: FleetState; children: Snippet; class?: string } = $props();
</script>

<section class={cn('flex flex-col gap-3 rounded-[20px] border border-line bg-panel p-4', className)} aria-label="Decision">
  <header class="flex items-center gap-3">
    <AgentBadge {agent} {command} {state} />
    <div class="flex min-w-0 flex-1 flex-col">
      <b class="truncate text-base font-semibold">{session}</b>
      {#if task}<span class="truncate text-sm text-soft" data-decision-task>{task}</span>{/if}
    </div>
    {#if waited}<small class="shrink-0 text-xs font-semibold text-warn" data-decision-waited>Waiting {waited}</small>{/if}
  </header>
  {#if context || unhealthy}
    <p class="flex flex-wrap items-center gap-x-3 text-base text-soft">{#if context}<span>{context}</span>{/if}{#if unhealthy}<HealthDot />{/if}</p>
  {/if}
  <h2 class="text-title leading-tight font-bold tracking-tight">{question}</h2>
  {#if step}
    <div class="flex flex-col gap-1 rounded-card bg-sunken p-3" data-decision-step>
      {#if step.name}<small class="text-xs font-semibold text-soft">The agent is about to use {step.name}</small>{/if}
      {#if step.summary}<code class="font-mono text-sm break-words whitespace-pre-wrap text-fg">{step.summary}</code>{/if}
    </div>
  {/if}
  {@render children()}
  {#if tier}<small class="text-xs text-muted" data-decision-tier title={tier.detail}>{tier.name} · {tier.detail}</small>{/if}
</section>
