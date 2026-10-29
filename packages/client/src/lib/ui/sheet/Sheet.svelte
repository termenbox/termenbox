<script lang="ts">
  import { Dialog } from 'bits-ui';
  import type { Snippet } from 'svelte';
  import { cn } from '../utils';

  let {
    open = $bindable(false),
    title,
    side = 'bottom',
    onclose,
    class: className = '',
    children,
  }: { open?: boolean; title: string; side?: 'bottom' | 'right'; onclose?: () => void; class?: string; children: Snippet } = $props();
</script>

<!-- A panel docked to an edge of the app shell (no portal, see Dialog). -->
<Dialog.Root
  bind:open
  onOpenChange={(o) => {
    if (!o) onclose?.();
  }}
>
  <Dialog.Overlay class="absolute inset-0 z-[100] bg-black/60">
    <Dialog.Content
      class={cn(
        'absolute overflow-y-auto border-border bg-bg-secondary p-4',
        side === 'bottom' ? 'inset-x-0 bottom-0 max-h-[85%] rounded-t-2xl border-t' :
        'inset-y-0 right-0 w-[min(100%,360px)] border-l',
        className,
      )}
    >
      <Dialog.Title class="mb-3 text-base font-bold">{title}</Dialog.Title>
      {@render children()}
    </Dialog.Content>
  </Dialog.Overlay>
</Dialog.Root>
