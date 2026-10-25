<script lang="ts">
  import DecisionPanel from './DecisionPanel.svelte';
  import AcpMarkdown from './AcpMarkdown.svelte';
  import AcpDiffView from './AcpDiffView.svelte';
  import { baseName, blocksOf, diffRunCount, entryMarks, permissionOutcome, planProgress, toolStatus, TRUNCATED_NOTE, type Entry } from './lib/acpStream';
  import { decorate, type Part } from './lib/markdown';
  import type { Decision } from './lib/decision';

  /** One entry of the workspace stream (U07b). Agent text is drawn as text only. A permission that is open is the same
   *  decision card as on Home, answered with one tap; once answered it is one line of the log. `current` is the search
   *  hit (0-based, within this entry) that stands out, or -1. `announce`: an error line is read out at once (an alert) only
   *  when it is new, not when it is history that arrives with the stream. */
  let { entry, query = '', current = -1, asking = false, announce = false, opened, ontoggle, onanswer, workspaceKey, name, agent, now }: {
    entry: Entry; query?: string; current?: number; asking?: boolean; announce?: boolean; opened: Set<string>; ontoggle: (key: string) => void;
    onanswer: (promptId: string, optionId: string) => void; workspaceKey: string; name: string; agent?: string; now: number;
  } = $props();

  const marks = $derived(entryMarks(entry, query, current, asking));
  const mark = 'rounded-sm bg-warn/30';
  const here = 'rounded-sm bg-warn text-on-warn';
  const STATUS_COLOR: Record<string, string> = { pending: 'text-muted', in_progress: 'text-info', completed: 'text-ok', failed: 'text-bad' };
  const KIND_WORD: Record<string, string> = {
    read: 'Wants to read a file', edit: 'Wants to change a file', delete: 'Wants to delete a file', move: 'Wants to move a file', search: 'Wants to search',
    execute: 'Wants to run a command', think: 'Wants to think', fetch: 'Wants to fetch a page', switch_mode: 'Wants to switch mode', other: 'Wants to use a tool',
  };
  const decision = (e: Extract<Entry, { kind: 'permission' }>): Decision => ({
    key: workspaceKey, promptId: e.promptId, name, agent, question: e.title || 'The agent asks permission', tier: 't0', options: e.options.map((o) => o.name),
  });
</script>

{#snippet runs(ps: Part[] | undefined, text: string)}
  {#if ps}{#each ps as p, i (i)}{#if p.hit}<mark class={p.current ? here : mark} data-hit data-current={p.current ? '' : undefined}>{p.text}</mark>{:else}{p.text}{/if}{/each}{:else}{text}{/if}
{/snippet}

{#if entry.kind === 'user'}
  <div class="flex gap-2 rounded-card bg-raised px-3 py-2" data-kind="user">
    <span aria-hidden="true" class="font-mono text-accent-text select-none">›</span>
    <p class="min-w-0 flex-1 text-base break-words whitespace-pre-wrap"><span class="sr-only">You: </span>{@render runs(marks[0], entry.text)}</p>
  </div>
{:else if entry.kind === 'message'}
  <div data-kind="message">
    <AcpMarkdown blocks={decorate(blocksOf(entry.text), query, current)} />
    {#if entry.truncated}<p class="pt-1 text-sm text-muted" data-truncated>{TRUNCATED_NOTE}</p>{/if}
  </div>
{:else if entry.kind === 'thought'}
  <details class="text-soft" open={current >= 0} data-kind="thought">
    <summary class="flex min-h-11 cursor-pointer items-center text-sm">Thinking</summary>
    <p class="pb-2 text-sm break-words whitespace-pre-wrap">{@render runs(marks[0], entry.text)}</p>
    {#if entry.truncated}<p class="pb-2 text-sm text-muted" data-truncated>{TRUNCATED_NOTE}</p>{/if}
  </details>
{:else if entry.kind === 'tool'}
  {@const st = toolStatus(entry.status)}
  <div class="flex flex-col gap-2 rounded-card bg-sunken p-3 font-mono text-sm" role="group" aria-label="{entry.title}, {entry.toolKind}, {st.word}" data-kind="tool" data-status={entry.status}>
    <div class="flex items-start gap-2">
      <span aria-hidden="true" class={STATUS_COLOR[entry.status] ?? 'text-muted'}>{st.glyph}</span>
      <span class="shrink-0 rounded-sm bg-raised px-1.5 text-xs text-soft uppercase">{entry.toolKind}</span>
      <span class="min-w-0 flex-1 break-words">{@render runs(marks[0], entry.title)}</span>
      <small class="shrink-0 text-xs text-muted">{st.word}</small>
    </div>
    {#if entry.locations.length}
      <div class="flex flex-wrap gap-1 text-xs text-soft">
        {#each entry.locations as l, i (i)}<span class="rounded-sm bg-raised px-1.5" title={l.path}>{@render runs(marks[1 + i], baseName(l.path))}{l.line ? `:${l.line}` : ''}</span>{/each}
      </div>
    {/if}
    {#if !asking}
      {#each entry.diffs as d, k (k)}
        {@const first = 1 + entry.locations.length + entry.diffs.slice(0, k).reduce((n, x) => n + diffRunCount(x), 0)}
        <AcpDiffView diff={d} open={opened.has(`${entry.id}:${k}`) || current >= 0 && marks.slice(first, first + diffRunCount(d)).some((m) => m?.some((p) => p.current))}
          ontoggle={() => ontoggle(`${entry.id}:${k}`)} marks={marks.slice(first, first + diffRunCount(d))} />
      {/each}
    {/if}
  </div>
{:else if entry.kind === 'plan'}
  {@const p = planProgress(entry.items)}
  <section class="flex flex-col gap-1 rounded-card bg-sunken p-3" aria-label="Plan" data-kind="plan">
    <h3 class="text-sm font-semibold">Plan · {p.done} of {p.total} done</h3>
    <ul class="flex flex-col gap-1 text-sm">
      {#each entry.items as it, i (i)}
        <li class="flex gap-2" aria-label="{it.status === 'completed' ? 'done' : it.status === 'in_progress' ? 'in progress' : 'to do'}: {it.text}">
          <span aria-hidden="true" class={it.status === 'completed' ? 'text-ok' : it.status === 'in_progress' ? 'text-info' : 'text-muted'}>{it.status === 'completed' ? '✓' : it.status === 'in_progress' ? '◐' : '○'}</span>
          <span class="min-w-0 flex-1 break-words {it.status === 'completed' ? 'text-soft' : ''}">{@render runs(marks[i], it.text)}</span>
        </li>
      {/each}
    </ul>
  </section>
{:else if entry.kind === 'permission'}
  {#if entry.state === 'open'}
    <DecisionPanel decision={decision(entry)} {now} composer={false} server={KIND_WORD[entry.toolKind] ?? KIND_WORD.other}
      onpick={(i) => { const o = entry.options[i]; if (o) onanswer(entry.promptId, o.id); }}>
      {#snippet detail()}
        {#each entry.diffs as d, k (k)}<AcpDiffView diff={d} open collapsible={false} marks={[]} />{/each}
        {#if entry.plan}<div class="max-h-[40vh] overflow-y-auto rounded-card bg-sunken p-3" data-plan-text><AcpMarkdown blocks={blocksOf(entry.plan)} /></div>{/if}
      {/snippet}
    </DecisionPanel>
  {:else}
    <p class="flex gap-2 text-sm {entry.state === 'cancelled' ? 'text-muted' : 'text-soft'}" data-kind="permission" data-state={entry.state}>
      <span aria-hidden="true">{entry.state === 'cancelled' ? '–' : '●'}</span>
      <span class="min-w-0 flex-1 break-words">{@render runs(marks[0], entry.title || 'Permission')} · {permissionOutcome(entry)}</span>
    </p>
  {/if}
{:else}
  <p class="flex gap-2 text-sm {entry.level === 'error' ? 'text-bad' : 'text-muted'}" data-kind="notice" data-level={entry.level} role={entry.level === 'error' && announce ? 'alert' : undefined}>
    <span aria-hidden="true">{entry.level === 'error' ? '✕' : '–'}</span>
    <span class="min-w-0 flex-1 break-words">{@render runs(marks[0], entry.text)}</span>
  </p>
{/if}
