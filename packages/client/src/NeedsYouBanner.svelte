<script lang="ts">
  import { bannerChoices, bannerKind, bannerText, type Waiting } from './lib/notify';
  import type { WorkspaceAgent } from './lib/agents';

  export let waiting: Waiting[];
  export let now: number;
  /** The soft keyboard is open: the banner steps aside (the tab strip still shows who needs you). */
  export let collapsed = false;
  export let onopen: (key: string) => void;
  /** The queue behind the banner lives on Home; the text links there. */
  export let onhome: () => void = () => {};
  /** Clears a finished or failed item for good; a question cannot be dismissed. */
  export let ondismiss: (key: string, attentionId: string) => void = () => {};
  export let onanswer: (key: string, choice: number, prompt: NonNullable<WorkspaceAgent['prompt']>) => void;

  let dismissed = '';
  $: signature = waiting.map(w => w.key).join(',');
  $: show = waiting.length > 0 && !collapsed && dismissed !== signature;
  $: first = waiting[0];
  $: choices = first ? bannerChoices(first.agent, now) : [];
  $: kind = first ? bannerKind(first.agent) : '';
  $: clearable = first?.agent.attention && first.agent.attention.kind !== 'needs_input' ? first.agent.attention.id : '';
</script>

<div class="live" aria-live="polite">
  {#if show && first}
    <div class="banner" role="status">
      <button type="button" class="what" title="Open Home" on:click={onhome}><strong>{bannerText(waiting.length)}</strong>{#if waiting.length === 1} · {first.name}{:else} · first: {first.name}{/if}{#if kind} · {kind}{/if}</button>
      {#each choices as choice, i}
        <button type="button" class="choice" on:click={() => first.agent.prompt && onanswer(first.key, i, first.agent.prompt)}>{choice}</button>
      {/each}
      <button type="button" class="open" on:click={() => onopen(first.key)}>Open</button>
      <button type="button" class="secondary dismiss" aria-label="Dismiss" on:click={() => { if (clearable) ondismiss(first.key, clearable); dismissed = signature; }}>×</button>
    </div>
  {/if}
</div>

<style>
  .banner { display: flex; align-items: center; gap: 6px; padding: 2px 8px; min-height: 44px; background: var(--bg-secondary); border-bottom: 2px solid var(--danger); font-size: 0.85rem; }
  .what { background: none; border: 0; color: inherit; text-align: left; font: inherit; padding: 0; flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  button { min-height: 40px; min-width: 44px; }
  .choice, .open { padding: 0 10px; }
  .dismiss { padding: 0 10px; }
</style>
