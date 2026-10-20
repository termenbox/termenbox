<script lang="ts">
  /** The exact change Enable will make (guardrail user-settings-are-wrapped-not-replaced): the whole file (Claude's
   *  settings or OpenCode's plugin) as it will be written. It is the server's text, shown as text only. */
  let { preview, label }: { preview?: string; label: string } = $props();
</script>

<div class="flex flex-col gap-1" data-hook-preview>
  <small class="text-xs font-semibold text-soft">What changes for {label}</small>
  {#if preview}
    <pre class="max-h-40 overflow-auto rounded-card bg-sunken p-3 font-mono text-xs whitespace-pre text-fg" tabindex="0" aria-label={`File after enabling hooks for ${label}`}>{preview}</pre>
    <small class="text-xs text-muted">Everything else stays as it is. A backup is kept, Remove undoes this, and restarting the agent makes it take effect.</small>
  {:else}
    <p class="rounded-card bg-sunken p-3 text-sm text-soft" role="status">Loading the exact change…</p>
  {/if}
</div>
