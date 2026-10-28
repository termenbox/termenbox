<script lang="ts">
  import { cn } from '../utils';
  import Sheet from './Sheet.svelte';
  import AgentIcon from '../../../AgentIcon.svelte';
  import { onMount } from 'svelte';

  interface Option<T> {
    value: T;
    label: string;
    icon?: string; // agent/command id for AgentIcon
    description?: string;
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
    side = 'bottom',
    class: className = '',
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
    side?: 'bottom' | 'right' | 'center';
    class?: string;
  } = $props();

  let filter = $state('');

  const filteredOptions = $derived(
    filter
      ? options.filter(o => o.label.toLowerCase().includes(filter.toLowerCase()) || String(o.value).toLowerCase().includes(filter.toLowerCase()))
      : options
  );

  function select(value: any) {
    onselect(value);
    open = false;
  }

  // Handle Escape key globally when open
  $effect(() => {
    if (!open) return;
    function handleKeydown(e: KeyboardEvent) {
      if (e.key === 'Escape') {
        open = false;
        onclose?.();
      }
    }
    window.addEventListener('keydown', handleKeydown);
    return () => window.removeEventListener('keydown', handleKeydown);
  });
</script>

<Sheet bind:open {title} {side} onclose={onclose} class={className}>
  {#if filterable}
    <div class="mb-3">
      <input
        type="search"
        bind:value={filter}
        placeholder={filterPlaceholder}
        class="w-full rounded-control border border-line bg-sunken px-3 py-2 text-base text-fg outline-none focus:border-accent focus:ring-2 focus:ring-accent/40"
        aria-label={filterPlaceholder}
        autocomplete="off"
      />
    </div>
  {/if}

  <div class="max-h-[60vh] overflow-y-auto">
    {#if filteredOptions.length === 0}
      <p class="text-center text-soft py-8">{emptyMessage}</p>
    {:else}
      <div class="grid gap-2" role="listbox" aria-label={title}>
        {#each filteredOptions as opt}
          <button
            type="button"
            role="option"
            aria-selected={selected === opt.value}
            class={cn(
              'w-full text-left p-3 rounded-control border transition-colors',
              selected === opt.value
                ? 'border-accent bg-accent/10 text-fg'
                : 'border-line bg-bg hover:border-accent/50 hover:bg-raised',
            )}
            on:click={() => select(opt.value)}
          >
            <div class="flex items-center gap-3">
              {#if opt.icon}
                <AgentIcon agent={opt.icon} command={opt.icon} size={24} />
              {/if}
              <div class="flex-1 min-w-0">
                <div class="font-medium truncate">{opt.label}</div>
                {#if opt.description}
                  <div class="text-sm text-soft truncate">{opt.description}</div>
                {/if}
              </div>
              {#if selected === opt.value}
                <svg viewBox="0 0 24 24" width="20" height="20" fill="currentColor" class="text-accent flex-shrink-0">
                  <path d="M9 16.17L4.83 12l-1.42 1.41L9 19 21 7l-1.41-1.41L9 16.17z"/>
                </svg>
              {/if}
            </div>
          </button>
        {/each}
      </div>
    {/if}
  </div>
</Sheet>