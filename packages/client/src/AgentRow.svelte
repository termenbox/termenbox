<script lang="ts">
  import AgentIcon from './AgentIcon.svelte';
  import PickerSheet from './lib/ui/sheet/PickerSheet.svelte';
  import { visibleAgents } from './lib/newworkspace';

  /** Three recent agents and a More tile that opens the full list in a layer inside the dialog. Choosing saves nothing: recents are written on create. */
  let {
    templates,
    recent,
    selected,
    onselect,
    moreOpen = $bindable(false),
  }: {
    /** The agents this server can start. */
    templates: { id: string; label: string }[];
    /** Most recent first. */
    recent: string[];
    selected: string;
    onselect: (id: string) => void;
    moreOpen?: boolean;
  } = $props();

  const tiles = $derived(visibleAgents(templates, recent, selected));
  const options = $derived(
    [...new Set([...recent, ...templates.map(t => t.id)])]
      .map(id => templates.find(t => t.id === id))
      .filter((t): t is { id: string; label: string } => !!t)
      .map(t => ({ value: t.id, label: t.label, icon: t.id === 'shell' ? 'bash' : t.id, recent: recent.includes(t.id) })),
  );
  const icon = (id: string) => (id === 'shell' ? { command: 'bash', agent: undefined } : { command: id, agent: id });
</script>

<div class="grid grid-cols-2 gap-2 min-[480px]:grid-cols-4" role="radiogroup" aria-label="Start with" data-testid="agent-row">
  {#each tiles as t (t.id)}
    <button
      type="button"
      role="radio"
      aria-checked={selected === t.id}
      data-testid="agent-tile"
      class="tile"
      class:on={selected === t.id}
      onclick={() => onselect(t.id)}
    >
      <AgentIcon command={icon(t.id).command} agent={icon(t.id).agent} size={24} /><span>{t.label}</span>
    </button>
  {/each}
  {#if templates.length > tiles.length}
    <button type="button" class="tile more" data-testid="agents-more" aria-haspopup="dialog" onclick={() => (moreOpen = true)}>
      <span>More</span><small>{templates.length}</small>
    </button>
  {/if}
</div>

<PickerSheet
  bind:open={moreOpen}
  title="Choose agent"
  {options}
  {selected}
  {onselect}
  filterable
  filterPlaceholder="Filter agents…"
  emptyMessage="No agents match"
  data-testid="agents-layer"
/>

<style>
  .tile { display: flex; align-items: center; gap: 8px; min-height: 48px; padding: 0 12px; text-align: left; background: var(--bg); color: var(--fg); border: 1px solid var(--border); border-radius: 8px; }
  .tile.on { border-color: var(--accent); }
  .tile span { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .more { justify-content: center; background: transparent; color: var(--fg-muted); }
  .more:hover { color: var(--fg); border-color: var(--fg-muted); }
  .more small { color: var(--fg-muted); }
</style>
