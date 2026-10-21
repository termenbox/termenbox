<script lang="ts">
  import { cn } from '../utils';

  import { copyText } from '../../clipboard';

  let {
    toast,
    onclose,
    onhold = () => {},
    class: className = '',
  }: {
    toast: { message: string; type: 'success' | 'error'; detail?: string } | null;
    onclose: () => void;
    /** The pointer is over the toast (true) or left it (false): the timer waits while someone reads or selects. */
    onhold?: (held: boolean) => void;
    class?: string;
  } = $props();

  let copied = $state(false);
  async function copyDetail() {
    if (toast?.detail && (await copyText(toast.detail))) copied = true;
  }
</script>

<!-- Absolute inside the app shell, which follows the visible area; fixed would follow the layout viewport.
     On a phone it moves to the top: the bottom edge belongs to the terminal key row. -->
{#if toast}
  <div
    role="status"
    class={cn(
      'toast animate-slide-in absolute right-6 bottom-6 z-[1000] rounded-lg border border-l-4 border-border bg-bg-secondary px-6 py-4 shadow-[0_8px_24px_rgba(0,0,0,0.4)]',
      toast.type === 'success' ? 'border-l-success' : 'border-l-danger',
      'max-[720px]:top-[calc(var(--safe-top)+8px)] max-[720px]:right-2 max-[720px]:bottom-auto max-[720px]:left-2 max-[720px]:max-h-[30vh] max-[720px]:overflow-hidden max-[720px]:px-3.5 max-[720px]:py-2.5 max-[720px]:text-[0.8rem] max-[720px]:[overflow-wrap:anywhere]',
      className,
    )}
    onclick={toast.detail ? undefined : onclose}
    onmouseenter={() => onhold(true)}
    onmouseleave={() => onhold(false)}
  >
    {toast.message}
    {#if toast.detail}
      <!-- What actually failed: selectable, and one click copies it for a bug report. -->
      <div class="mt-2 text-[0.8rem] opacity-80 [overflow-wrap:anywhere] select-text">{toast.detail}</div>
      <div class="mt-2 flex gap-3 text-[0.8rem]">
        <button type="button" class="underline" onclick={copyDetail}>{copied ? 'Copied' : 'Copy details'}</button>
        <button type="button" class="underline" onclick={onclose}>Close</button>
      </div>
    {/if}
  </div>
{/if}
