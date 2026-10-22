<script lang="ts">
  import { onDestroy, onMount, tick } from 'svelte';
  import type { Bundle, ServerListItem } from './lib/protocol';
  import type { DirRecord } from './lib/directory';
  import { installCommand, parseClaim, parseServerLink, waitForServer, previewClaim, checkReachable, ClaimTimeout, type Reachability, type ClaimPreview, type RunChoice } from './lib/onboard';
  import { connectPaths, DIAGNOSES, diagnose, initialState, reduce, type ActionId, type Diagnosis, type Event } from './lib/addServer';
  import { buildPaths, isTunnel, orderPaths } from './lib/paths';
  import { pairedServer, parsePairLink, previewPair, type PairPreview } from './lib/pair';
  import { decodeQrFile, startCameraScan } from './lib/scan';
  import { wsUrl } from './lib/servers';
  import { Unreachable } from './lib/transport';
  import { Dialog } from './lib/ui';
  import PortHelp from './PortHelp.svelte';
  import StepIndicator from './addserver/StepIndicator.svelte';
  import InstallStep from './addserver/InstallStep.svelte';
  import ConnectStep from './addserver/ConnectStep.svelte';
  import ConfirmStep from './addserver/ConfirmStep.svelte';
  import DoneStep from './addserver/DoneStep.svelte';

  export let origin: string;
  /** A secret that arrived by link or earlier scan: start waiting for that server right away. */
  export let prefillSecret: string | null = null;
  /** `scan` is the Scan QR button outside the wizard (decision 0127): straight to the camera, then to Confirm. */
  export let start: 'install' | 'scan' = 'install';
  export let onclose: () => void;
  export let onadd: (server: ServerListItem) => void;
  /** A scanned or pasted re-add link for a server that is already registered. */
  export let onserver: (serverId: string) => void;

  let s = start === 'scan' ? reduce(initialState(), { type: 'next' }) : initialState();
  const dispatch = (e: Event) => (s = reduce(s, e));

  let name = '';
  /** The claim secret of a link that arrived alone; the command carries none, so otherwise there is nothing to wait for. */
  let secret: string | null = prefillSecret;
  /** Where sessions run: this machine, as the current user, by default (decision 0113). Host choices give this browser a shell on the machine, so they need consent. */
  let where: 'container' | 'host' = 'host';
  type HostAs = 'user' | 'me' | 'root';
  const hostMode: Record<HostAs, RunChoice> = { user: 'host-user', me: 'host-me', root: 'host' };
  let hostAs: HostAs = 'me';
  let consent = false;
  $: mode = where === 'container' ? 'container' : hostMode[hostAs];
  $: if (where === 'container' || hostAs) consent = false; // each choice of a host mode is agreed to on its own
  $: allowed = mode === 'container' || consent;
  $: command = installCommand(origin, mode);

  let error = '';
  let codeRead = false;
  let video: HTMLVideoElement | null = null;
  let stopCamera: (() => void) | null = null;
  let scanning = false;
  let abort: AbortController | null = null;
  /** The preview failed because the server could not be reached (not because of what it said). */
  let blocked = false;

  /** The server has announced itself; its preview is fetched and shown, and nothing is claimed until Add is tapped (0117). */
  let found: DirRecord | null = null;
  let claimPreview: ClaimPreview | null = null;
  let pairPrev: PairPreview | null = null;
  let previewName = '';

  /** Set once the server is added and the reachability check is running or done (decision 0118). */
  let checking = false;
  let reach: Reachability | null = null;
  let pathLabel = '';

  function ensureConnect() {
    if (s.step === 'install') dispatch({ type: 'next' });
  }

  function fail(diagnosis: Diagnosis) {
    ensureConnect();
    dispatch({ type: 'failed', diagnosis });
  }

  async function showPreview(record: DirRecord, sec: string) {
    found = record;
    ensureConnect();
    dispatch({ type: 'reading' });
    const row = { key: 'record', label: 'Direct', addr: `${record.ip.includes(':') ? `[${record.ip}]` : record.ip}:${record.udp_port}` };
    dispatch({ type: 'address', row: { ...row, state: 'trying' } });
    error = '';
    blocked = false;
    abort?.abort();
    abort = new AbortController();
    try {
      claimPreview = await previewClaim({ record }, sec, abort.signal);
      previewName = name.trim() || claimPreview.name;
      dispatch({ type: 'address', row: { ...row, state: 'won' } });
      dispatch({ type: 'previewed' });
    } catch (e) {
      if (e instanceof DOMException && e.name === 'AbortError') return;
      blocked = e instanceof Unreachable;
      dispatch({ type: 'address', row: { ...row, state: 'failed', reason: blocked ? 'unreachable' : 'other' } });
      const why = e instanceof Error ? e.message : 'failed';
      fail(blocked ? { ...DIAGNOSES.announced_unreachable, detail: `${why}. ${DIAGNOSES.announced_unreachable.detail}` } : { ...DIAGNOSES.server_stopped, title: 'Could not read what this server is.', detail: why });
    }
  }

  async function begin(sec: string) {
    abort?.abort();
    abort = new AbortController();
    secret = sec;
    codeRead = true;
    error = '';
    if (s.step === 'connect') dispatch({ type: 'looking' });
    try {
      const record = await waitForServer(origin, sec, abort.signal);
      ensureConnect();
      dispatch({ type: 'found' });
      await showPreview(record, sec);
    } catch (e) {
      if (e instanceof DOMException && e.name === 'AbortError') return;
      fail(e instanceof ClaimTimeout ? DIAGNOSES.server_stopped : { ...DIAGNOSES.server_stopped, title: e instanceof Error ? e.message : 'Failed', detail: DIAGNOSES.server_stopped.detail });
    }
  }

  /** A pairing link (decision 0123): verified, then every address is tried live so the checklist and the cause are real. */
  async function handlePair(text: string) {
    const link = parsePairLink(text);
    if (!link) return;
    abort?.abort();
    abort = new AbortController();
    const { signal } = abort;
    codeRead = true;
    error = '';
    ensureConnect();
    dispatch({ type: 'looking' });
    const result = await previewPair(link);
    if (!result.ok) return fail(diagnose({ type: 'link', result: result.reason })!);
    pairPrev = result.preview;
    previewName = name.trim() || result.preview.name;
    const p = result.preview;
    const bundle = { server_id: p.serverId, seq: p.seq, expires: 0, published: p.published, fingerprint: p.fingerprint, name: p.name, candidates: p.candidates } as Bundle;
    const [{ openRtc }, { openWs }] = await Promise.all([import('./lib/rtc'), import('./lib/ws')]);
    const paths = orderPaths(buildPaths({ bundle }));
    const r = await connectPaths(paths, (path, sig) => (isTunnel(path) ? openWs(path.url!, 8000, sig) : openRtc({ ip: path.addr, udp_port: path.port, fingerprint: path.fingerprint! }, 10000, sig)), dispatch, { signal });
    if (signal.aborted) return;
    if (r.won) {
      r.won.value.close();
      pathLabel = r.won.path.label;
      reach = r.fallback ? { state: 'degraded', port: p.candidates.find((c) => c.kind !== 'tunnel')?.port ?? null } : { state: 'reachable' };
      dispatch({ type: 'found' });
      dispatch({ type: 'previewed' });
    } else {
      reach = { state: 'unreachable', port: null, message: r.diagnosis?.title ?? '' };
    }
  }

  function confirm() {
    if (pairPrev) {
      onadd(pairedServer(pairPrev, previewName.trim() || pairPrev.name));
      dispatch({ type: 'confirmed' });
      return;
    }
    if (!found) return;
    const record = found;
    onadd({ server_id: record.server_id, server_name: (claimPreview ? previewName : name).trim(), server_url: '', record, registered_at: Date.now(), claim: secret ?? undefined });
    dispatch({ type: 'confirmed' });
    // The server stays added whatever the check says; it only advises (GUARDRAIL connections-warn-never-drop).
    checking = true;
    void checkReachable({ record }).then((r) => { reach = r; checking = false; });
  }

  /** The preview could not be read, but the server announced itself: add it, and let the reachability check advise. */
  function addWithoutPreview() {
    dispatch({ type: 'previewed' });
    confirm();
  }

  /** Added although no address answered: the server stays in the list and keeps retrying. */
  function addAnyway() {
    if (!pairPrev) return;
    onadd(pairedServer(pairPrev, previewName.trim() || pairPrev.name));
    dispatch({ type: 'previewed' });
    dispatch({ type: 'confirmed' });
  }

  function gotText(text: string) {
    const known = parseServerLink(text);
    if (known) {
      stopScan();
      onserver(known);
      return;
    }
    stopScan();
    if (parsePairLink(text)) return void handlePair(text);
    const sec = parseClaim(text);
    if (!sec) {
      fail(DIAGNOSES.bad_link);
      return;
    }
    ensureConnect();
    void begin(sec);
  }

  function onAction(id: ActionId) {
    switch (id) {
      case 'retry':
        if (found && !claimPreview && secret) void showPreview(found, secret);
        else if (pairPrev) void handlePair(`#/pair/${pairPrev.payload}${pairPrev.claim ? `?claim=${pairPrev.claim}` : ''}`);
        else if (secret) void begin(secret);
        break;
      case 'rescan':
        dispatch({ type: 'looking' });
        void startScan();
        break;
      case 'rerun_installer':
        secret = null;
        found = claimPreview = pairPrev = null;
        codeRead = false;
        dispatch({ type: 'reset' });
        break;
      case 'edit_code':
        document.getElementById('have-link')?.scrollIntoView({ block: 'center' });
        document.getElementById('have-link')?.focus();
        break;
      case 'open_settings':
        void handleSettings();
        break;
      case 'copy_firewall':
        break;
    }
  }

  /** Browsers give no way to open the permission page; the cause's detail says where it is, so this only retries. */
  async function handleSettings() {
    if (pairPrev) void handlePair(`#/pair/${pairPrev.payload}${pairPrev.claim ? `?claim=${pairPrev.claim}` : ''}`);
  }

  async function startScan() {
    error = '';
    scanning = true;
    await tick();
    if (!video) return;
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
  let manualUrl = '';
  let manualClaim = '';

  function goConnect() {
    dispatch({ type: 'next' });
    if (codeRead && !found) dispatch({ type: 'looking' });
  }

  onMount(() => {
    // The command carries no secret, so there is nothing to wait for until a link, scan or pasted secret arrives.
    if (start === 'scan') void startScan();
    else if (secret) void begin(secret);
  });
  onDestroy(() => {
    abort?.abort();
    stopScan();
  });
</script>

<Dialog title="Add Server" {onclose} class="p-4">
  {#if start === 'install'}<StepIndicator step={s.step} />{/if}

  {#if s.step === 'install'}
    <InstallStep {command} {allowed} bind:name bind:where bind:hostAs bind:consent onnext={goConnect} />
  {:else if s.step === 'connect'}
    <ConnectStep connect={s.connect} {codeRead} {scanning} bind:video {error} onscan={startScan} onstopscan={stopScan} onfile={onFile} ontext={gotText} onaction={onAction}>
      <svelte:fragment slot="detail">
        {#if blocked && found}<PortHelp port={found.udp_port ?? null} />{/if}
        {#if pairPrev && s.connect.diagnosis}
          <button class="secondary" on:click={addAnyway} style="width: 100%; margin-top: 8px;">Add it anyway</button>
        {/if}
        {#if found && !claimPreview}
          <button class="secondary" on:click={addWithoutPreview} style="width: 100%; margin-top: 8px;">Add without a preview</button>
        {/if}
      </svelte:fragment>
    </ConnectStep>
    <details style="margin-top: 12px;">
      <summary style="cursor: pointer; color: var(--fg-muted); font-size: 0.8rem;">Advanced: connect by URL (tunnel or reverse proxy)</summary>
      <input bind:value={manualUrl} placeholder="https://xxxx.trycloudflare.com" style="width: 100%; margin: 8px 0;" />
      <input bind:value={manualClaim} placeholder="Registration token (first connection only)" style="width: 100%; margin-bottom: 8px;" />
      <button class="secondary" on:click={addByUrl} disabled={!manualUrl} style="width: 100%;">Add and connect</button>
    </details>
  {:else if s.step === 'confirm'}
    <ConfirmStep claim={claimPreview} pair={pairPrev} bind:name={previewName} onconfirm={confirm} />
  {:else}
    <DoneStep {checking} {reach} {pathLabel} ondone={onclose} />
  {/if}
</Dialog>
