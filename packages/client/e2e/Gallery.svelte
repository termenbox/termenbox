<script lang="ts">
  import BottomBar from '../src/shell/BottomBar.svelte';
  import { NativeSelect } from '../src/lib/ui/native-select';
  import { StatusChip, AgentBadge, FleetTile, WorkspaceStrip, DecisionCard, OptionButton, QueueItem, Composer, MoreSheet, ChartFrame, EmptyState, STATES, type FleetState } from '../src/lib/ds';

  const states = Object.keys(STATES) as FleetState[];
  const strip = [
    { id: 'a', name: 'api', agent: 'claude', state: 'needs-you' as const },
    { id: 'b', name: 'web', agent: 'codex', state: 'working' as const },
    { id: 'c', name: 'docs', state: 'finished' as const },
  ];
  const groups = [
    { title: 'Agent', items: [{ label: 'Interrupt' }, { label: 'Compact' }] },
    { title: 'Keys', items: [{ label: 'Esc' }, { label: 'Tab' }, { label: '^C', danger: true }] },
  ];
  let selected = $state('a');
  let more = $state(false);
</script>

<main class="mx-auto flex max-w-[390px] flex-col gap-6 bg-bg p-4 pb-48 text-fg">
  <section aria-label="Status chip and agent badge" class="flex flex-col gap-3">
    <h1 class="text-title font-bold">Components</h1>
    <div class="flex flex-wrap gap-2">{#each states as state}<StatusChip {state} />{/each}</div>
    <div class="flex gap-3">{#each states as state}<AgentBadge agent="claude" {state} />{/each}</div>
  </section>

  <section aria-label="Fleet state tiles" class="grid grid-cols-4 gap-2">
    <FleetTile state="needs-you" count={2} onclick={() => {}} />
    <FleetTile state="working" count={5} onclick={() => {}} />
    <FleetTile state="failed" count={1} onclick={() => {}} />
    <FleetTile state="finished" count={7} onclick={() => {}} />
  </section>

  <WorkspaceStrip items={strip} {selected} onselect={(id) => (selected = id)} onadd={() => {}} />

  <DecisionCard agent="claude" workspace="api" waited="4 min" context="Edit src/server.rs, 12 lines" question="Allow this edit?" state="needs-you">
    <div class="flex flex-col gap-2">
      <OptionButton hint="1" label="Yes, once" detail="Only this edit" primary onclick={() => {}} />
      <OptionButton hint="2" label="Yes, for this workspace" onclick={() => {}} />
      <OptionButton hint="3" label="No" detail="Tell the agent why" onclick={() => {}} />
    </div>
  </DecisionCard>

  <section aria-label="Queue" class="flex flex-col gap-2">
    <QueueItem name="web" summary="Running the test suite" meta="3 min ago" state="working" agent="codex" onclick={() => {}} />
    <QueueItem name="docs" summary="Finished the changelog" state="finished" onclick={() => {}} />
  </section>

  <ChartFrame title="Workspaces per day" caption="12 workspaces this week, most on Tuesday.">
    {#snippet legend()}last 7 days{/snippet}
    <div class="h-24 rounded-control bg-raised" role="img" aria-label="Bar chart placeholder"></div>
  </ChartFrame>

  <EmptyState title="No workspaces yet" body="Start one and it shows up here with its state.">
    <button type="button" class="min-h-12 rounded-control bg-accent text-base text-on-accent" onclick={() => (more = true)}>Open More</button>
  </EmptyState>

  <section aria-label="Select" class="flex flex-col gap-2">
    <label class="flex items-center gap-3 text-base text-fg">Server
      <NativeSelect aria-label="Server"><option>alpha</option><option>beta</option></NativeSelect></label>
    <label class="flex items-center gap-3 text-base text-fg">Sort <NativeSelect aria-label="Sort" disabled><option>Recent</option></NativeSelect></label>
  </section>
  <section aria-label="Bottom bar"><BottomBar current="workspaces" needsYou={2} onnavigate={() => {}} class="rounded-card border" /></section>

  <MoreSheet bind:open={more} {groups} />
</main>

<div class="fixed inset-x-0 bottom-0 mx-auto max-w-[390px]">
  <Composer onmic={() => {}}>
    {#snippet decision()}
      <div class="flex min-h-14 items-center gap-2 rounded-card bg-warn/15 py-1 pr-1 pl-3">
        <span class="flex min-w-0 flex-1 flex-col"><b class="text-xs text-warn">Needs you</b><span class="truncate text-sm text-soft">Allow this edit?</span></span>
        <button type="button" class="min-h-11 shrink-0 rounded-control bg-warn px-3 text-base font-bold text-on-warn">1 Yes</button>
      </div>
    {/snippet}
  </Composer>
</div>
