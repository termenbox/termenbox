<script lang="ts">
  // Each Add Server failure state, rendered by the real step components inside the real Dialog, chosen by ?s=<state>.
  // The e2e opens them at 375x667 and checks that nothing scrolls and every detail fold starts closed.
  import { Dialog } from '../src/lib/ui';
  import { DIAGNOSES, type Cause, type Connect } from '../src/lib/addServer';
  import StepIndicator from '../src/addserver/StepIndicator.svelte';
  import ConnectStep from '../src/addserver/ConnectStep.svelte';
  import DoneStep from '../src/addserver/DoneStep.svelte';

  const state = new URLSearchParams(location.search).get('s') ?? '';
  const CAUSE: Record<string, Cause> = {
    expired_link: 'link_expired', bad_link: 'bad_link', not_found: 'server_stopped', udp_closed: 'udp_closed', permission_denied: 'local_network_denied',
    wrong_code: 'wrong_code', burned_code: 'code_burned', different_network: 'different_network', guest_isolation: 'guest_isolation',
  };
  const addresses = [
    { key: 'a', label: 'Direct, LAN', state: 'failed' as const, reason: 'timeout' as const },
    { key: 'b', label: 'Direct, internet', state: 'failed' as const, reason: 'timeout' as const },
    { key: 'c', label: 'Tunnel', state: 'failed' as const, reason: 'refused' as const },
  ];
  const connect: Connect | null = CAUSE[state] ? { phase: 'code', addresses, diagnosis: DIAGNOSES[CAUSE[state]] } : null;
  const done = state === 'udp_closed_no_fallback' ? { state: 'unreachable' as const, port: 4443, message: 'timed out' } : state === 'udp_closed_with_fallback' ? { state: 'degraded' as const, port: 4443 } : null;
</script>

<div style="position: fixed; inset: 0;">
  <Dialog title="Add Server" class="p-4" onclose={() => {}}>
    {#if connect}
      <StepIndicator step="connect" />
      <ConnectStep {connect} codeRead={true} scanning={false} onscan={() => {}} onstopscan={() => {}} onfile={() => {}} ontext={() => {}} onaction={() => {}} />
    {:else if done}
      <StepIndicator step="done" />
      <DoneStep checking={false} reach={done} pathLabel="" ondone={() => {}} />
    {/if}
  </Dialog>
</div>
