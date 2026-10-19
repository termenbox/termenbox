<script lang="ts">
  import { onDestroy, onMount } from 'svelte';
  import type { ServerListItem } from './lib/protocol';
  import type { DirRecord } from './lib/directory';
  import { randomClaimSecret } from './lib/directory';
  import { installCommand, scanCommand as scanCommandFor, parseClaim, parseServerLink, waitForServer, previewClaim, checkReachable, firewallCommand, UDP_RANGE, ClaimTimeout, type Reachability, type ClaimPreview, type RunChoice } from './lib/onboard';
  import { decodeQrFile, startCameraScan } from './lib/scan';
  import { wsUrl } from './lib/servers';
  import { Dialog } from './lib/ui';

  export let origin: string;
  /** A secret that arrived by link or earlier scan: start waiting for that server right away. */
  export let prefillSecret: string | null = null;
  export let onclose: () => void;
  export let onadd: (server: ServerListItem) => void;
  /** A scanned or pasted re-add link for a server that is already registered. */
  export let onserver: (serverId: string) => void;

  type Tab = 'paste' | 'scan' | 'url';
  let tab: Tab = prefillSecret ? 'paste' : 'paste';
  let name = '';
  let secret = prefillSecret ?? randomClaimSecret();
  /** Where sessions run. The host choices give this browser a shell on the machine itself, so they need consent. */
  let where: 'container' | 'host' = 'container';
  type HostAs = 'user' | 'me' | 'root';
  const hostMode: Record<HostAs, RunChoice> = { user: 'host-user', me: 'host-me', root: 'host' };
  let hostAs: HostAs = 'user';
  let consent = false;
  $: mode = where === 'container' ? 'container' : hostMode[hostAs];
  $: if (where === 'container' || hostAs) consent = false; // each choice of a host mode is agreed to on its own
  $: allowed = mode === 'container' || consent;
  $: command = installCommand(origin, secret, mode);
  let waiting = false;
  let error = '';
  let copied = false;
  let manualUrl = '';
  let manualClaim = '';
  let video: HTMLVideoElement;
  let stopCamera: (() => void) | null = null;
  let scanning = false;
  let abort: AbortController | null = null;

  /** The server has announced itself; its preview is fetched and shown, and nothing is claimed until Add is tapped (0117). */
  let found: DirRecord | null = null;
  let preview: ClaimPreview | null = null;
  let previewing = false;
  let previewName = '';

  async function showPreview(record: DirRecord, s: string) {
    found = record;
    previewing = true;
    error = '';
    abort?.abort();
    abort = new AbortController();
    try {
      preview = await previewClaim({ record }, s, abort.signal);
      previewName = name.trim() || preview.name;
    } catch (e) {
      if (e instanceof DOMException && e.name === 'AbortError') return;
      error = `Could not read what this server is: ${e instanceof Error ? e.message : 'failed'}`;
    } finally {
      previewing = false;
    }
  }

  /** The only place a claim leaves the dialog: the client connects with the token on this server's first connection. */
  /** Set once the server is added and the reachability check is running or done (decision 0118). */
  let added: DirRecord | null = null;
  let checking = false;
  let reach: Reachability | null = null;

  function confirm() {
    if (!found) return;
    const record = found;
    onadd({ server_id: found.server_id, server_name: (preview ? previewName : name).trim(), server_url: '', record: found, registered_at: Date.now(), claim: secret });
    // The server stays added whatever the check says; it only advises (GUARDRAIL connections-warn-never-drop).
    added = record;
    checking = true;
    void checkReachable({ record }).then((r) => { reach = r; checking = false; });
  }

  async function begin(s: string) {
    abort?.abort();
    abort = new AbortController();
    secret = s;
    waiting = true;
    error = '';
    try {
      const record = await waitForServer(origin, s, abort.signal);
      waiting = false;
      await showPreview(record, s);
    } catch (e) {
      waiting = false;
      if (e instanceof DOMException && e.name === 'AbortError') return;
      error = e instanceof ClaimTimeout ? 'Nothing arrived within 15 minutes. Start again to get a new command.' : e instanceof Error ? e.message : 'Failed';
    }
  }

  $: scanCommand = scanCommandFor(origin, mode);

  /** A tap selects the whole command, so it can also be copied by hand on browsers without clipboard access. */
  function selectAll(e: MouseEvent) {
    const sel = window.getSelection();
    if (!sel) return;
    const range = document.createRange();
    range.selectNodeContents(e.currentTarget as Node);
    sel.removeAllRanges();
    sel.addRange(range);
  }

  let linkText = '';
  let linkError = '';
  function submitLink() {
    linkError = '';
    if (!parseServerLink(linkText) && !parseClaim(linkText)) { linkError = 'That is not a Termenbox link or code.'; return; }
    gotText(linkText);
  }

  async function copy(text: string) {
    await navigator.clipboard.writeText(text);
    copied = true;
    setTimeout(() => (copied = false), 2000);
  }

  function gotText(text: string) {
    const known = parseServerLink(text);
    if (known) {
      stopScan();
      onserver(known);
      return;
    }
    const s = parseClaim(text);
    if (!s) {
      error = 'That is not a Termenbox link, code or QR.';
      return;
    }
    stopScan();
    void begin(s);
  }

  async function startScan() {
    error = '';
    scanning = true;
    stopCamera = await startCameraScan(video, gotText, (m) => {
      error = m;
      scanning = false;
    });
  }

  function stopScan() {
    stopCamera?.();
    stopCamera = null;
    scanning = false;
  }

  async function onFile(e: Event) {
    const file = (e.target as HTMLInputElement).files?.[0];
    if (!file) return;
    error = '';
    try {
      const text = await decodeQrFile(file);
      if (text) gotText(text);
      else error = 'No QR code found in that image. Make sure the whole QR is visible, then try again.';
    } catch {
      error = 'Could not read that image.';
    }
  }

  function addByUrl() {
    try {
      wsUrl(manualUrl);
    } catch {
      error = 'Enter the server URL (https://...)';
      return;
    }
    onadd({ server_url: manualUrl.trim(), server_name: name.trim(), registered_at: Date.now(), claim: manualClaim.trim() || undefined });
    onclose();
  }

  function choose(t: Tab) {
    stopScan();
    error = '';
    tab = t;
  }

  onMount(() => {
    // Flow A starts waiting immediately: the secret is already in the command the user is about to paste.
    void begin(secret);
  });
  onDestroy(() => {
    abort?.abort();
    stopScan();
  });
</script>

<Dialog title="Add Server" {onclose}>
    <!-- Once a server is found the setup steps are done: only its preview or result is shown. -->
    {#if !found}
    <form class="have-link" on:submit|preventDefault={submitLink} style="margin-bottom: 16px;">
      <label for="have-link" style="display: block; font-size: 0.875rem; margin-bottom: 6px; color: var(--fg-muted);">Already added on another device? Paste its link or code</label>
      <div style="display: flex; gap: 8px;">
        <input id="have-link" bind:value={linkText} placeholder="termenbox-server:… or a link" autocapitalize="off" autocomplete="off" spellcheck="false" style="flex: 1; min-width: 0;" />
        <button type="submit" class="secondary" disabled={!linkText.trim()}>Add</button>
      </div>
      {#if linkError}<p role="alert" style="color: var(--danger); margin-top: 6px; font-size: 0.875rem;">{linkError}</p>{/if}
    </form>
    <div style="display: flex; gap: 8px; margin-bottom: 16px;">
      <button class={tab === 'paste' ? '' : 'secondary'} on:click={() => choose('paste')} style="flex: 1; padding: 8px;">Paste a command</button>
      <button class={tab === 'scan' ? '' : 'secondary'} on:click={() => choose('scan')} style="flex: 1; padding: 8px;">Scan a QR</button>
    </div>

    <label style="display: block; font-size: 0.875rem; margin-bottom: 6px; color: var(--fg-muted);">Name</label>
    <input bind:value={name} placeholder="Leave empty to use the server's hostname" maxlength="40" style="width: 100%; margin-bottom: 16px;" />

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

    {#if tab === 'paste'}
      <p style="color: var(--fg-muted); margin-bottom: 8px;">Run this once on your VPS. It installs and starts everything, and the server appears here by itself. Running as your own user needs no root; the other choices ask for sudo.</p>
      {#if allowed}
        <pre class="command" on:click={selectAll}>{command}</pre>
        <button on:click={() => copy(command)} style="width: 100%; margin-bottom: 12px;">{copied ? 'Copied' : 'Copy command'}</button>
      {:else}
        <p class="locked">Tick the box above to see the command.</p>
      {/if}
    {:else if tab === 'scan'}
      <p style="color: var(--fg-muted); margin-bottom: 8px;">On a new VPS run this. It shows a QR code. Scan it here, or take a screenshot of it and choose the screenshot. Delete the screenshot afterwards.</p>
      {#if allowed}
        <pre class="command" on:click={selectAll}>{scanCommand}</pre>
        <button class="secondary" on:click={() => copy(scanCommand)} style="width: 100%; margin-bottom: 12px;">{copied ? 'Copied' : 'Copy command'}</button>
      {:else}
        <p class="locked">Tick the box above to see the command.</p>
      {/if}
      <video bind:this={video} muted playsinline style="width: 100%; border-radius: 8px; background: #000; margin-bottom: 8px; display: {scanning ? 'block' : 'none'};"></video>
      <div style="display: flex; gap: 8px; margin-bottom: 12px;">
        {#if scanning}
          <button class="secondary" on:click={stopScan} style="flex: 1;">Stop camera</button>
        {:else}
          <button on:click={startScan} style="flex: 1;">Scan with camera</button>
        {/if}
        <label class="secondary" style="flex: 1; text-align: center; cursor: pointer; padding: 10px; border: 1px solid var(--border); border-radius: 8px;">
          Choose screenshot
          <input type="file" accept="image/*" on:change={onFile} style="display: none;" />
        </label>
      </div>
    {/if}
    {/if}

    {#if added}
      <div class="preview" role="status" aria-live="polite">
        <strong>Server added</strong>
        {#if checking}
          <p class="note">Checking that this device can reach it…</p>
        {:else if reach?.state === 'reachable'}
          <p class="note">This device can reach it.</p>
        {:else if reach?.state === 'degraded'}
          <p class="note">This device reached it only through the WebSocket fallback{reach.port ? `; UDP port ${reach.port} did not work` : ''}. Sessions work but can feel slower. It stays in your list.</p>
          <p class="note">To use the direct path, an admin opens the range for Termenbox once:</p>
          <pre class="command">{firewallCommand}</pre>
        {:else if reach?.state === 'unreachable'}
          <p class="note">This device could not reach it{reach.port ? ` on UDP port ${reach.port}` : ''}. It stays in your list and keeps retrying.</p>
          <p class="note">On the server, an admin opens the range for Termenbox once:</p>
          <pre class="command">{firewallCommand}</pre>
          <p class="note">Servers use a port from {UDP_RANGE.replace(':', '-')}. Your own network (cafe, work or mobile data) may also block UDP; try another network if the port is open.</p>
        {:else}
          <p class="note">Could not tell whether this device can reach it. It stays in your list.</p>
        {/if}
        <button on:click={onclose} disabled={checking} style="width: 100%;">Done</button>
      </div>
    {:else if preview}
      <div class="preview" role="group" aria-label="What you are adding">
        <strong>This server is ready to add</strong>
        <dl>
          <dt>Runs sessions</dt>
          <dd>{preview.mode === 'container' ? 'in an isolated container' : `on the machine itself, as ${preview.run_user || 'its own user'}`}</dd>
          <dt>Version</dt>
          <dd>{preview.version}</dd>
        </dl>
        <label for="preview-name" style="display: block; font-size: 0.875rem; margin-bottom: 6px; color: var(--fg-muted);">Name</label>
        <input id="preview-name" bind:value={previewName} maxlength="40" style="width: 100%; margin-bottom: 12px;" />
        <p style="color: var(--fg-muted); font-size: 0.8rem; margin: 0 0 12px;">Nothing is claimed yet. Tap Add to become this server's owner; closing this leaves the link valid until it expires.</p>
        <button on:click={confirm} style="width: 100%;">Add</button>
      </div>
    {:else if previewing}
      <p style="color: var(--fg-muted); font-size: 0.875rem;">Your server appeared. Reading what it is…</p>
    {:else if found && error}
      <button class="secondary" on:click={() => found && showPreview(found, secret)} style="width: 100%; margin-bottom: 8px;">Try again</button>
      <button class="secondary" on:click={confirm} style="width: 100%;">Add without a preview</button>
    {/if}
    {#if waiting}
      <p style="color: var(--fg-muted); font-size: 0.875rem;">Waiting for your server to appear… this can take a minute.</p>
    {/if}
    {#if error}
      <p style="color: var(--danger); margin: 8px 0; font-size: 0.875rem;">{error}</p>
    {/if}

    <details style="margin-top: 16px;">
      <summary style="cursor: pointer; color: var(--fg-muted); font-size: 0.875rem;">Advanced: connect by URL (tunnel or reverse proxy)</summary>
      <input bind:value={manualUrl} placeholder="https://xxxx.trycloudflare.com" style="width: 100%; margin: 8px 0;" />
      <input bind:value={manualClaim} placeholder="Registration token (first connection only)" style="width: 100%; margin-bottom: 8px;" />
      <button class="secondary" on:click={addByUrl} disabled={!manualUrl} style="width: 100%;">Add and connect</button>
    </details>
</Dialog>

<style>
  .where { border: 1px solid var(--border); border-radius: 10px; padding: 12px; margin: 0 0 16px; }
  .where legend { padding: 0 6px; font-size: 0.875rem; color: var(--fg-muted); }
  .choice { display: flex; gap: 10px; align-items: flex-start; margin: 6px 0; cursor: pointer; }
  .choice input { margin-top: 4px; }
  .choice small { color: var(--fg-muted); }
  .sub { margin-left: 24px; border-left: 2px solid var(--border); padding-left: 12px; }
  .warning { margin: 10px 0; padding: 10px 12px; border: 1px solid var(--danger); border-radius: 8px; font-size: 0.8rem; background: rgba(248, 81, 73, 0.08); }
  .preview { border: 1px solid var(--border); border-radius: 10px; padding: 12px; margin: 0 0 12px; }
  .preview dl { display: grid; grid-template-columns: auto 1fr; gap: 4px 12px; margin: 8px 0 12px; font-size: 0.875rem; }
  .preview dt { color: var(--fg-muted); }
  .preview dd { margin: 0; }
  .note { color: var(--fg-muted); font-size: 0.85rem; margin: 8px 0; }
  .locked { color: var(--fg-muted); font-size: 0.85rem; margin: 0 0 12px; }
  .command { background: var(--bg); border: 1px solid var(--border); border-radius: 8px; padding: 12px; font-family: monospace; font-size: 0.75rem; white-space: pre-wrap; overflow-wrap: anywhere; user-select: all; cursor: copy; margin: 0 0 8px; }
</style>
