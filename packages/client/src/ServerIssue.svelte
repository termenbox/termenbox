<script lang="ts">
  import { Popover } from 'bits-ui';
  import type { HubServer } from './lib/hub';
  import PathReport from './PathReport.svelte';
  import PortHelp from './PortHelp.svelte';

  /** Why a server is not connected, behind an icon instead of in the row: the row keeps its size, and the reader
   *  opens the reason when they want it. Yellow while Termenbox is still trying or the server is paused, red once it
   *  gave up. */
  let { name, status, udpPort }: { name: string; status: HubServer; udpPort?: number } = $props();
  const failed = $derived(status.state === 'unreachable' || status.state === 'closed');
</script>

<Popover.Root>
  <Popover.Trigger aria-label={`Why ${name} is not connected`} data-testid="server-issue" data-severity={failed ? 'error' : 'warning'}
    class={`grid size-11 shrink-0 place-items-center rounded-control border-0 bg-transparent p-0 hover:bg-raised ${failed ? 'text-bad' : 'text-warn'}`}>
    {#if failed}
      <svg width="22" height="22" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true"><circle cx="12" cy="12" r="10"></circle><line x1="15" y1="9" x2="9" y2="15"></line><line x1="9" y1="9" x2="15" y2="15"></line></svg>
    {:else}
      <svg width="22" height="22" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true"><path d="M10.3 3.9 1.8 18a2 2 0 0 0 1.7 3h17a2 2 0 0 0 1.7-3L13.7 3.9a2 2 0 0 0-3.4 0z"></path><line x1="12" y1="9" x2="12" y2="13"></line><line x1="12" y1="17" x2="12.01" y2="17"></line></svg>
    {/if}
  </Popover.Trigger>
  <Popover.Content align="end" sideOffset={4} class="z-[200] max-h-[70vh] w-[min(22rem,calc(100vw-2rem))] overflow-y-auto rounded-card border border-line bg-panel p-4 text-sm text-fg shadow-sheet">
    <b class="block text-base">{name}</b>
    <p class="mt-1 text-soft" data-testid="server-issue-text">{status.reason || (status.state === 'paused' ? 'Paused: Termenbox does not connect to this server until you resume it.' : `The server is ${status.state}.`)}</p>
    {#if status.failures?.length}<PathReport failures={status.failures} />{/if}
    {#if status.state === 'unreachable'}
      <p class="mt-3 text-soft">The server may be stopped, or its UDP port may be blocked. On the machine, <code>systemctl status termenbox</code> (or <code>systemctl --user status termenbox</code> for a user install) says whether it runs; a server whose port is taken stops with a message in <code>journalctl</code>.</p>
      {#if udpPort}<PortHelp port={udpPort} />{/if}
    {/if}
  </Popover.Content>
</Popover.Root>
