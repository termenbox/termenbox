<script lang="ts">
  import { onMount } from 'svelte';
  import { EmptyState, FleetTile, QueueItem } from './lib/ds';
  import DecisionPanel from './DecisionPanel.svelte';
  import HookOfferCard from './HookOfferCard.svelte';
  import type { HookOffer } from './lib/hooks';
  import { decisionFor, type Decision } from './lib/decision';
  import type { FleetState } from './lib/ds/states';
  import type { AgentMap, WorkspaceAgent } from './lib/agents';
  import type { HubServer, HubWorkspace } from './lib/hub';
  import type { TabAgentState } from './lib/protocol';
  import { ageText } from './lib/cards';
  import { continueTarget, fleetCounts, queueOf, rowOf, Settle, splitQueue, type HomeQueue, type Row } from './lib/home';

  /** Home (decision 0100): the oldest waiting decision, the rest of the queue, the four fleet counts and Continue. */
  let { workspaces, agents, servers, now, lastKey, offer, focusKey = null, tabStates = {}, onopen, onanswer, onreply, onservers, onnew, onhook }: {
    workspaces: HubWorkspace[]; agents: AgentMap; servers: HubServer[]; now: number; lastKey: string | null;
    /** Per-tab state from `hub.tabStates`, so the workspace row follows all its tabs. */
    tabStates?: Record<string, TabAgentState>;
    onopen: (key: string) => void;
    onanswer: (key: string, choice: number, prompt: NonNullable<WorkspaceAgent['prompt']>) => void;
    onreply: (key: string, text: string) => void;
    onservers: () => void; onnew: (serverId?: string) => void;
    /** The hook offer to show, if one is due (decision 0106), and what the user chose on it. */
    offer?: HookOffer; onhook?: (choice: 'enable' | 'later' | 'never', offer: HookOffer) => void;
    /** The workspace a push's deep link named (U11): its decision is shown first and scrolled to, while it still waits. */
    focusKey?: string | null;
  } = $props();

  const FILTERS = ['needs-you', 'working', 'failed', 'finished'] as const;
  let filter = $state<FleetState | null>(null);
  // The queue moves only when no touch is down and 2 s after a scroll (ui-never-moves-under-the-finger); a beat re-reads it afterwards.
  const settle = new Settle<HomeQueue & { decision?: Decision; offerKey?: string }>();
  let beat = $state(0);
  onMount(() => { const t = setInterval(() => beat++, 500); return () => clearInterval(t); });

  // The servers that say they are unhealthy (U14): a server with no verdict yet, or an older one, is not.
  const unhealthy = $derived(new Set(servers.filter((s) => s.health && !s.health.healthy).map((s) => s.id)));

  const counts = $derived(fleetCounts(workspaces, agents, tabStates));
  const live = $derived.by(() => {
    const q = splitQueue(queueOf(workspaces, agents, unhealthy, tabStates), focusKey);
    return { ...q, decision: q.hero ? decisionFor(q.hero, agents[q.hero.key], now) : undefined, offerKey: offer ? `${offer.serverId}|${offer.agent}` : undefined };
  });
  const view = $derived.by(() => { beat; return settle.view(live, Date.now()); });
  let decisionEl = $state<HTMLElement>();
  $effect(() => { if (focusKey && view.hero?.key === focusKey) decisionEl?.scrollIntoView({ block: 'start' }); });
  const connected = $derived(new Set(servers.filter((s) => s.state === 'connected').map((s) => s.id)));
  const cont = $derived(continueTarget(lastKey, workspaces, connected, [...(view.hero ? [view.hero] : []), ...view.rest]));
  const filtered = $derived(filter ? workspaces.map((s) => rowOf(s, agents[s.key], unhealthy.has(s.serverId), tabStates)).filter((r) => r.state === filter) : []);
  const KIND_WORD = { needs_input: 'waiting', failed: 'failed', finished_unseen: 'finished' } as const;

  const meta = (r: Row) => [r.serverName, r.kind && r.since ? `${KIND_WORD[r.kind]} ${ageText(r.since, now)}` : ''].filter(Boolean).join(' · ');
  const detach = (id: number) => settle.touchUp(id);
</script>

<div class="flex min-h-0 flex-1 flex-col gap-4 overflow-y-auto p-4" data-home
  onpointerdowncapture={(e) => settle.touchDown(e.pointerId)} onpointerupcapture={(e) => detach(e.pointerId)} onpointercancelcapture={(e) => detach(e.pointerId)}
  onscroll={() => settle.scrolled(Date.now())}>
  <h1 class="sr-only">Home</h1>

  {#if servers.length === 0}
    <EmptyState title="No servers yet" body="Add a server and its agents will gather here.">
      <button type="button" onclick={onservers} class="min-h-11 rounded-card bg-accent px-4 text-base font-semibold text-on-accent">Add a server</button>
    </EmptyState>
  {:else}
    <div class="grid grid-cols-4 gap-2" role="group" aria-label="Fleet" data-home-tiles>
      {#each FILTERS as f (f)}
        <FleetTile state={f} count={counts[f]} pressed={filter === f} onclick={() => (filter = filter === f ? null : f)} />
      {/each}
    </div>

    {#if filter}
      <section class="flex flex-col gap-2" aria-label="Filtered workspaces" data-home-filter>
        {#each filtered as r (r.key)}
          <QueueItem name={r.name} summary={r.title} meta={meta(r)} state={r.state} agent={r.agent} unhealthy={r.unhealthy} onclick={() => onopen(r.key)} />
        {:else}
          <p class="py-6 text-center text-base text-soft">No workspaces in this state.</p>
        {/each}
      </section>
    {:else}
      {#if view.hero && view.decision}
        {@const h = view.hero}
        {@const d = view.decision}
        <div bind:this={decisionEl} data-decision-card={h.key}>
          <DecisionPanel decision={d} {now} server={h.serverName} unhealthy={h.unhealthy}
            onpick={(i) => d.prompt && onanswer(h.key, i, d.prompt)} onreply={(text) => onreply(h.key, text)} onopen={() => onopen(h.key)} />
        </div>
      {/if}

      <!-- Inserted only when nothing is being touched (it is part of the settled view), so the queue never jumps under a finger. -->
      {#if offer && view.offerKey === `${offer.serverId}|${offer.agent}`}
        <HookOfferCard {offer} onenable={() => onhook?.('enable', offer)} onlater={() => onhook?.('later', offer)} onnever={() => onhook?.('never', offer)} />
      {/if}

      {#if view.rest.length}
        <section class="flex flex-col gap-2" aria-label="Queue" data-home-queue>
          {#each view.rest as r (r.key)}
            <QueueItem name={r.name} summary={r.title} meta={meta(r)} state={r.state} agent={r.agent} unhealthy={r.unhealthy} onclick={() => onopen(r.key)} />
          {/each}
        </section>
      {/if}

      {#if cont}
        <section class="flex flex-col gap-2" aria-label="Continue" data-home-continue>
          <h2 class="text-sm font-semibold text-soft">Continue</h2>
          <QueueItem name={cont.name} meta={`${cont.serverName} · last used`} state={rowOf(cont, agents[cont.key], false, tabStates).state} agent={agents[cont.key]?.agent} unhealthy={unhealthy.has(cont.serverId)} onclick={() => onopen(cont.key)} />
        </section>
      {:else if !view.hero && !view.rest.length}
        {#if workspaces.length === 0}
          <EmptyState title="No workspaces yet" body="Start one and it shows up here.">
            <button type="button" onclick={() => onnew()} class="min-h-11 rounded-card bg-accent px-4 text-base font-semibold text-on-accent">Start a workspace</button>
          </EmptyState>
        {:else}
          <EmptyState title="All quiet" body="Nothing needs you right now." />
        {/if}
      {/if}
    {/if}
  {/if}
</div>
