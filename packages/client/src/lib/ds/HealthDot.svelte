<script lang="ts">
  import { cn } from '../ui/utils';
  /** A server that says it is unhealthy (U14), on the rows of the workspaces it runs. The dot is decoration: the words carry
   *  the meaning, so it does not rely on colour and a screen reader hears the words once. */
  let { label = 'Server unhealthy', class: className = '' }: { label?: string; class?: string } = $props();
</script>

<small class={cn('inline-flex items-center gap-1.5 text-xs font-semibold text-bad', className)} data-server-unhealthy>
  <span class="size-2 shrink-0 rounded-full bg-bad" aria-hidden="true"></span>{label}
</small>
