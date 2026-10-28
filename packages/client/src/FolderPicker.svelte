<script lang="ts">
  import { onDestroy } from 'svelte';
  import DialogLayer from './lib/ui/sheet/DialogLayer.svelte';
  import { FolderBrowser, type BrowserState } from './lib/folderBrowser';
  import type { ListDir } from './lib/dirClient';
  import { breadcrumb, displayPath, parentOf } from './lib/dirs';

  /** The only place a path is typed: browse the server's folders under its home, or type an absolute path and Go. */
  let {
    open = $bindable(false),
    list,
    value = '',
    onselect,
    onclose,
  }: { open?: boolean; list: ListDir; value?: string; onselect: (path: string) => void; onclose?: () => void } = $props();

  let view: BrowserState = $state({ status: 'loading', path: '', home: '', entries: [], truncated: false, error: '', filter: '', hidden: false, outside: false });
  const browser = new FolderBrowser(req => list(req), s => (view = s));
  let typed = $state('');
  let typedError = $state('');
  let slow = $state(false); // skeleton rows only after 150 ms, so a quick answer does not flash
  let slowTimer: ReturnType<typeof setTimeout> | undefined;
  let filterTimer: ReturnType<typeof setTimeout> | undefined;
  let rowsEl: HTMLElement | undefined = $state();

  // Start where the field already points (it may be outside home: then the path bar just shows it), else at home.
  let started = false;
  $effect(() => {
    if (!open) { started = false; return; }
    if (started) return;
    started = true;
    typedError = '';
    void browser.open(value && value.startsWith('/') ? value : undefined).then(() => { if (browser.state.status === 'error' && value) void browser.toHome(); });
  });
  $effect(() => { typed = view.path; });
  $effect(() => {
    clearTimeout(slowTimer);
    slow = false;
    if (view.status === 'loading') slowTimer = setTimeout(() => (slow = true), 150);
  });
  onDestroy(() => { clearTimeout(slowTimer); clearTimeout(filterTimer); });

  const trail = $derived(view.home ? breadcrumb(view.path, view.home) : []);
  const canUp = $derived(!!view.path && parentOf(view.path) !== null);
  const selection = $derived(browser.selection ?? (view.status === 'ready' && view.path ? view.path : null));

  function go() { typedError = browser.go(typed) ?? ''; }
  function choose() {
    const p = browser.selection;
    if (!p) return;
    onselect(p);
    open = false;
  }
  function onfilter(e: Event) {
    const v = (e.currentTarget as HTMLInputElement).value;
    clearTimeout(filterTimer);
    filterTimer = setTimeout(() => void browser.setFilter(v), 200);
  }
  function rows(): HTMLElement[] { return rowsEl ? [...rowsEl.querySelectorAll<HTMLElement>('[data-dir]')] : []; }
  function onrowkeys(e: KeyboardEvent) {
    const all = rows();
    const at = all.indexOf(document.activeElement as HTMLElement);
    if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
      e.preventDefault();
      all[Math.max(0, Math.min(all.length - 1, at + (e.key === 'ArrowDown' ? 1 : -1)))]?.focus();
    } else if (e.key === 'Backspace' && canUp) {
      e.preventDefault();
      void browser.up();
    }
  }
</script>

<DialogLayer bind:open title="Choose folder" {onclose}>
  <div data-testid="folder-picker">
    <form class="mb-2 flex gap-2" onsubmit={(e) => { e.preventDefault(); go(); }}>
      <input
        class="min-h-12 min-w-0 flex-1 rounded-control border border-line bg-sunken px-3 text-base text-fg outline-none focus:border-accent focus:ring-2 focus:ring-accent/40"
        bind:value={typed}
        aria-label="Path"
        data-testid="picker-path"
        inputmode="url"
        autocapitalize="off"
        autocomplete="off"
        spellcheck="false"
        aria-invalid={typedError ? 'true' : undefined}
        aria-describedby={typedError ? 'picker-path-error' : undefined}
      />
      <button type="submit" class="act" data-testid="picker-go">Go</button>
    </form>
    {#if typedError}<p id="picker-path-error" class="mb-2 text-sm text-danger" role="alert" data-testid="picker-path-error">{typedError}</p>{/if}

    <nav aria-label="Path" class="trail mb-2" data-testid="picker-trail">
      {#each trail as part, i (part.path)}
        {#if i > 0}<span aria-hidden="true" class="text-soft">/</span>{/if}
        <button type="button" class="crumb" onclick={() => browser.open(part.path)} aria-current={i === trail.length - 1 ? 'location' : undefined}>{part.name}</button>
      {/each}
    </nav>

    <div class="mb-2 flex gap-2">
      <button type="button" class="act" data-testid="picker-up" disabled={!canUp} onclick={() => browser.up()}>Up</button>
      <button type="button" class="act" data-testid="picker-home" onclick={() => browser.toHome()}>Home</button>
      <input
        type="search"
        class="min-h-12 min-w-0 flex-1 rounded-control border border-line bg-sunken px-3 text-base text-fg outline-none focus:border-accent focus:ring-2 focus:ring-accent/40"
        placeholder="Filter folders…"
        aria-label="Filter folders"
        data-testid="picker-filter"
        autocomplete="off"
        value={view.filter}
        oninput={onfilter}
      />
    </div>
    <label class="mb-3 flex min-h-11 items-center gap-2 text-sm text-soft">
      <input type="checkbox" checked={view.hidden} onchange={(e) => browser.setHidden(e.currentTarget.checked)} data-testid="picker-hidden" /> Show hidden
    </label>

    <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
    <div bind:this={rowsEl} onkeydown={onrowkeys} role="group" aria-label="Folders" aria-busy={view.status === 'loading'}>
      {#if view.status === 'loading' && !view.entries.length}
        {#if slow}
          <div class="grid gap-2" data-testid="picker-loading" aria-hidden="true">{#each [0, 1, 2, 3] as n (n)}<div class="skeleton"></div>{/each}</div>
        {/if}
      {:else if view.status === 'error'}
        <div class="py-6 text-center" role="alert" data-testid="picker-error">
          <p class="mb-3 text-danger">{view.error}</p>
          <button type="button" class="act" data-testid="picker-retry" onclick={() => browser.retry()}>Retry</button>
        </div>
      {:else if view.outside}
        <p class="py-6 text-center text-soft" data-testid="picker-outside">This folder is outside your home folder and cannot be browsed. It will be used as typed.</p>
      {:else if view.entries.length === 0}
        <p class="py-6 text-center text-soft" data-testid="picker-empty">{view.filter ? 'No folders match' : 'No folders here'}</p>
      {:else}
        <ul class="grid gap-2" data-testid="picker-list">
          {#each view.entries as e (e.name)}
            <li>
              <button type="button" class="row" data-dir={e.name} onclick={() => browser.enter(e.name)}>
                <svg viewBox="0 0 24 24" width="20" height="20" fill="currentColor" aria-hidden="true" class="flex-none text-soft"><path d="M10 4H4a2 2 0 0 0-2 2v12a2 2 0 0 0 2 2h16a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-8l-2-2z"/></svg>
                <span class="min-w-0 flex-1 truncate">{e.name}</span>
                {#if e.symlink}<small class="text-soft">link</small>{/if}
              </button>
            </li>
          {/each}
        </ul>
        {#if view.truncated}<p class="mt-3 text-sm text-soft" data-testid="picker-truncated">Showing the first 500, filter to narrow.</p>{/if}
      {/if}
    </div>
  </div>

  {#snippet footer()}
    <button type="button" class="select" data-testid="picker-select" disabled={!selection || view.status !== 'ready'} onclick={choose}>
      <span class="label">Select this folder</span>
      <span class="path" title={selection ?? ''}>{selection ? displayPath(selection, view.home, 40) : ''}</span>
    </button>
  {/snippet}
</DialogLayer>

<style>
  .act { flex: none; min-height: 48px; min-width: 48px; padding: 0 14px; background: var(--bg); color: var(--fg); border: 1px solid var(--border); border-radius: 8px; }
  .act:disabled { opacity: 0.5; }
  .trail { display: flex; flex-wrap: wrap; align-items: center; gap: 2px; }
  .crumb { min-height: 44px; min-width: 44px; padding: 0 8px; background: transparent; border: none; color: var(--accent); max-width: 40vw; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .crumb[aria-current] { color: var(--fg); font-weight: 600; }
  .row { display: flex; align-items: center; gap: 12px; width: 100%; min-height: 48px; padding: 0 12px; text-align: left; background: var(--bg); color: var(--fg); border: 1px solid var(--border); border-radius: 8px; }
  .skeleton { height: 48px; border-radius: 8px; background: var(--border); opacity: 0.4; animation: pulse 1s ease-in-out infinite alternate; }
  @keyframes pulse { to { opacity: 0.15; } }
  @media (prefers-reduced-motion: reduce) { .skeleton { animation: none; } }
  .select { display: flex; flex-direction: column; align-items: center; width: 100%; min-height: 48px; padding: 6px 12px; background: var(--accent); color: var(--bg); border: none; border-radius: 8px; font-weight: 600; }
  .select:disabled { opacity: 0.5; }
  .select .path { font-size: 0.8rem; font-weight: 400; max-width: 100%; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
</style>
