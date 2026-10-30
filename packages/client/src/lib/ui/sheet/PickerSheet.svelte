<script lang="ts">
  import { cn } from '../utils';
  import DialogLayer from './DialogLayer.svelte';
  import AgentIcon from '../../../AgentIcon.svelte';
  import { filterOptions, listboxIndex } from './layer';

  interface Option<T> {
    value: T;
    label: string;
    icon?: string; // agent/command id for AgentIcon
    description?: string;
    recent?: boolean; // listed first under a "Recent" heading
  }

  let {
    open = $bindable(false),
    title,
    options,
    selected,
    onselect,
    onclose,
    filterable = false,
    filterPlaceholder = 'Filter…',
    emptyMessage = 'No options',
    class: className = '',
    'data-testid': testid,
  }: {
    open?: boolean;
    title: string;
    options: Option<any>[];
    selected: any;
    onselect: (value: any) => void;
    onclose?: () => void;
    filterable?: boolean;
    filterPlaceholder?: string;
    emptyMessage?: string;
    class?: string;
    'data-testid'?: string;
  } = $props();

  let filter = $state('');
  let list: HTMLElement | undefined = $state();

  const shown = $derived(filterOptions(options, filter));
  // Recent rows first, then the rest; with no recent rows (or a filter on) the list is flat.
  const recent = $derived(filter.trim() ? [] : shown.filter(o => o.recent));
  const rest = $derived(filter.trim() ? shown : shown.filter(o => !o.recent));
  const ordered = $derived([...recent, ...rest]);

  function select(value: any) {
    onselect(value);
    open = false;
  }

  function rows(): HTMLElement[] {
    return list ? [...list.querySelectorAll<HTMLElement>('[role="option"]')] : [];
  }

  function onlistkeydown(e: KeyboardEvent) {
    const all = rows();
    const to = listboxIndex(e.key, all.indexOf(document.activeElement as HTMLElement), all.length);
    if (to === null) return;
    e.preventDefault();
    all[to].focus();
  }

  // The filter takes the focus on open when there is one; otherwise the selected (or first) row does.
  $effect(() => {
    if (!open) filter = '';
  });
</script>

{#snippet row(opt: Option<any>)}
  <button
    type="button"
    role="option"
    aria-selected={selected === opt.value}
    tabindex={selected === opt.value || (selected === undefined && opt === ordered[0]) ? 0 : -1}
    data-autofocus={selected === opt.value && !filterable ? '' : undefined}
    class={cn(
      'flex w-full min-h-12 items-center gap-3 rounded-control border px-3 py-2 text-left transition-colors',
      selected === opt.value ? 'border-accent bg-accent/10 text-fg' : 'border-line bg-bg hover:border-accent/50 hover:bg-raised',
    )}
    onclick={() => select(opt.value)}
  >
    {#if opt.icon}<AgentIcon agent={opt.icon} command={opt.icon} size={24} />{/if}
    <span class="min-w-0 flex-1">
      <span class="block whitespace-normal [overflow-wrap:anywhere] font-medium">{opt.label}</span>
      {#if opt.description}<span class="block whitespace-normal [overflow-wrap:anywhere] text-sm text-soft">{opt.description}</span>{/if}
    </span>
    {#if selected === opt.value}
      <svg viewBox="0 0 24 24" width="20" height="20" fill="currentColor" class="flex-shrink-0 text-accent" aria-hidden="true" data-testid="picker-check">
        <path d="M9 16.17L4.83 12l-1.42 1.41L9 19 21 7l-1.41-1.41L9 16.17z"/>
      </svg>
    {/if}
  </button>
{/snippet}

<DialogLayer bind:open {title} {onclose} class={className}>
  <div data-testid={testid}>
    {#if filterable}
      <div class="mb-3">
        <input
          type="search"
          bind:value={filter}
          placeholder={filterPlaceholder}
          data-autofocus
          class="min-h-12 w-full rounded-control border border-line bg-sunken px-3 py-2 text-base text-fg outline-none focus:border-accent focus:ring-2 focus:ring-accent/40"
          aria-label={filterPlaceholder}
          autocomplete="off"
          onkeydown={(e) => { if (e.key === 'ArrowDown') { e.preventDefault(); rows()[0]?.focus(); } }}
        />
      </div>
    {/if}

    {#if ordered.length === 0}
      <p class="py-8 text-center text-soft">{emptyMessage}</p>
    {:else}
      <!-- svelte-ignore a11y_interactive_supports_focus -->
      <div bind:this={list} role="listbox" aria-label={title} class="grid gap-2" onkeydown={onlistkeydown}>
        {#if recent.length}
          <div class="px-1 pt-1 text-sm font-medium text-soft" role="presentation">Recent</div>
          {#each recent as opt (opt.value)}{@render row(opt)}{/each}
          {#if rest.length}<div class="px-1 pt-2 text-sm font-medium text-soft" role="presentation">All</div>{/if}
        {/if}
        {#each rest as opt (opt.value)}{@render row(opt)}{/each}
      </div>
    {/if}
  </div>
</DialogLayer>
