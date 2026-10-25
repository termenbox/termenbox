<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { rememberFocus } from './lib/focus';
  import { ageText } from './lib/cards';
  import type { InboxItem } from './lib/inbox';
  import type { AgentMap, WorkspaceAgent } from './lib/agents';
  import DecisionPanel from './DecisionPanel.svelte';
  import { decisionFor } from './lib/decision';
  import { kindOf } from './lib/home';

  export let items: InboxItem[];
  export let now: number;
  export let agents: AgentMap = {};
  export let onanswer: (key: string, choice: number, prompt: NonNullable<WorkspaceAgent['prompt']>) => void = () => {};
  export let onreply: (key: string, text: string) => void = () => {};
  export let onopen: (key: string) => void;
  export let onhome: () => void = () => {};
  export let onread: () => void;
  export let onclear: () => void;
  export let onclose: () => void;
  let dialog: HTMLDialogElement;
  const restoreFocus = rememberFocus();
  onDestroy(restoreFocus);
  onMount(() => { dialog.showModal(); });
  // Words, never colour alone; the task title is text from the agent and is shown as text only.
  const WORDS = { needs_input: 'needs you', finished_unseen: 'finished', failed: 'failed', unknown: '' } as const;
  // A question that is still open is the decision card, answerable here (decision 0104); anything else is a line that opens the workspace.
  function liveCard(it: InboxItem, map: AgentMap) {
    const a = map[it.key];
    if (it.kind !== 'needs_input' || kindOf(a) !== 'needs_input' || (a?.attention && a.attention.raised_at !== it.at)) return null;
    return decisionFor({ key: it.key, name: it.name, since: a?.attention?.raised_at ?? it.at, question: a?.attention?.prompt, title: a?.attention?.task?.title ?? it.title }, a, now);
  }
  function open(key: string) { onclose(); onopen(key); }
  function home() { onclose(); onhome(); }
</script>

<dialog bind:this={dialog} aria-label="Inbox" on:cancel|preventDefault={onclose} on:click={(e) => { if (e.target === dialog) onclose(); }}>
  <div class="box">
    <div class="row"><h2>Inbox</h2><button class="secondary" aria-label="Close inbox" on:click={onclose}>×</button></div>
    {#if items.length === 0}
      <p>Nothing yet. A workspace that needs you, finishes or fails shows up here.</p>
    {:else}
      <div class="row"><button class="secondary" on:click={home}>Open Home</button><button class="secondary" on:click={onread}>Mark all read</button><button class="secondary" on:click={onclear}>Clear</button></div>
      <ul>
        {#each items as it (it.id)}
          {@const card = liveCard(it, agents)}
          <li>
            {#if card}
              <DecisionPanel decision={card} {now} onpick={(i) => card.prompt && onanswer(it.key, i, card.prompt)} onreply={(text) => onreply(it.key, text)} onopen={() => open(it.key)} />
            {:else}
            <button type="button" class="item" class:unread={!it.read} on:click={() => open(it.key)}>
              <span class="head"><strong>{it.name}</strong> {WORDS[it.kind] ?? ''}<span class="age">{ageText(it.at, now)}</span></span>
              {#if it.title}<span class="title">{it.title}</span>{/if}
            </button>
            {/if}
          </li>
        {/each}
      </ul>
    {/if}
  </div>
</dialog>

<style>
  dialog { margin: auto auto 0; width: min(100%, 420px); max-height: calc(var(--app-height, 100dvh) - 16px); overflow-y: auto; padding: 0; border: 1px solid var(--border); border-radius: 16px 16px 0 0; background: var(--bg-secondary); color: var(--fg); }
  dialog::backdrop { background: #0008; }
  .box { padding: 16px; display: flex; flex-direction: column; gap: 8px; }
  .row { display: flex; justify-content: space-between; align-items: center; gap: 8px; }
  button { min-height: 44px; min-width: 44px; }
  ul { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; gap: 4px; }
  .item { width: 100%; text-align: left; display: flex; flex-direction: column; gap: 2px; background: transparent; border: 1px solid var(--border); border-radius: 8px; padding: 6px 10px; color: var(--fg); }
  .item.unread { border-left: 4px solid var(--danger); }
  .head { display: flex; gap: 6px; align-items: baseline; }
  .age { margin-left: auto; font-size: 0.7rem; color: var(--fg-muted); }
  .title { font-size: 0.8rem; color: var(--fg-muted); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  p { font-size: 0.8rem; color: var(--fg-muted); }
</style>
