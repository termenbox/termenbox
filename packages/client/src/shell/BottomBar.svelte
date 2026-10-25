<script lang="ts">
  import { DESTINATIONS, type Destination } from '../lib/router';
  import { cn } from '../lib/ui/utils';
  /**
   * The phone's four destinations, in thumb reach. 56 px tall, 8 px between the buttons (DR1, DR2). A tap never moves
   * focus (K2): the soft keyboard stays as the user left it. The bottom inset is the shell's (AppShell pads it), so
   * the bar sits above the home indicator without a second inset here.
   */
  let { current, onnavigate, needsYou = 0, class: className = '' }: { current: Destination; onnavigate: (d: Destination) => void; needsYou?: number; class?: string } = $props();
  const ICONS: Record<Destination, string> = {
    home: 'M3 11.5 12 4l9 7.5M5.5 10v9.5h13V10',
    workspaces: 'M4 5h16v11H4zM8 20h8M12 16v4',
    activity: 'M3 12h4l3-7 4 14 3-7h4',
    settings: 'M12 9.5a2.5 2.5 0 1 0 0 5 2.5 2.5 0 0 0 0-5ZM4 12h2.5M17.5 12H20M12 4v2.5M12 17.5V20',
  };
</script>

<nav aria-label="Main" data-shell="bottom-bar" class={cn('flex shrink-0 gap-2 border-t border-line bg-panel px-2 py-1', className)}>
  {#each DESTINATIONS as d (d.id)}
    {@const count = d.id === 'workspaces' ? needsYou : 0}
    <button type="button" data-destination={d.id} aria-current={current === d.id ? 'page' : undefined}
      aria-label={count ? `${d.label}, ${count} need you` : undefined}
      onpointerdown={(e) => e.preventDefault()} onclick={() => onnavigate(d.id)}
      class={cn('relative flex h-14 min-w-0 flex-1 flex-col items-center justify-center gap-0.5 rounded-control bg-transparent p-0 text-xs font-semibold text-soft', current === d.id && 'bg-raised text-accent-text')}>
      <svg width="22" height="22" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d={ICONS[d.id]} /></svg>
      <span>{d.label}</span>
      {#if count}<span aria-hidden="true" class="absolute top-1 left-1/2 ml-2 min-w-5 rounded-full bg-warn px-1 text-center text-xs font-bold text-on-warn">{count}</span>{/if}
    </button>
  {/each}
</nav>
