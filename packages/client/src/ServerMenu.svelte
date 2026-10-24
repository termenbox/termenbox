<script lang="ts">
  import { DropdownMenu } from 'bits-ui';
  import { cn } from './lib/ui/utils';

  /** What one server's row can do, behind one button (the three dots): pause or resume, retry a server that is not
   *  connected, send it to another device, remove it from this browser. Each entry is an icon and its words. */
  let { name, paused, canRetry, canShare, onpause, onresume, onretry, onshare, onremove }: {
    name: string; paused: boolean; canRetry: boolean; canShare: boolean;
    onpause: () => void; onresume: () => void; onretry: () => void; onshare: () => void; onremove: () => void;
  } = $props();
  const item = 'flex min-h-11 cursor-pointer items-center gap-3 rounded-control px-3 text-base text-fg outline-none data-[highlighted]:bg-raised';
</script>

<DropdownMenu.Root>
  <DropdownMenu.Trigger aria-label={`Menu of ${name}`} data-testid="server-menu"
    class="grid size-11 shrink-0 place-items-center rounded-control border-0 bg-transparent p-0 text-soft hover:bg-raised">
    <svg width="20" height="20" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"><circle cx="12" cy="5" r="1.8"></circle><circle cx="12" cy="12" r="1.8"></circle><circle cx="12" cy="19" r="1.8"></circle></svg>
  </DropdownMenu.Trigger>
  <DropdownMenu.Content align="end" sideOffset={4} class="z-[200] min-w-56 rounded-card border border-line bg-panel p-1 shadow-sheet">
    {#if paused}
      <DropdownMenu.Item class={item} onSelect={onresume}>
        <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true"><polygon points="6 4 20 12 6 20 6 4"></polygon></svg>Resume
      </DropdownMenu.Item>
    {:else}
      <DropdownMenu.Item class={item} onSelect={onpause}>
        <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true"><rect x="6" y="4" width="4" height="16"></rect><rect x="14" y="4" width="4" height="16"></rect></svg>Pause
      </DropdownMenu.Item>
    {/if}
    {#if canRetry}
      <DropdownMenu.Item class={item} onSelect={onretry}>
        <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true"><polyline points="23 4 23 10 17 10"></polyline><path d="M20.5 15a9 9 0 1 1-2.1-9.4L23 10"></path></svg>Retry now
      </DropdownMenu.Item>
    {/if}
    {#if canShare}
      <DropdownMenu.Item class={item} onSelect={onshare}>
        <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true"><path d="M12 3v12M8 7l4-4 4 4"></path><path d="M5 12v7a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2v-7"></path></svg>Send to another device
      </DropdownMenu.Item>
    {/if}
    <DropdownMenu.Item class={cn(item, 'text-bad')} onSelect={onremove}>
      <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true"><polyline points="3 6 5 6 21 6"></polyline><path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"></path></svg>Remove
    </DropdownMenu.Item>
  </DropdownMenu.Content>
</DropdownMenu.Root>
