<script lang="ts">
  import { iconFor } from './lib/agentIcons';
  /** A foreground command (`lazygit`) and/or the agent the server detected; the agent wins. */
  export let command: string | undefined = undefined;
  export let agent: string | undefined = undefined;
  export let size = 22;
  $: icon = iconFor(command, agent);
</script>
<span class="icon" role="img" aria-label={icon.label} title={icon.label} style="--c: {icon.color}; width: {size}px; height: {size}px; font-size: {Math.round(size * (icon.glyph.length > 2 ? 0.4 : icon.glyph.length > 1 ? 0.5 : 0.62))}px">{#if icon.shape}<svg viewBox="0 0 24 24" width="72%" height="72%" aria-hidden="true">{@html icon.shape}</svg>{:else}{icon.glyph}{/if}</span>
<style>
  .icon { display: inline-flex; align-items: center; justify-content: center; flex: 0 0 auto; border-radius: 6px; background: var(--c); color: #fff; font-weight: 700; font-family: system-ui, -apple-system, 'Segoe UI', Roboto, sans-serif; font-variant-emoji: text; line-height: 1; letter-spacing: -0.03em; user-select: none; }
</style>
