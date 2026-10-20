<script lang="ts">
  import { hostOf, type Block, type Inline, type Part } from './lib/markdown';
  import { copyText } from './lib/clipboard';
  import Self from './AcpMarkdown.svelte';

  /** An agent reply as a document (U07b). The tree was built by `lib/markdown.ts` from untrusted text and holds only
   *  data: every string here is drawn as a text node, never as markup. Search hits arrive already marked (`parts`). */
  let { blocks }: { blocks: Block[] } = $props();
  let copied = $state<string | null>(null);
  async function copy(text: string) {
    const ok = await copyText(text);
    copied = ok ? text : null;
    setTimeout(() => (copied = null), 1500);
  }
  const mark = 'rounded-sm bg-warn/30 text-fg';
  const here = 'rounded-sm bg-warn text-on-warn';
</script>

{#snippet runs(ps: Part[] | undefined, text: string)}
  {#if ps}{#each ps as p, i (i)}{#if p.hit}<mark class={p.current ? here : mark} data-hit data-current={p.current ? '' : undefined}>{p.text}</mark>{:else}{p.text}{/if}{/each}{:else}{text}{/if}
{/snippet}

{#snippet inl(list: Inline[])}
  {#each list as n, i (i)}
    {#if n.t === 'text'}{@render runs(n.parts, n.text)}
    {:else if n.t === 'code'}<code class="rounded-sm bg-sunken px-1 font-mono text-sm break-words">{@render runs(n.parts, n.text)}</code>
    {:else if n.t === 'b'}<strong class="font-semibold">{@render inl(n.inl)}</strong>
    {:else if n.t === 'i'}<em>{@render inl(n.inl)}</em>
    {:else}<a href={n.href} target="_blank" rel="noopener noreferrer" class="break-all text-accent-text underline">{@render runs(n.parts, n.text)}</a> <span class="text-xs text-muted">({hostOf(n.href)})</span>{/if}
  {/each}
{/snippet}

<div class="flex flex-col gap-3 text-base leading-relaxed break-words">
  {#each blocks as b, i (i)}
    {#if b.t === 'p'}<p class="whitespace-pre-wrap">{@render inl(b.inl)}</p>
    {:else if b.t === 'h'}<p role="heading" aria-level={Math.min(6, b.level + 2)} class="text-base font-bold">{@render inl(b.inl)}</p>
    {:else if b.t === 'ul'}<ul class="flex list-disc flex-col gap-1 ps-6">{#each b.items as item, j (j)}<li><Self blocks={item} /></li>{/each}</ul>
    {:else if b.t === 'ol'}<ol class="flex list-decimal flex-col gap-1 ps-6" start={b.start}>{#each b.items as item, j (j)}<li><Self blocks={item} /></li>{/each}</ol>
    {:else if b.t === 'quote'}<blockquote class="border-s-2 border-line ps-3 text-soft"><Self blocks={b.blocks} /></blockquote>
    {:else if b.t === 'hr'}<hr class="border-line" />
    {:else}
      <figure class="overflow-hidden rounded-card bg-term text-term-fg">
        <figcaption class="flex items-center justify-between gap-2 border-b border-line/50 ps-3 text-xs text-muted">
          <span>{b.lang || 'code'}</span>
          <button type="button" onclick={() => copy(b.text)} onpointerdown={(e) => e.preventDefault()} aria-label="Copy this code"
            class="min-h-11 min-w-11 rounded-none bg-transparent px-3 text-xs font-semibold text-soft">{copied === b.text ? 'Copied' : 'Copy'}</button>
        </figcaption>
        <pre class="max-h-[60vh] overflow-auto p-3 font-mono text-sm leading-snug">{@render runs(b.parts, b.text)}</pre>
      </figure>
    {/if}
  {/each}
</div>
