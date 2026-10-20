<script lang="ts">
  /** Settings > Privacy and your data (DECISION-0066): what the hosted service stores, for how long, and a way to delete it. */
  import { DeleteFailed } from '../lib/account';
  import { deletionSummary, plural, type HistoryDeletion } from '../lib/history';
  let { ondelete, onback, onlogout, historyServers, ondeletehistory }: {
    ondelete: () => Promise<number>; onback: () => void; onlogout: () => void;
    /** Connected servers: the ones whose history a deletion reaches. */
    historyServers: number; ondeletehistory: () => Promise<HistoryDeletion>;
  } = $props();
  type Step = { name: 'idle' } | { name: 'confirm' } | { name: 'busy' } | { name: 'done'; servers: number } | { name: 'failed'; message: string; expired: boolean };
  let step = $state<Step>({ name: 'idle' });
  // Swapping a step's buttons would drop the keyboard or screen-reader position, so each step puts focus on its own message.
  // The first idle step is the page just opening: focus goes to the heading instead.
  let moved = false;
  const go = (next: Step) => ((moved = true), (step = next));
  const focus = (el: HTMLElement, active: boolean) => { if (active) el.focus(); };
  const heading = (el: HTMLElement) => el.focus();
  type HStep = { name: 'idle' } | { name: 'confirm' } | { name: 'busy' } | { name: 'done'; text: string } | { name: 'failed'; text: string };
  let hstep = $state<HStep>({ name: 'idle' });
  const hgo = (next: HStep) => ((moved = true), (hstep = next));
  async function removeHistory() {
    hgo({ name: 'busy' });
    const r = await ondeletehistory();
    hgo({ name: r.failedNames.length ? 'failed' : 'done', text: deletionSummary(r) });
  }
  const btn = 'min-h-11 rounded-card px-4 text-base font-semibold disabled:opacity-60';
  const card = 'flex flex-col gap-2 rounded-card border border-line bg-panel p-4';

  async function remove() {
    go({ name: 'busy' });
    try {
      go({ name: 'done', servers: await ondelete() });
    } catch (e) {
      go({ name: 'failed', message: e instanceof Error ? e.message : 'Something went wrong. Nothing was deleted.', expired: e instanceof DeleteFailed && e.expired });
    }
  }
</script>

<div class="flex min-h-0 flex-1 flex-col gap-3 overflow-y-auto p-4" data-shell="privacy">
  <button type="button" class="{btn} self-start bg-raised text-fg" onclick={onback}><span aria-hidden="true">←</span> Settings</button>
  <h1 class="text-title font-bold outline-none" tabindex="-1" use:heading>Privacy and your data</h1>

  <section class={card} aria-labelledby="privacy-stored">
    <h2 id="privacy-stored" class="text-base font-semibold">What we store</h2>
    <ul class="list-disc space-y-1 pl-5 text-base text-soft">
      <li>Your GitHub account id, linked to each server you set up from this app. Not your name, email or GitHub token: the token is used once at sign-in and dropped.</li>
      <li>For each server: its public key, IP address, port, security fingerprint, optional tunnel address, the time it last checked in, and a hash of its one-time setup code.</li>
      <li>A scrambled hash of the address a server checked in from. It only limits abuse and cannot be turned back into the address.</li>
    </ul>
  </section>

  <section class={card} aria-labelledby="privacy-retention">
    <h2 id="privacy-retention" class="text-base font-semibold">How long we keep it</h2>
    <p class="text-base text-soft">A server that has not checked in for 30 days stops being served and is deleted at the next clean-up, along with its link to your account. A running server checks in at least every 7 days, so it stays as long as it runs.</p>
  </section>

  <section class={card} aria-labelledby="privacy-device">
    <h2 id="privacy-device" class="text-base font-semibold">What never reaches us</h2>
    <p class="text-base text-soft">Terminal content goes directly between this browser and your server. Your server list, settings, usage and device key stay in this browser. If you dictate, your browser's own speech service may process the audio; none of it comes to us. The site is hosted on Cloudflare, which sees requests the way any web host does.</p>
  </section>

  <section class={card} aria-labelledby="privacy-history">
    <h2 id="privacy-history" class="text-base font-semibold">What your servers keep</h2>
    <p class="text-base text-soft">Each server keeps a history of what its agents did, so Activity can show a day or a week. It stays on that server and never comes to us. For each task it holds:</p>
    <ul class="list-disc space-y-1 pl-5 text-base text-soft">
      <li>the session name and the agent;</li>
      <li>a short title: the first line of your prompt (or the title Codex or an ACP agent gives the task), at most 80 characters. Tokens, passwords after a keyword, IP addresses, host names with a port, email addresses, links and paths in your home folder are replaced; other words stay as you wrote them, so keep sensitive details out of that first line;</li>
      <li>when the task started and ended, how it ended (finished, failed or stopped), and how long the agent waited on you;</li>
      <li>the name of the folder the agent worked in (the name, not the path);</li>
      <li>counts the agent itself reported, as numbers: tokens in, out and cached, a cost if the agent gave one, and how many times it said it was rate limited. Claude Code's token counts are read from its own transcript file, as numbers only; nothing else from that file is kept. An agent that reports nothing leaves these empty, and Activity says so.</li>
    </ul>
    <p class="text-base text-soft">Never the rest of a prompt, an agent's reply, a plan or any terminal text. Only tasks an agent reports itself are recorded (Claude Code hooks, Codex, ACP agents), not guesses from the terminal screen. By default a server keeps 90 days and at most 20 MB, oldest first; whoever runs the server can change that.</p>
  </section>

  <section class={card} aria-labelledby="privacy-history-delete" data-history-step={hstep.name}>
    <h2 id="privacy-history-delete" class="text-base font-semibold">Delete activity history</h2>
    {#if hstep.name === 'idle'}
      <p class="text-base text-soft">{historyServers ? `Removes the history kept by the ${plural(historyServers, 'connected server', 'connected servers')}. A server that is not connected keeps its own until you connect it and delete again.` : 'Connect a server to delete the history it keeps.'}</p>
      <button type="button" class="{btn} bg-raised text-fg" disabled={!historyServers} use:focus={moved} onclick={() => hgo({ name: 'confirm' })}>Delete activity history…</button>
    {:else if hstep.name === 'confirm'}
      <p class="text-base text-fg outline-none" tabindex="-1" use:focus={true}>Delete the activity history of {plural(historyServers, 'server', 'servers')}? This cannot be undone.</p>
      <p class="text-base text-soft">Your sessions and agents are not touched. Activity starts empty and fills again as tasks run.</p>
      <div class="flex flex-col gap-2">
        <button type="button" class="{btn} bg-bad/15 text-bad" onclick={removeHistory}>Delete history</button>
        <button type="button" class="{btn} bg-raised text-fg" onclick={() => hgo({ name: 'idle' })}>Keep history</button>
      </div>
    {:else if hstep.name === 'busy'}
      <p class="text-base text-soft outline-none" role="status" tabindex="-1" use:focus={true}>Deleting…</p>
    {:else if hstep.name === 'done'}
      <p class="text-base text-ok outline-none" role="status" tabindex="-1" use:focus={true}>{hstep.text}</p>
    {:else}
      <p class="text-base text-bad outline-none" role="alert" tabindex="-1" use:focus={true}>{hstep.text}</p>
      <div class="flex flex-col gap-2">
        <button type="button" class="{btn} bg-raised text-fg" onclick={removeHistory}>Try again</button>
        <button type="button" class="{btn} bg-raised text-fg" onclick={() => hgo({ name: 'idle' })}>Close</button>
      </div>
    {/if}
  </section>

  <section class={card} aria-labelledby="privacy-delete" data-delete-step={step.name}>
    <h2 id="privacy-delete" class="text-base font-semibold">Delete my data</h2>
    {#if step.name === 'idle'}
      <p class="text-base text-soft">Removes the directory records of the servers you set up from this app, and their link to your account. Servers set up before this option existed are not linked yet; they disappear 30 days after they go quiet.</p>
      <button type="button" class="{btn} bg-raised text-fg" use:focus={moved} onclick={() => go({ name: 'confirm' })}>Delete my data…</button>
    {:else if step.name === 'confirm'}
      <p class="text-base text-fg outline-none" tabindex="-1" use:focus={true}>Delete your records from our directory? This cannot be undone.</p>
      <p class="text-base text-soft">Your servers keep running and keep their sessions, and this browser keeps its saved copy of each server's address, so it can still reach them. A server registers itself again when it next checks in: within 5 minutes if it has not been claimed yet, otherwise within 7 days. To stop that, uninstall it on its machine.</p>
      <div class="flex flex-col gap-2">
        <button type="button" class="{btn} bg-bad/15 text-bad" onclick={remove}>Delete my data</button>
        <button type="button" class="{btn} bg-raised text-fg" onclick={() => go({ name: 'idle' })}>Keep my data</button>
      </div>
    {:else if step.name === 'busy'}
      <p class="text-base text-soft outline-none" role="status" tabindex="-1" use:focus={true}>Deleting…</p>
    {:else if step.name === 'done'}
      <p class="text-base text-ok outline-none" role="status" tabindex="-1" use:focus={true}>
        {step.servers === 0 ? 'Done. No server records were linked to your account.' : `Done. Removed the records of ${step.servers} ${step.servers === 1 ? 'server' : 'servers'} and their link to your account.`}
      </p>
      <p class="text-base text-soft">You are still signed in, and this browser still has your server list. Uninstall a server on its machine to stop it registering again.</p>
    {:else}
      <p class="text-base text-bad outline-none" role="alert" tabindex="-1" use:focus={true}>{step.message}</p>
      <div class="flex flex-col gap-2">
        {#if step.expired}
          <button type="button" class="{btn} bg-raised text-fg" onclick={onlogout}>Sign out</button>
        {:else}
          <button type="button" class="{btn} bg-raised text-fg" onclick={remove}>Try again</button>
        {/if}
        <button type="button" class="{btn} bg-raised text-fg" onclick={() => go({ name: 'idle' })}>Close</button>
      </div>
    {/if}
  </section>
</div>
