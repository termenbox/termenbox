<script lang="ts">
  import type { WorkspaceInfo } from './lib/protocol';
  import { runningMs, type WorkspaceAgent } from './lib/agents';
  import { ageText, chipFor, isDone, progress, staleText, toolLine } from './lib/cards';
  import { cacheRemaining, cacheText, expiringSoon } from './lib/cache';
  import { representativeWindow, shortPath, type WorkspaceWindows } from './lib/windows';
  import AgentIcon from './AgentIcon.svelte';

  export let workspace: WorkspaceInfo;
  export let agent: WorkspaceAgent;
  export let now: number;
  export let active = false;
  export let expanded = false;
  export let onopen: () => void;
  export let ontoggle: () => void;
  export let onanswer: (choice: number, prompt: NonNullable<WorkspaceAgent['prompt']>) => void;
  export let serverName = '';
  export let windows: WorkspaceWindows | undefined = undefined;
  export let ttlMs = 300000;
  /** What Keep warm sends; the button is hidden while it is empty. */
  export let keepWarm = '';
  export let onwarm: () => void = () => {};
  export let onpeek: () => void = () => {};
  let pressedAnswer: { choice: number; prompt: NonNullable<WorkspaceAgent['prompt']> } | null = null;

  $: chip = chipFor(agent, now);
  $: prog = progress(agent.plan);
  $: line = toolLine(agent);
  $: running = chip.tone === 'work' ? runningMs(agent, now) : null;
  $: rep = representativeWindow(windows);
  $: remaining = cacheRemaining(agent, now, ttlMs);
  // Only a prompt the agent's own hook confirmed gets answer buttons, and never one that may be out of date.
  $: canAnswer = !!agent.prompt && agent.prompt.confidence === 'high' && !chip.stale && agent.state === 'needs_input';
</script>

<div class="sc" class:active role="group" aria-label={`${workspace.name}: ${chip.label}${chip.stale ? ', ' + staleText(agent, now) : ''}`}>
  <button type="button" class="open" on:pointerdown={() => !active && onpeek()} on:click={onopen}>
    <span class="top">
      <AgentIcon command={rep?.command} agent={rep ? rep.agent : agent.agent} size={20} />
      <strong class="name">{workspace.name}</strong>
      {#if serverName}<span class="badge">{serverName}</span>{/if}
      {#if workspace.backend}<span class="badge">{workspace.backend}</span>{/if}
      <span class="chip {chip.tone}" class:stale={chip.stale}><span aria-hidden="true">{chip.icon}</span> {chip.label}</span>
    </span>
    {#if agent.task}<span class="task">{agent.task.title}</span>{/if}
    {#if line}<span class="line">{line}</span>{/if}
    <span class="meta">
      {#if rep?.path}<span class="path" title={rep.path}>{shortPath(rep.path)}</span>{/if}
      {#if running !== null}<span>{running < 10_000 ? 'just started' : `running ${ageText(0, running)}`}</span>{/if}
      <span>{ageText(agent.ts, now)}</span>
      {#if remaining !== null}<span class="cache" class:soon={expiringSoon(remaining)}>{cacheText(remaining)}</span>{/if}
      {#if prog}<span>plan {prog}</span>{/if}
      {#if chip.stale}<span class="staletext">{staleText(agent, now)}</span>{/if}
    </span>
  </button>
  {#if keepWarm && expiringSoon(remaining)}<button type="button" class="warm" on:click={onwarm}>Keep warm</button>{/if}
  <button type="button" class="more" aria-expanded={expanded} aria-label={expanded ? 'Hide details' : 'Show details'} on:click={ontoggle}>{expanded ? '▲' : '▼'}</button>

  {#if expanded}
    <div class="sheet">
      {#if agent.prompt}
        {#if canAnswer}
          <div class="answers" role="group" aria-label="Answer">
            {#each agent.prompt.choices as choice, i}
              <button type="button" class="answer" on:pointerdown={() => { if (agent.prompt) pressedAnswer = { choice: i, prompt: agent.prompt }; }} on:pointercancel={() => pressedAnswer = null}
                on:click={() => { const answer = pressedAnswer?.choice === i ? pressedAnswer : (agent.prompt ? { choice: i, prompt: agent.prompt } : null); pressedAnswer = null; if (answer) onanswer(answer.choice, answer.prompt); }}>{choice}</button>
            {/each}
          </div>
        {:else}
          <p class="note">
            Waiting for an answer.
            {#if chip.stale}This status may be out of date.{:else}One-tap answers need the agent's hook or plugin connected (Settings → Agent hooks).{/if}
            Open the terminal to answer.
          </p>
        {/if}
      {/if}
      {#if agent.message}<p class="msg">{agent.message}</p>{/if}
      {#if agent.plan && agent.plan.length}
        <ul class="plan" aria-label="Plan">
          {#each agent.plan as item}
            <li><span aria-hidden="true">{isDone(item.status) ? '☑' : '☐'}</span> {item.text} <span class="st">({item.status})</span></li>
          {/each}
        </ul>
      {/if}
      {#if agent.timeline && agent.timeline.length}
        <ol class="timeline" aria-label="Recent tools">
          {#each [...agent.timeline].reverse() as t}
            <li>{t.phase === 'finished' ? '✓' : '…'} {t.name} {t.summary} <span class="st">{ageText(t.ts, now)}</span></li>
          {/each}
        </ol>
      {/if}
      {#if !agent.prompt && !agent.message && !(agent.plan && agent.plan.length) && !(agent.timeline && agent.timeline.length)}
        <p class="note">Nothing reported yet.</p>
      {/if}
    </div>
  {/if}
</div>

<style>
  .path { max-width: 14em; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .cache.soon { color: var(--danger); font-weight: 600; }
  .warm { margin: 0 12px 10px; min-height: 44px; }
  .sc {
    position: relative;
    margin-bottom: 6px;
    border-radius: 8px;
    border: 1px solid var(--border);
    background: var(--bg);
    color: var(--fg);
  }
  .sc.active {
    border-color: var(--accent);
  }
  .open {
    display: flex;
    flex-direction: column;
    gap: 4px;
    width: 100%;
    min-height: 44px;
    padding: 10px 48px 10px 12px;
    text-align: left;
    background: transparent;
    color: inherit;
    border: 0;
    font: inherit;
  }
  .top {
    display: flex;
    align-items: center;
    gap: 6px;
    flex-wrap: wrap;
  }
  .name {
    overflow-wrap: anywhere;
  }
  .badge {
    font-size: 0.65rem;
    padding: 1px 5px;
    border-radius: 4px;
    background: var(--border);
  }
  .chip {
    margin-left: auto;
    font-size: 0.75rem;
    padding: 2px 8px;
    border-radius: 10px;
    border: 1px solid currentColor;
    white-space: nowrap;
  }
  .chip.need {
    color: var(--danger);
    font-weight: 600;
  }
  .chip.work {
    color: var(--accent);
  }
  .chip.done {
    color: var(--success);
  }
  .chip.error {
    color: var(--danger);
  }
  .chip.idle {
    color: var(--fg-muted);
  }
  .chip.stale {
    border-style: dashed;
  }
  .task {
    font-size: 0.85rem;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .line {
    font-size: 0.8rem;
    color: var(--fg-muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .meta {
    display: flex;
    gap: 10px;
    font-size: 0.7rem;
    color: var(--fg-muted);
  }
  .staletext {
    color: var(--danger);
  }
  .more {
    position: absolute;
    top: 0;
    right: 0;
    width: 44px;
    height: 44px;
    padding: 0;
    background: transparent;
    color: var(--fg-muted);
    border: 0;
  }
  .more:hover:not(:disabled) {
    background: transparent;
  }
  .sheet {
    padding: 4px 12px 12px;
    border-top: 1px solid var(--border);
    font-size: 0.8rem;
  }
  .answers {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    margin: 8px 0;
  }
  .answer {
    min-height: 44px;
    min-width: 44px;
    padding: 6px 12px;
    overflow-wrap: anywhere;
  }
  .note,
  .msg {
    margin: 8px 0;
    color: var(--fg-muted);
    overflow-wrap: anywhere;
  }
  .plan,
  .timeline {
    list-style: none;
    margin: 8px 0;
    padding: 0;
    overflow-wrap: anywhere;
  }
  .plan li,
  .timeline li {
    margin-bottom: 3px;
  }
  .st {
    color: var(--fg-muted);
    font-size: 0.7rem;
  }
</style>
