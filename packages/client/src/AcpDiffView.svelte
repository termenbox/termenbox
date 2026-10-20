<script lang="ts">
  import { diffRows, diffStat, type Diff } from './lib/acpStream';
  import type { Part } from './lib/markdown';
  import { copyText } from './lib/clipboard';

  /** One file's change (U07b): additions and removals as the agent sent them, unchanged runs folded. `marks` are the
   *  search marks of the path and of each shown line, in that order (the order `entryRuns` lists them). */
  let { diff, open, ontoggle, marks = [], collapsible = true }: {
    diff: Diff; open: boolean; ontoggle?: () => void; marks?: (Part[] | undefined)[]; collapsible?: boolean;
  } = $props();
  const rows = $derived(diffRows(diff));
  const stat = $derived(diffStat(diff));
  let copied = $state(false);
  async function copy() {
    copied = await copyText(rows.map((r) => (r.t === 'gap' ? '' : r.text)).filter(Boolean).join('\n'));
    setTimeout(() => (copied = false), 1500);
  }
  const mark = 'rounded-sm bg-warn/30';
  const here = 'rounded-sm bg-warn text-on-warn';
  const sign = { '+': 'text-ok', '-': 'text-bad', ' ': 'text-muted' } as const;
  // Each shown line takes the next mark after the path's.
  const indexed = $derived.by(() => {
    let k = 1;
    return rows.map((r) => (r.t === 'gap' ? { r, m: undefined as Part[] | undefined } : { r, m: marks[k++] }));
  });
</script>

{#snippet runs(ps: Part[] | undefined, text: string)}
  {#if ps}{#each ps as p, i (i)}{#if p.hit}<mark class={p.current ? here : mark} data-hit data-current={p.current ? '' : undefined}>{p.text}</mark>{:else}{p.text}{/if}{/each}{:else}{text}{/if}
{/snippet}

<div class="overflow-hidden rounded-card bg-term text-term-fg" data-diff>
  <div class="flex items-center gap-2 ps-3">
    {#if collapsible}
      <button type="button" aria-expanded={open} onclick={ontoggle} onpointerdown={(e) => e.preventDefault()}
        class="flex min-h-11 min-w-0 flex-1 items-center gap-2 rounded-none bg-transparent p-0 text-start text-sm font-normal text-term-fg">
        <span aria-hidden="true" class="w-3 text-muted">{open ? '▾' : '▸'}</span>
        <span class="min-w-0 flex-1 truncate font-mono">{@render runs(marks[0], diff.path)}</span>
        <span class="shrink-0 font-mono text-xs"><span class="text-ok">+{stat.added}</span> <span class="text-bad">−{stat.removed}</span></span>
      </button>
    {:else}
      <div class="flex min-h-11 min-w-0 flex-1 items-center gap-2 text-sm">
        <span class="min-w-0 flex-1 truncate font-mono">{@render runs(marks[0], diff.path)}</span>
        <span class="shrink-0 font-mono text-xs"><span class="text-ok">+{stat.added}</span> <span class="text-bad">−{stat.removed}</span></span>
      </div>
    {/if}
    {#if open}<button type="button" onclick={copy} onpointerdown={(e) => e.preventDefault()} aria-label="Copy this change"
      class="min-h-11 min-w-11 rounded-none bg-transparent px-3 text-xs font-semibold text-soft">{copied ? 'Copied' : 'Copy'}</button>{/if}
  </div>
  {#if open}
    <pre class="max-h-[50vh] overflow-auto border-t border-line/50 py-2 font-mono text-sm leading-snug" aria-label="Changes in {diff.path}">{#each indexed as { r, m }, i (i)}{#if r.t === 'gap'}<span class="block px-3 text-muted" aria-label="{r.count} unchanged lines">⋯ {r.count} unchanged</span>{:else}<span class="block px-3 {r.t === '+' ? 'bg-ok/10' : r.t === '-' ? 'bg-bad/10' : ''}"><span class="select-none {sign[r.t]}">{r.t}</span> {@render runs(m, r.text)}</span>{/if}{/each}</pre>
    {#if diff.truncated}<p class="border-t border-line/50 px-3 py-2 text-xs text-muted">The agent’s change is longer; the rest is not shown.</p>{/if}
  {/if}
</div>
