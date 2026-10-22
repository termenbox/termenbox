<script lang="ts">
  import type { RunChoice } from '../lib/onboard';

  /** Step 1 (decision 0127): one command and one Copy button. The run-mode choice is folded under Options. */
  export let command: string;
  export let allowed: boolean;
  export let name: string;
  export let where: 'container' | 'host';
  export let hostAs: 'user' | 'me' | 'root';
  export let consent: boolean;
  export let onnext: () => void;

  let copied = false;
  async function copy() {
    await navigator.clipboard.writeText(command);
    copied = true;
    setTimeout(() => (copied = false), 2000);
  }
  /** A tap selects the whole command, so it can also be copied by hand on browsers without clipboard access. */
  function selectAll(e: MouseEvent) {
    const sel = window.getSelection();
    if (!sel) return;
    const range = document.createRange();
    range.selectNodeContents(e.currentTarget as Node);
    sel.removeAllRanges();
    sel.addRange(range);
  }
</script>

<p class="lead">Run this once on your server. It installs and starts everything, then prints a QR, a link and a code.</p>
{#if allowed}
  <pre class="command" on:click={selectAll}>{command}</pre>
  <button on:click={copy} style="width: 100%; margin-bottom: 8px;" data-testid="copy-command">{copied ? 'Copied' : 'Copy command'}</button>
{:else}
  <p class="locked">Tick the box under Options to see the command.</p>
{/if}
<button class="secondary" on:click={onnext} style="width: 100%; margin-bottom: 8px;" data-testid="install-next">I ran it</button>

<details class="options" data-testid="install-options">
  <summary>Options</summary>
  <label class="label" for="server-name">Name</label>
  <input id="server-name" bind:value={name} placeholder="Leave empty to use the server's hostname" maxlength="40" style="width: 100%; margin-bottom: 12px;" />

  <fieldset class="where">
    <legend>Where should sessions run?</legend>
    <label class="choice">
      <input type="radio" bind:group={where} value="container" />
      <span><strong>In an isolated container</strong> (recommended)<br /><small>Sessions run in a Docker container and cannot see the rest of your VPS. Docker is installed if missing.</small></span>
    </label>
    <label class="choice">
      <input type="radio" bind:group={where} value="host" />
      <span><strong>On the machine itself</strong> (no Docker)<br /><small>Sessions run directly on your VPS, where your repos and agent tools already are.</small></span>
    </label>
    {#if where === 'host'}
      <div class="sub">
        <label class="choice">
          <input type="radio" bind:group={hostAs} value="user" />
          <span>As a dedicated user <code>termenbox</code> (recommended)<br /><small>Created for you. It can only touch its own files and what you give it access to. Needs sudo; the command asks for it.</small></span>
        </label>
        <label class="choice">
          <input type="radio" bind:group={hostAs} value="me" />
          <span>As the user who runs the command<br /><small>Sessions open as you, in your home, with your repos, keys and agent logins. Nothing new is created and <strong>no root is needed</strong>: it runs as a service of your own. Anything that would need root (starting at boot when your system does not allow it, a firewall rule) is listed at the end, for you to run if you want it.</small></span>
        </label>
        <label class="choice">
          <input type="radio" bind:group={hostAs} value="root" />
          <span>As <code>root</code><br /><small>Full control of the machine. Needs sudo; the command asks for it.</small></span>
        </label>
      </div>
      <div class="warning" role="note">
        <strong>This gives your browser a shell on your server.</strong>
        Anything you run in a session can read and change this machine's files and run programs as {hostAs === 'root' ? 'root, so it can change anything' : hostAs === 'me' ? 'you, with access to everything you can reach, including your SSH keys and agent logins' : 'the termenbox user'}.
        Only continue on a machine you own. The installer lists every change it makes first and can undo them all.
      </div>
      <label class="choice">
        <input type="checkbox" bind:checked={consent} />
        <span>I understand and want sessions to run on this machine{hostAs === 'root' ? ' as root' : hostAs === 'me' ? ' as my own user' : ''}.</span>
      </label>
    {/if}
  </fieldset>
</details>

<style>
  .lead { color: var(--fg-muted); font-size: 0.875rem; margin: 0 0 8px; }
  .label { display: block; font-size: 0.875rem; margin: 8px 0 6px; color: var(--fg-muted); }
  summary { cursor: pointer; color: var(--fg-muted); font-size: 0.875rem; }
  .where { border: 1px solid var(--border); border-radius: 10px; padding: 12px; margin: 0; }
  .where legend { padding: 0 6px; font-size: 0.875rem; color: var(--fg-muted); }
  .choice { display: flex; gap: 10px; align-items: flex-start; margin: 6px 0; cursor: pointer; }
  .choice input { margin-top: 4px; }
  .choice small { color: var(--fg-muted); }
  .sub { margin-left: 24px; border-left: 2px solid var(--border); padding-left: 12px; }
  .warning { margin: 10px 0; padding: 10px 12px; border: 1px solid var(--danger); border-radius: 8px; font-size: 0.8rem; background: rgba(248, 81, 73, 0.08); }
  .locked { color: var(--fg-muted); font-size: 0.85rem; margin: 0 0 12px; }
  .command { background: var(--bg); border: 1px solid var(--border); border-radius: 8px; padding: 10px; font-family: monospace; font-size: 0.7rem; white-space: pre-wrap; overflow-wrap: anywhere; user-select: all; cursor: copy; margin: 0 0 8px; }
</style>
