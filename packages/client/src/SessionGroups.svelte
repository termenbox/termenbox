<script lang="ts">
  import { EmptyState, QueueItem } from './lib/ds';
  import type { AgentMap } from './lib/agents';
  import type { HubSession } from './lib/hub';
  import { ageText } from './lib/cards';
  import { sessionGroups, type Row } from './lib/home';

  /** The Sessions destination without an open session: every session, grouped by state, in the rows Home uses. */
  let { sessions, agents, now, onopen, onnew, unhealthy }: { sessions: HubSession[]; agents: AgentMap; now: number; /** Ids of the servers that say they are unhealthy (U14). */ unhealthy?: ReadonlySet<string>; onopen: (key: string) => void; onnew: (serverId?: string) => void } = $props();
  const groups = $derived(sessionGroups(sessions, agents, unhealthy));
  const meta = (r: Row) => [r.serverName, r.since ? `waiting ${ageText(r.since, now)}` : ''].filter(Boolean).join(' · ');
</script>

<div class="flex min-h-0 flex-1 flex-col gap-4 overflow-y-auto p-4" data-session-groups>
  {#each groups as g (g.state)}
    <section class="flex flex-col gap-2" aria-label={g.title}>
      <h2 class="text-sm font-semibold text-soft">{g.title} · {g.rows.length}</h2>
      {#each g.rows as r (r.key)}
        <QueueItem name={r.name} summary={r.title} meta={meta(r)} state={r.state} agent={r.agent} unhealthy={r.unhealthy} onclick={() => onopen(r.key)} />
      {/each}
    </section>
  {:else}
    <EmptyState title="No sessions yet" body="Start one and it shows up here.">
      <button type="button" onclick={() => onnew()} class="min-h-11 rounded-card bg-accent px-4 text-base font-semibold text-on-accent">Start a session</button>
    </EmptyState>
  {/each}
</div>
