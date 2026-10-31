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
      <AgentBadge agent={first.agent.agent} {state} size={28} />
      <button type="button" class="what" title="Open Home" on:click={onhome}>
        <strong>{bannerText(waiting.length)}</strong>
        <span class="name">{#if waiting.length === 1}{first.name}{:else}first: {first.name}{/if}{#if kind} · {kind}{/if}</span>
      </button>
      <button type="button" class="secondary dismiss" aria-label="Dismiss" on:click={() => { if (clearable) ondismiss(first.key, clearable); dismissed = signature; }}>×</button>
      <div class="actions">
        {#each choices as choice, i}
          <!-- pointerdown/mousedown are cancelled so a tap does not take the focus (and the soft keyboard) from the terminal. -->
          <button type="button" class="choice" on:pointerdown|preventDefault on:mousedown|preventDefault on:click={() => first.agent.prompt && onanswer(first.key, i, first.agent.prompt)}>{choice}</button>
        {/each}
        <button type="button" class="open" on:click={() => onopen(first.key)}>Open</button>
      </div>
    </div>
  {/if}
</div>

<style>
  /* A card that floats inset from the edges: badge and words on top, the choices on their own row (R-needs-you card). */
  .banner { --tone: var(--warn); position: absolute; top: 8px; left: 8px; right: 8px; box-sizing: border-box; display: grid; grid-template-columns: auto 1fr auto; align-items: center; gap: 6px 10px; padding: 10px 10px 10px 12px; background: var(--bg-secondary); border: 1px solid color-mix(in oklab, var(--tone) 45%, var(--border)); border-radius: 14px; box-shadow: 0 8px 24px rgb(0 0 0 / 0.35); font-size: 0.85rem; }
  .banner.failed { --tone: var(--bad); }
  .banner.finished { --tone: var(--ok); }
  .what { background: none; border: 0; color: inherit; text-align: left; font: inherit; padding: 0; min-width: 0; display: flex; flex-direction: column; gap: 1px; }
  .what strong { color: var(--tone); }
  .name { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: var(--fg-muted); }
  .actions { grid-column: 1 / -1; display: flex; flex-wrap: wrap; gap: 8px; }
  button { min-height: 40px; min-width: 44px; }
  .choice, .open { padding: 0 16px; border-radius: 999px; }
  .choice { background: var(--tone); color: var(--panel); border: 0; font-weight: 600; }
  .dismiss { padding: 0 10px; border-radius: 999px; }
</style>
