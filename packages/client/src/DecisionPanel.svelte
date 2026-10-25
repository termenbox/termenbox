<script lang="ts">
  import type { Snippet } from 'svelte';
  import { DecisionCard, OptionButton, Composer } from './lib/ds';
  import { ANSWER_WAIT_MS, replyLine, type Decision } from './lib/decision';
  import { TIER_LABEL } from './lib/sessionView';
  import { ageText } from './lib/cards';

  /** The decision card as every surface uses it (Home, the inbox, the workspace view; decisions 0100, 0103, 0104).
   *  A tap sends exactly the option's keys through `onpick`, and the card stays where it is: it resolves only when
   *  the server clears the Attention item and the parent stops passing the decision. Until then it says it is waiting.
   *  If the server has not cleared it after a few seconds, the card says so and the options come back.
   *  `detail` is what the question is about when the card has more than a line to show (the diff of an ACP edit). */
  let { decision, now, composer = true, server, detail, unhealthy, onpick, onreply, onopen, class: className = '' }: {
    decision: Decision; now: number; composer?: boolean; server?: string; detail?: Snippet; unhealthy?: boolean;
    onpick: (index: number) => void; onreply?: (line: string) => void; onopen?: () => void; class?: string;
  } = $props();

  let draft = $state('');
  let sent = $state<{ what: string; index?: number } | null>(null);
  let slow = $state(false);
  const identity = $derived(`${decision.key}|${decision.promptId ?? decision.prompt?.id ?? ''}|${decision.since ?? ''}`);

  // A new decision (another prompt or another wait) starts clean.
  $effect(() => { identity; sent = null; slow = false; });
  $effect(() => {
    if (!sent) return;
    const t = setTimeout(() => { sent = null; slow = true; }, ANSWER_WAIT_MS);
    return () => clearTimeout(t);
  });

  const pick = (i: number) => { if (sent) return; slow = false; sent = { what: 'Answer', index: i }; onpick(i); };
  const reply = (text: string) => { const line = replyLine(text); if (!line || sent) return; slow = false; sent = { what: 'Reply' }; onreply?.(line); };
  const labelOf = (i: number) => decision.options[i];
</script>

<DecisionCard agent={decision.agent} workspace={decision.name} task={decision.task} waited={decision.since ? ageText(decision.since, now) : undefined}
  context={server} {unhealthy} step={decision.step} tier={TIER_LABEL[decision.tier]} question={decision.question} class={className}>
  {#if detail}{@render detail()}{/if}
  {#if decision.options.length}
    <div class="flex flex-col gap-2" data-hero-options aria-busy={!!sent}>
      {#each decision.options as c, i (i)}
        <OptionButton hint={String(i + 1)} label={c} primary={i === 0} disabled={!!sent}
          detail={sent?.index === i ? 'Sent. Waiting for the agent to take it.' : undefined} onclick={() => pick(i)} />
      {/each}
    </div>
  {/if}
  {#if composer && onreply}
    <Composer bind:value={draft} label="Write a reply" placeholder="Write a reply" disabled={!!sent} onsend={reply}
      class="border-0 bg-transparent p-0" />
  {/if}
  <p class="text-sm text-soft empty:hidden" role="status" data-decision-status>
    {#if sent}{sent.what === 'Reply' ? 'Reply sent.' : `Sent “${labelOf(sent.index ?? -1) ?? 'your answer'}”.`} Waiting for the agent to take it.{:else if slow}Nothing changed yet. The agent may still be busy; try again, or open the terminal.{/if}</p>
  {#if onopen}<button type="button" onclick={onopen} class="min-h-11 rounded-card bg-raised px-4 text-base font-semibold text-fg">Open workspace</button>{/if}
</DecisionCard>
