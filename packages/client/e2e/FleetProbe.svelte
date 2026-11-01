<script lang="ts">
  // The fleet Needs-you template on its fixtures: 3 nodes, 40 asks, one silent node. Props only, no store and no wire
  // (build-an-agent-view-slice-template-first). window.__fleet records which card was opened.
  import FleetNeedsYou from '../src/FleetNeedsYou.svelte';
  import { fleetItems } from '../src/lib/fleet';
  import { FLEET_NOW, fleetCards, fleetNodes } from '../src/lib/fleetFixtures';

  const items = fleetItems(fleetCards);
  const opened: string[] = [];
  Object.assign(window, { __fleet: { opened } });
</script>

<main class="mx-auto flex h-dvh max-w-xl flex-col gap-2 overflow-y-auto bg-bg p-4 text-fg">
  <h1 class="text-lg font-semibold">Needs you</h1>
  <FleetNeedsYou {items} nodes={fleetNodes} now={FLEET_NOW} onopen={(card) => opened.push(card)} />
</main>
