<script lang="ts">
  import { onMount, tick, untrack } from 'svelte';
  import { AgentBadge, EmptyState, StatusChip, type FleetState } from './lib/ds';
  import { NativeSelect } from './lib/ui/native-select';
  import AcpEntryView from './AcpEntryView.svelte';
  import AcpPrompt from './AcpPrompt.svelte';
  import { contextPercent, isNewEntry, OPEN_WAIT_MS, openingText, search, toolsAsking, transcriptText, type StreamState } from './lib/acpStream';
  import { promptsOf } from './lib/acpPrompt';
  import { loadSaved, placeFor, save, type Place } from './lib/acpStore';
  import { join } from './lib/acpHistory';
  import type { SessionWindowMessage } from './lib/protocol';
  import { copyText } from './lib/clipboard';
  import { Settle } from './lib/home';
  import { TIER_LABEL } from './lib/sessionView';

  /** The ACP workspace view (T0, decision 0111; U07b): a terminal-style stream we draw ourselves, because an ACP agent
   *  has no TUI. It is plain text in the page rather than a canvas, because that is what a phone can scroll, select and
   *  copy (see docs/plans/roadmap/U07b.md). What moves, and when (guardrail ui-never-moves-under-the-finger): the stream
   *  holds still while a finger is down and for 2 s after a scroll, and applies what arrived after that; it follows the
   *  end only while the reader is at the end; and nothing above the reader's place changes their place. */
  let { workspaceKey, name, agent, fleet = 'idle', stream, connState = 'connected', now = Date.now(), canShell = false,
    onprompt, oninterrupt, onanswer, onmode, onshell, focus = null, window: fileWindow, changed = false, onwindow }: {
    workspaceKey: string; name: string; agent?: string; fleet?: FleetState; stream?: StreamState; connState?: string; now?: number; canShell?: boolean;
    /** R78: a search hit to land on. `query` is the matched text, `nonce` tells one tap from the next. The view asks for the turn with `onwindow` and shows what comes back as `window`. */
    focus?: { turn: string; query: string; nonce: number } | null; window?: SessionWindowMessage; changed?: boolean; onwindow?: (turn: string) => void;
    onprompt: (text: string) => void; oninterrupt: () => void; onanswer: (promptId: string, optionId: string) => void; onmode: (mode: string) => void; onshell: () => void;
  } = $props();

  // The view is made per workspace (App keys it), so the key is read once.
  const key = untrack(() => workspaceKey);
  const saved = loadSaved(key);
  let draft = $state(saved.draft ?? '');
  let opened = $state(new Set<string>(saved.opened ?? []));
  const toggleOpen = (k: string) => {
    const next = new Set(opened);
    if (!next.delete(k)) next.add(k);
    opened = next;
    save(key, { opened: [...next].slice(-100) });
  };

  const info = $derived(stream?.info);
  const entries = $derived(stream?.entries ?? []);
  const connected = $derived(connState === 'connected');
  const ended = $derived(info?.status === 'ended' || info?.status === 'failed');
  const blocked = $derived(!connected ? 'Reconnecting. Draft kept.' : info?.status === 'failed' ? 'The agent stopped.' : info?.status === 'ended' ? 'This workspace has ended.' : '');
  const tier = TIER_LABEL.t0;

  // The first full picture (the facts arrive last): what is in it is history, so an old error is not read out as an
  // alert; only what arrives after it is. A stream held from before starts out with its picture.
  let baseRev = $state<number | undefined>(untrack(() => (stream?.info ? stream.rev : undefined)));
  $effect(() => { if (baseRev === undefined && stream?.info) baseRev = stream.rev; });

  // A server older than this view ignores `acp_open`, so nothing ever comes back: say so after a while.
  let silent = $state(false);
  $effect(() => {
    silent = false;
    if (stream || !connected) return;
    const t = setTimeout(() => (silent = true), OPEN_WAIT_MS);
    return () => clearTimeout(t);
  });

  // ---- Holding still: the stream shown is the latest one unless a finger is down or the reader just scrolled.
  const settle = new Settle<typeof entries>();
  let beat = $state(0);
  const shown = $derived.by(() => { beat; return settle.view(entries, Date.now()); });
  const held = $derived(shown !== entries);

  // ---- Older turns from the agent's own file (R78). They sit above the live list and never enter the reducer; the live
  // list wins where the two overlap (lib/acpHistory.ts).
  const joined = $derived(fileWindow ? join(fileWindow, entries) : undefined);
  const history = $derived(joined?.history ?? []);
  const hitId = $derived(joined?.anchor?.entry.id ?? joined?.live?.id);
  let hitQuery = $state('');
  let landing = $state(false); // between the tap and the scroll: nothing else may move the view
  let missing = $state(false);
  let asked = -1;
  $effect(() => {
    const f = focus;
    if (!f || f.nonce === asked) return;
    asked = f.nonce;
    hitQuery = f.query;
    missing = false;
    landing = true;
    onwindow?.(f.turn);
    const t = setTimeout(() => { if (landing) { landing = false; missing = true; } }, 5000); // no answer: do not hold the view
    return () => clearTimeout(t);
  });
  $effect(() => {
    const w = fileWindow;
    const f = focus;
    if (!landing || !w || !f || w.turn !== f.turn) return;
    const id = untrack(() => hitId);
    void tick().then(() => tick()).then(() => {
      landing = false;
      const el = scroller;
      const target = id === undefined ? null : el?.querySelector<HTMLElement>(`[data-entry="${id}"]`);
      if (!el || !target) { missing = true; return; }
      following = false;
      const box = el.getBoundingClientRect();
      setTop(el.scrollTop + target.getBoundingClientRect().top - box.top - box.height / 3);
    });
  });
  onMount(() => { const t = setInterval(() => beat++, 400); return () => clearInterval(t); });

  // ---- Scrolling: follow the end while the reader is there, keep the place otherwise.
  let scroller = $state<HTMLDivElement | null>(null);
  let following = $state(saved.place?.bottom ?? true);
  let lastSet = -1; // scrollTop we set ourselves: the scroll event it causes is not the reader's
  const NEAR_END = 48;
  const atEnd = (el: HTMLElement) => el.scrollHeight - el.scrollTop - el.clientHeight < NEAR_END;
  function setTop(top: number) {
    if (!scroller) return;
    lastSet = Math.max(0, top);
    scroller.scrollTop = top;
    lastSet = scroller.scrollTop;
  }
  let saveTimer: ReturnType<typeof setTimeout> | undefined;
  function placeOf(): Place | undefined {
    const el = scroller;
    if (!el) return undefined;
    if (atEnd(el)) return { entry: 0, offset: 0, bottom: true, epoch: stream?.epoch };
    const box = el.getBoundingClientRect();
    for (const row of el.querySelectorAll<HTMLElement>('[data-entry]')) {
      const r = row.getBoundingClientRect();
      if (r.bottom > box.top + 1) return { entry: Number(row.dataset.entry), offset: r.top - box.top, bottom: false, epoch: stream?.epoch };
    }
    return undefined;
  }
  // The place is worked out while the view is on screen (not at teardown, when the layout may already be gone) and
  // saved a moment after scrolling stops; leaving the view saves the last one at once.
  let lastPlace: Place | undefined;
  let raf = 0;
  const remember = () => {
    cancelAnimationFrame(raf);
    raf = requestAnimationFrame(() => { lastPlace = placeOf() ?? lastPlace; });
    clearTimeout(saveTimer);
    saveTimer = setTimeout(() => { if (lastPlace) save(key, { place: lastPlace }); }, 300);
  };
  function onscroll() {
    const el = scroller;
    if (!el) return;
    if (Math.abs(el.scrollTop - lastSet) > 1) settle.scrolled(Date.now()); // the reader's own scroll
    lastSet = -1;
    following = atEnd(el);
    remember();
  }
  function restore(saved: Place | undefined) {
    const el = scroller;
    if (!el) return;
    const place = placeFor(saved, stream?.epoch ?? 0);
    if (!place || place.bottom) return setTop(el.scrollHeight);
    const row = el.querySelector<HTMLElement>(`[data-entry="${place.entry}"]`);
    if (!row) return setTop(el.scrollHeight); // that part is no longer kept
    setTop(el.scrollTop + row.getBoundingClientRect().top - el.getBoundingClientRect().top - place.offset);
  }
  let anchor: Place | undefined;
  $effect.pre(() => { shown; history; if (scroller && !following) anchor = placeOf(); else anchor = undefined; });
  let first = true;
  $effect(() => {
    shown; history; scroller;
    if (!scroller) return;
    const a = anchor;
    void tick().then(() => {
      if (landing) return; // the tap's own scroll follows
      if (first && entries.length) { first = false; restore(saved.place); return; }
      if (following) setTop(scroller!.scrollHeight);
      else if (a) restore(a);
    });
  });
  onMount(() => () => { clearTimeout(saveTimer); cancelAnimationFrame(raf); if (lastPlace) save(key, { place: lastPlace }); });
  // A finger that lifts anywhere releases the hold; and while the reader is at the end, the end stays in view when the
  // keyboard opens or the window changes size.
  onMount(() => {
    const up = (e: PointerEvent) => settle.touchUp(e.pointerId);
    window.addEventListener('pointerup', up, true);
    window.addEventListener('pointercancel', up, true);
    return () => { window.removeEventListener('pointerup', up, true); window.removeEventListener('pointercancel', up, true); };
  });
  $effect(() => {
    const el = scroller;
    if (!el || typeof ResizeObserver === 'undefined') return;
    const ro = new ResizeObserver(() => { if (following) setTop(el.scrollHeight); });
    ro.observe(el);
    return () => ro.disconnect();
  });
  const toEnd = () => { hitQuery = ''; following = true; settle.scrolled(-Infinity); beat++; void tick().then(() => setTop(scroller!.scrollHeight)); };

  // ---- Search through everything held
  let searching = $state(false);
  let query = $state('');
  let at = $state(0);
  let searchBox = $state<HTMLInputElement | null>(null);
  const matches = $derived(searching ? search(shown, query) : []);
  $effect(() => { query; at = 0; });
  const cur = $derived(matches[Math.min(at, Math.max(0, matches.length - 1))]);
  // Jump to a hit when it is chosen, not each time the stream grows underneath it.
  const jump = $derived(cur ? `${cur.entry}:${cur.index}:${query}` : '');
  $effect(() => {
    if (!jump) return;
    const c = untrack(() => cur);
    void tick().then(() => {
      const el = scroller;
      if (!el || !c) return;
      const target = el.querySelector<HTMLElement>('[data-hit][data-current]') ?? el.querySelector<HTMLElement>(`[data-entry="${c.entry}"]`);
      if (!target) return;
      following = false;
      const box = el.getBoundingClientRect();
      setTop(el.scrollTop + target.getBoundingClientRect().top - box.top - box.height / 3);
    });
  });
  const step = (by: 1 | -1) => { if (matches.length) at = (at + by + matches.length) % matches.length; };
  function openSearch() { searching = true; void tick().then(() => searchBox?.focus()); }
  function closeSearch() { searching = false; query = ''; }
  function searchKey(e: KeyboardEvent) {
    if (e.key === 'Enter') { e.preventDefault(); step(e.shiftKey ? -1 : 1); }
    else if (e.key === 'Escape') { e.preventDefault(); closeSearch(); }
  }

  // ---- Copy
  let copied = $state(false);
  async function copyAll() {
    copied = await copyText(transcriptText(entries));
    setTimeout(() => (copied = false), 1500);
  }

  // ---- Prompt
  let draftTimer: ReturnType<typeof setTimeout> | undefined;
  const remember_draft = (text: string) => { clearTimeout(draftTimer); draftTimer = setTimeout(() => save(key, { draft: text }), 400); };
  onMount(() => () => { clearTimeout(draftTimer); save(key, { draft }); });
  const send = (text: string) => { onprompt(text); draft = ''; save(key, { draft: '' }); toEnd(); };

  // Esc anywhere in the view interrupts a turn that is running, as it does in the field.
  function escape(e: KeyboardEvent) {
    if (e.key !== 'Escape' || e.defaultPrevented) return;
    if (searching) return closeSearch();
    if (info?.working && connected) { e.preventDefault(); oninterrupt(); }
  }
  onMount(() => {
    const on = (e: KeyboardEvent) => { if (document.activeElement === document.body) escape(e); };
    window.addEventListener('keydown', on);
    return () => window.removeEventListener('keydown', on);
  });

  // ---- What a screen reader hears: the turn's edges and a question, not every token.
  let heard = $state('');
  let wasWorking = false;
  $effect(() => {
    const w = !!info?.working;
    if (w !== wasWorking) heard = w ? 'The agent is working.' : 'The agent has finished.';
    wasWorking = w;
  });
  const asking = $derived(toolsAsking(shown));
  const open = $derived(shown.filter((e) => e.kind === 'permission' && e.state === 'open').length);
  $effect(() => { if (open) heard = 'The agent asks for permission.'; });

  const pct = $derived(contextPercent(info?.usage));
  const subtitle = $derived(!connected ? 'Reconnecting. What you see is kept.' : info?.status === 'failed' ? `Stopped${info.statusText ? `: ${info.statusText}` : ''}` : info?.status === 'ended' ? 'Ended' : info?.title || info?.dir || 'Opening…');
</script>

<div class="flex h-full min-h-0 flex-col bg-bg text-fg" data-testid="acp-view" data-workspace={workspaceKey} onkeydown={escape} role="presentation">
  <header class="flex items-center gap-3 border-b border-line px-4 py-3">
    <AgentBadge agent={info?.agent || agent} state={fleet} />
    <div class="flex min-w-0 flex-1 flex-col">
      <b class="truncate text-base font-semibold">{name}</b>
      <span class="truncate text-sm text-soft" title={info?.dir} data-testid="subtitle">{subtitle}</span>
    </div>
    <span class="shrink-0 rounded-full bg-raised px-2.5 py-1 text-xs font-semibold text-soft" data-testid="tier" title={tier.detail}>{tier.name}</span>
    <StatusChip state={fleet} />
  </header>

  <div class="flex items-center gap-2 border-b border-line px-4 py-2" role="toolbar" aria-label="Workspace tools">
    {#if info && info.modes.length > 1}
      <div class="min-w-0 flex-1">
        <NativeSelect aria-label="Mode" value={info.mode} disabled={!connected || ended} onchange={(e) => onmode((e.currentTarget as HTMLSelectElement).value)} data-testid="mode">
          {#each info.modes as m (m.id)}<option value={m.id} title={m.description}>{m.name}</option>{/each}
        </NativeSelect>
      </div>
    {:else}<div class="flex-1"></div>{/if}
    {#if pct !== null}
      <span class="shrink-0 text-xs {pct >= 80 ? 'font-semibold text-warn' : 'text-soft'}" title="Context window in use{info?.usage?.cost ? ` · cost ${info.usage.cost}` : ''}" data-testid="context">{pct}%<span class="sr-only"> of the context window used</span></span>
    {/if}
    <button type="button" aria-label="Search this workspace" aria-pressed={searching} onclick={() => (searching ? closeSearch() : openSearch())} onpointerdown={(e) => e.preventDefault()}
      class="grid size-11 shrink-0 place-items-center rounded-control bg-raised p-0 text-base text-fg">⌕</button>
    <button type="button" aria-label={copied ? 'Copied' : 'Copy the whole workspace'} onclick={copyAll} onpointerdown={(e) => e.preventDefault()} disabled={!entries.length}
      class="grid size-11 shrink-0 place-items-center rounded-control bg-raised p-0 text-base text-fg">{copied ? '✓' : '⧉'}</button>
    <button type="button" onclick={onshell} onpointerdown={(e) => e.preventDefault()} disabled={!canShell || !connected} data-testid="shell"
      title={canShell ? `Open a shell in ${info?.dir ?? 'the workspace’s directory'}` : 'This server runs no tmux, so there is no shell to open'}
      class="min-h-11 shrink-0 rounded-control bg-raised px-3 text-base font-semibold text-fg">Shell</button>
  </div>

  {#if searching}
    <div class="flex items-center gap-2 border-b border-line px-4 py-2" role="search">
      <input bind:this={searchBox} bind:value={query} type="search" enterkeyhint="search" placeholder="Search this workspace" aria-label="Search this workspace" autocapitalize="off" autocomplete="off" spellcheck="false" onkeydown={searchKey}
        class="h-11 min-w-0 flex-1 rounded-control border border-line bg-sunken px-3 text-input text-fg outline-none focus-visible:border-accent" />
      <span class="shrink-0 text-sm text-soft" role="status" data-testid="matches">{#if !query.trim()}{:else if matches.length}{Math.min(at, matches.length - 1) + 1} / {matches.length}{:else}No matches{/if}</span>
      <button type="button" aria-label="Previous match" disabled={!matches.length} onclick={() => step(-1)} onpointerdown={(e) => e.preventDefault()} class="grid size-11 shrink-0 place-items-center rounded-control bg-raised p-0 text-base text-fg">▲</button>
      <button type="button" aria-label="Next match" disabled={!matches.length} onclick={() => step(1)} onpointerdown={(e) => e.preventDefault()} class="grid size-11 shrink-0 place-items-center rounded-control bg-raised p-0 text-base text-fg">▼</button>
    </div>
  {/if}

  <div class="relative min-h-0 flex-1">
    <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
    <div bind:this={scroller} {onscroll} role="log" aria-live="off" aria-label="Workspace output" tabindex="0" data-testid="stream"
      onpointerdowncapture={(e) => settle.touchDown(e.pointerId)} onpointerupcapture={(e) => settle.touchUp(e.pointerId)} onpointercancelcapture={(e) => settle.touchUp(e.pointerId)}
      class="h-full overflow-y-auto overscroll-contain px-4 py-4 select-text [-webkit-touch-callout:default] [overflow-anchor:none]">
      <div class="mx-auto flex w-full max-w-3xl flex-col gap-3">
        {#if !stream}
          <p class="py-8 text-center text-base text-soft" role="status">{openingText(connected, silent)}</p>
        {:else if !shown.length}
          <EmptyState title={ended ? 'Nothing was said' : 'Say what you want done'} body={ended ? 'This workspace ended before anything was kept.' : 'Write a prompt below. Type / for the commands this agent offers.'} />
        {/if}
        {#if history.length}
          <p class="text-center text-xs text-muted" data-testid="history-note" role="note">Earlier in this conversation, read from the agent’s own file.</p>
          {#each history as h (h.entry.id)}
            <div data-entry={h.entry.id} data-turn={h.turn}>
              <AcpEntryView entry={h.entry} query={h.entry.id === hitId ? hitQuery : ''} current={h.entry.id === hitId ? 0 : -1} announce={false} {opened} ontoggle={toggleOpen}
                {onanswer} {workspaceKey} {name} agent={info?.agent || agent} {now} />
            </div>
          {/each}
          <hr class="border-line" aria-hidden="true" />
        {/if}
        {#if changed}<p class="rounded-control bg-raised px-3 py-2 text-sm text-soft" data-testid="history-changed" role="status">The conversation file changed since the search, so the place found may have moved. Search again to be sure.</p>{/if}
        {#if missing}<p class="rounded-control bg-raised px-3 py-2 text-sm text-soft" data-testid="history-missing" role="status">That turn could not be found in the conversation file.</p>{/if}
        {#each shown as e (e.id)}
          {@const m = searching && cur && cur.entry === e.id ? cur.index : !searching && e.id === hitId ? 0 : -1}
          <div data-entry={e.id}>
            <AcpEntryView entry={e} query={searching ? query.trim() : e.id === hitId ? hitQuery : ''} current={m} asking={e.kind === 'tool' && asking.has(e.toolId)} announce={isNewEntry(e, baseRev)} {opened} ontoggle={toggleOpen}
              {onanswer} {workspaceKey} {name} agent={info?.agent || agent} {now} />
          </div>
        {/each}
      </div>
    </div>
    {#if !following}
      <button type="button" onclick={toEnd} onpointerdown={(e) => e.preventDefault()} data-testid="latest"
        class="absolute right-4 bottom-3 min-h-11 rounded-full bg-accent px-4 text-base font-semibold text-on-accent shadow-sheet">{held ? 'New output ↓' : 'Latest ↓'}</button>
    {/if}
  </div>
  <p class="sr-only" role="status">{heard}</p>

  <AcpPrompt bind:value={draft} commands={info?.commands ?? []} working={!!info?.working} disabled={ended} {blocked} prompts={promptsOf(entries)} onsend={send} {oninterrupt} onchange={remember_draft} />
</div>
