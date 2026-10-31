<script lang="ts">
  import { bannerChoices, bannerKind, bannerText, type Waiting } from './lib/notify';
  import type { WorkspaceAgent } from './lib/agents';
  import AgentBadge from './lib/ds/AgentBadge.svelte';
  import type { FleetState } from './lib/ds';

  export let waiting: Waiting[];
  export let now: number;
  export let onopen: (key: string) => void;
  /** The queue behind the banner lives on Home; the text links there. */
  export let onhome: () => void = () => {};
  /** Clears a finished or failed item for good; a question cannot be dismissed. */
  export let ondismiss: (key: string, attentionId: string) => void = () => {};
  export let onanswer: (key: string, choice: number, prompt: NonNullable<WorkspaceAgent['prompt']>) => void;

  let dismissed = '';
  $: signature = waiting.map(w => w.key).join(',');
  $: show = waiting.length > 0 && dismissed !== signature;
  $: first = waiting[0];
  $: choices = first ? bannerChoices(first.agent, now) : [];
  $: kind = first ? bannerKind(first.agent) : '';
  let state: FleetState = 'needs-you';
  $: state = kind === 'failed' ? 'failed' : kind === 'finished' ? 'finished' : 'needs-you';
  $: clearable = first?.agent.attention && first.agent.attention.kind !== 'needs_input' ? first.agent.attention.id : '';
</script>

<div class="live" aria-live="polite">
  {#if show && first}
    <div class="banner" class:failed={state === 'failed'} class:finished={state === 'finished'} role="status">
      <AgentBadge agent={first.agent.agent} {state} size={24} />
      <button type="button" class="what" title="Open Home" on:click={onhome}><strong>{bannerText(waiting.length)}</strong>{#if waiting.length === 1} · {first.name}{:else} · first: {first.name}{/if}{#if kind} · {kind}{/if}</button>
      {#each choices as choice, i}
        <!-- pointerdown/mousedown are cancelled so a tap does not take the focus (and the soft keyboard) from the terminal. -->
        <button type="button" class="choice" on:pointerdown|preventDefault on:mousedown|preventDefault on:click={() => first.agent.prompt && onanswer(first.key, i, first.agent.prompt)}>{choice}</button>
      {/each}
      <button type="button" class="open" on:click={() => onopen(first.key)}>Open</button>
      <button type="button" class="secondary dismiss" aria-label="Dismiss" on:click={() => { if (clearable) ondismiss(first.key, clearable); dismissed = signature; }}>×</button>
    </div>
  {/if}
</div>

<style>
  /* One row, as tall as the bar it replaces, so it hides no more of the screen: an inset rounded card in the state's tone. */
  .banner { --tone: var(--warn); position: absolute; top: 4px; left: 6px; right: 6px; box-sizing: border-box; display: flex; align-items: center; gap: 6px; padding: 2px 6px 2px 8px; min-height: 40px; background: var(--bg-secondary); border: 1px solid color-mix(in oklab, var(--tone) 45%, var(--border)); border-left: 4px solid var(--tone); border-radius: 12px; box-shadow: 0 6px 18px rgb(0 0 0 / 0.3); font-size: 0.85rem; }
  .banner.failed { --tone: var(--bad); }
  .banner.finished { --tone: var(--ok); }
  .what { background: none; border: 0; color: inherit; text-align: left; font: inherit; padding: 0; flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .what strong { color: var(--tone); }
  button { min-height: 36px; min-width: 44px; }
  .choice, .open { padding: 0 12px; border-radius: 999px; }
  .choice { background: var(--tone); color: var(--panel); border: 0; font-weight: 600; }
  .dismiss { padding: 0 10px; border-radius: 999px; }
</style>
