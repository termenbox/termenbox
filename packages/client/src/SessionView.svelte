<script lang="ts">
  import { AgentBadge, StatusChip, Composer, type FleetState } from './lib/ds';
  import DecisionPanel from './DecisionPanel.svelte';
  import Timeline from './Timeline.svelte';
  import type { Step } from './lib/decision';
  import { TIER_LABEL, answerFor, type ViewModel } from './lib/sessionView';
  import type { ClientMessage } from './lib/protocol';
  import { ageText } from './lib/cards';
  /** Decision 0103: header (agent, task, time, state), the parsed decision, the cleaned tail, the composer, the
   *  tier in use and a way to the full terminal. Never an empty state for an agent: with no view yet it still
   *  shows the header, the composer and the terminal button. The decision is the shared card (decision 0104): its
   *  free-text field is the composer below, so the card does not repeat it. `waitingSince` and `step` come from the
   *  agent's events and the hook (T1 only). */
  let { name, agent, fleet = 'idle', task, startedAt, waitingSince, step, now = Date.now(), view = null, onreply, onanswer, onterminal }: {
    name: string; agent?: string; fleet?: FleetState; task?: string; startedAt?: number; waitingSince?: number; step?: Step; now?: number; view?: ViewModel | null;
    onreply?: (line: string) => void; onanswer?: (message: ClientMessage) => void; onterminal?: () => void;
  } = $props();
  let draft = $state('');
  const tier = $derived(TIER_LABEL[view?.tier ?? 't3']);
  const title = $derived(task || view?.task);
  const prompt = $derived(view?.prompt);
  const send = (i: number) => { const m = view && answerFor(view, i); if (m) onanswer?.(m); };
</script>

<div class="flex h-full min-h-0 flex-col bg-bg text-fg" data-testid="session-view">
  <header class="flex items-center gap-3 border-b border-line px-4 py-3">
    <AgentBadge {agent} state={fleet} />
    <div class="flex min-w-0 flex-1 flex-col">
      <b class="truncate text-base font-semibold">{name}</b>
      <span class="truncate text-sm text-soft">{title || 'No task reported'}{#if startedAt} · {ageText(startedAt, now)}{/if}</span>
    </div>
    <StatusChip state={fleet} />
  </header>

  <div class="flex min-h-0 flex-1 flex-col gap-3 overflow-y-auto p-4">
    {#if prompt}
      <DecisionPanel composer={false} {now}
        decision={{ key: view?.workspace ?? name, promptId: prompt.id, name, agent, task: title, since: waitingSince, question: prompt.question || 'The agent is asking', tier: view?.tier ?? 't3',
          step: view?.tier === 't1' ? step : undefined, options: prompt.options.map((o) => o.label) }}
        onpick={send} />
    {/if}

    {#if view?.tier === 't1' && view.timeline}<Timeline timeline={view.timeline} />{/if}

    <section aria-label="Screen" class="flex flex-col gap-1">
      <div class="flex items-center justify-between gap-2">
        <h3 class="text-xs font-semibold tracking-wide text-muted uppercase">Screen</h3>
        <span class="text-xs text-muted" data-testid="tier" title={tier.detail}>{tier.name} view · {tier.detail}</span>
      </div>
      <pre class="max-h-[40vh] overflow-auto rounded-card bg-term p-3 font-mono text-sm leading-snug whitespace-pre-wrap text-term-fg" data-testid="tail">{#if view?.tail.length}{view.tail.join('\n')}{:else}Reading the screen…{/if}</pre>
    </section>

    <button type="button" onclick={onterminal} onpointerdown={(e) => e.preventDefault()}
      class="min-h-11 rounded-card border border-line bg-panel px-4 text-base font-semibold text-fg">Open full terminal</button>
  </div>

  <Composer bind:value={draft} placeholder="Reply to {name}" onsend={(l) => { if (l.trim()) onreply?.(l); }} />
</div>
