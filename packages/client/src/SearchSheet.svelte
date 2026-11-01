<script lang="ts">
  import { Sheet } from './lib/ui/sheet';
  import AgentIcon from './AgentIcon.svelte';
  import { NOTICE, landingOf, cutText, debounced, groupHits, GROUP_MODES, RANGES, olderText, loadGroupMode, saveGroupMode, offText, parts, whenText, type GroupMode, type RangeId, type Hit, type SearchState } from './lib/sessionSearch';

  /**
   * Find text in the agents' own conversations (R78, decision 0156). The sheet shows what `search` holds and tells its owner
   * what was typed; it never keeps the query once it closes. A tap on a hit is handed to `onpick`.
   */
  let { open = $bindable(false), wide = false, search, now = Date.now(), nameOf = () => undefined, onquery, onrange = () => {}, onstop, onpick, onresume, onclose }: {
    open?: boolean; wide?: boolean; search: SearchState; now?: number; nameOf?: (serverId: string, workspace: string) => string | undefined;
    onquery: (text: string) => void; onrange?: (range: RangeId) => void; onstop: () => void; onpick: (hit: Hit) => void; onresume: (hit: Hit) => void; onclose?: () => void;
  } = $props();

  let text = $state('');
  // A hit in a dormant workspace asks first: Resume starts the agent again, so it is its own step, never part of the tap.
  let resuming = $state<Hit | null>(null);
  const tap = (h: Hit) => { if (landingOf(h) === 'resume') resuming = h; else onpick(h); };
  const typed = debounced((t) => onquery(t));
  let mode = $state<GroupMode>(loadGroupMode());
  const pickMode = (m: GroupMode) => { mode = m; saveGroupMode(m); };
  const groups = $derived(groupHits(search.hits, mode, nameOf));
  const where = (h: Hit) => nameOf(h.serverId, h.workspace) ?? h.workspace;
  const cuts = $derived(search.cuts);
  const searching = $derived(search.status === 'searching');
  const empty = $derived(search.status === 'done' && search.hits.length === 0 && search.query !== '');
  const first = $derived(search.status === 'idle');

  $effect(() => { if (!open) { typed.cancel(); text = ''; resuming = null; } });
  function input(e: Event) { text = (e.currentTarget as HTMLInputElement).value; if (text.trim()) typed.push(text); else { typed.cancel(); onquery(''); } }
  function key(e: KeyboardEvent) { if (e.key === 'Enter' && !e.isComposing) { e.preventDefault(); typed.flush(); } }
  function close() { typed.cancel(); onclose?.(); }
</script>

<Sheet bind:open title="Search conversations" side={wide ? 'right' : 'bottom'} class="min-h-[60%]" onclose={close}>
  <div class="flex flex-col gap-3" data-testid="session-search">
    <input type="search" value={text} oninput={input} onkeydown={key} data-autofocus data-testid="search-input" aria-label="Search the agents’ conversations" placeholder="Words from a conversation"
      autocomplete="off" autocapitalize="off" spellcheck="false" enterkeyhint="search" maxlength="512"
      class="h-11 w-full rounded-control border border-line bg-bg px-3 text-input text-fg outline-none placeholder:text-muted focus:border-accent" />
    <p class="text-xs text-muted" data-testid="search-notice">{NOTICE}</p>

    <div role="status" aria-live="polite" class="min-h-5 text-sm text-soft" data-testid="search-status">
      {#if searching}
        Searching… {search.hits.length ? `${search.hits.length} found` : ''}
        <button type="button" onclick={onstop} data-testid="search-stop" class="ml-2 h-11 rounded-control bg-transparent px-2 text-sm font-semibold text-accent-text">Stop</button>
      {:else if empty}No conversation matches “{search.query}”.
      {:else if first}Type to search every conversation this server has a record of, open or not.
      {:else}{search.hits.length} {search.hits.length === 1 ? 'match' : 'matches'}{/if}
    </div>

    {#if resuming}
      <div class="flex flex-col gap-2 rounded-card border border-line bg-raised p-3" role="alertdialog" aria-label="Resume" data-testid="search-resume">
        <p class="text-sm text-fg">This workspace is not running. Resume opens it and starts {resuming.agent} again with this conversation.</p>
        <div class="flex gap-2">
          <button type="button" data-testid="search-resume-go" onclick={() => { const h = resuming; resuming = null; if (h) onresume(h); }} class="h-11 flex-1 rounded-control bg-accent px-4 text-base font-semibold text-on-accent">Resume</button>
          <button type="button" data-testid="search-resume-cancel" onclick={() => (resuming = null)} class="h-11 rounded-control bg-transparent px-4 text-base font-semibold text-soft">Not now</button>
        </div>
      </div>
    {/if}

    <div role="group" aria-label="Search conversations from the last" data-testid="search-range" class="flex w-full gap-1 rounded-control bg-raised p-1">
      {#each RANGES as r (r.id)}
        <button type="button" aria-pressed={search.range === r.id} data-testid="search-range-{r.id}" onclick={() => onrange(r.id)}
          class="h-11 min-w-0 flex-1 truncate rounded-control bg-transparent px-2 text-sm font-semibold text-soft aria-pressed:bg-accent aria-pressed:text-on-accent">{r.label}</button>
      {/each}
    </div>

    <div role="group" aria-label="Group matches by" data-testid="search-grouping" class="flex w-full gap-1 rounded-control bg-raised p-1">
      {#each GROUP_MODES as m (m.mode)}
        <button type="button" aria-pressed={mode === m.mode} data-testid="search-group-{m.mode}" onclick={() => pickMode(m.mode)}
          class="h-11 min-w-0 flex-1 truncate rounded-control bg-transparent px-2 text-sm font-semibold text-soft aria-pressed:bg-accent aria-pressed:text-on-accent">{m.label}</button>
      {/each}
    </div>

    {#each search.cuts as c (c)}<p class="rounded-control bg-raised px-3 py-2 text-sm text-soft" data-testid="search-cut">{c}</p>{/each}
    {#if search.older > 0 && search.range !== 'any'}
      <p class="flex items-center gap-2 rounded-control bg-raised px-3 py-2 text-sm text-soft" data-testid="search-older">
        <span class="min-w-0 flex-1">{olderText(search.older)}</span>
        <button type="button" data-testid="search-older-go" onclick={() => onrange('any')} class="h-11 shrink-0 rounded-control bg-transparent px-2 text-sm font-semibold text-accent-text">Search older</button>
      </p>
    {/if}
    {#each search.off as o (o.agent + o.reason)}<p class="rounded-control bg-raised px-3 py-2 text-sm text-soft" data-testid="search-off">{offText(o)}</p>{/each}
    {#if search.lost}<p class="rounded-control bg-raised px-3 py-2 text-sm text-soft" data-testid="search-lost">{search.lost === 1 ? 'A server' : `${search.lost} servers`} went away before it finished, so its matches may be missing.</p>{/if}

    {#each groups as g (g.key)}
      <section aria-label={g.name} data-testid="search-group" data-workspace={g.workspace} data-mode={mode} class="flex flex-col">
        {#if mode !== 'none'}
        <h4 class="flex h-11 items-center gap-2 text-sm font-semibold text-fg">
          <AgentIcon agent={g.agent} size={20} />
          <span class="min-w-0 flex-1 truncate">{g.name}</span>
          <span class="shrink-0 text-xs font-normal text-muted">{g.serverName}</span>
        </h4>
        {/if}
        {#each g.hits as h (h.serverId + h.workspace + h.turn + h.tab)}
          {@const p = parts(h)}
          <button type="button" data-testid="search-hit" data-turn={h.turn} onclick={() => tap(h)}
            class="flex min-h-11 w-full flex-col gap-1 rounded-control bg-transparent px-2 py-2 text-left">
            <span class="flex items-center gap-2 text-xs text-muted">
              <span class={h.up ? 'text-accent-text' : 'text-warn'} data-testid="search-badge">{h.up ? 'Open' : 'Resume'}</span>
              {#if mode !== 'workspace'}<span class="truncate" data-testid="search-where">{where(h)}{h.tab ? ` · ${h.tab}` : ''}</span>
              {:else if h.tab}<span class="truncate">{h.tab}</span>{/if}
              <span class="ml-auto shrink-0">{whenText(h.at, now)}</span>
            </span>
            <span class="text-sm break-words text-fg">{p.before}<mark class="rounded-sm bg-accent/30 text-fg" data-hit>{p.match}</mark>{p.after}</span>
          </button>
        {/each}
      </section>
    {/each}
  </div>
</Sheet>
