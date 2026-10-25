<script lang="ts">
  import * as snippets from './lib/snippets';
  import { NativeSelect } from './lib/ui/native-select';
  import type { HubWorkspace } from './lib/hub';
  import type { ClientMessage } from './lib/protocol';
  export let workspaces: HubWorkspace[];
  export let activeKey: string | null;
  export let send: (message: ClientMessage) => void;
  let target: string | null = null;
  let draft = '';
  let saved = snippets.load();
  $: replyTo = workspaces.find(s => s.key === target)?.key ?? workspaces.find(s => s.key === activeKey)?.key ?? workspaces[0]?.key;
  function reply(text: string) {
    const clean = snippets.clean(text);
    if (clean && replyTo) { send({ type: 'reply', session: replyTo, text: clean }); draft = ''; }
  }
</script>
{#if workspaces.length}
  <form class="reply" on:submit|preventDefault={() => reply(draft)}>
    <label for="reply-to">Quick reply to</label>
    <NativeSelect id="reply-to" value={replyTo} onchange={(e) => target = e.currentTarget.value}>
      {#each workspaces as s (s.key)}<option value={s.key}>{s.name} · {s.serverName}</option>{/each}
    </NativeSelect>
    <div class="snips">
      {#each saved as text}
        <span class="snip"><button type="button" class="s1" on:click={() => reply(text)}>{text}</button><button type="button" class="s2" aria-label={`Delete snippet ${text}`} on:click={() => { saved = snippets.remove(saved, text); snippets.save(saved); }}>×</button></span>
      {/each}
    </div>
    <div class="row">
      <input bind:value={draft} maxlength="200" placeholder="Type a reply" aria-label="Reply text" enterkeyhint="send" />
      <button type="submit" disabled={!draft.trim()}>Send</button>
      <button type="button" class="secondary" disabled={!draft.trim()} on:click={() => { saved = snippets.add(saved, draft); snippets.save(saved); }}>Save</button>
    </div>
  </form>
{/if}
<style>
  .reply { display: flex; flex-direction: column; gap: 8px; border-top: 1px solid var(--border); padding: 12px 0; margin-top: 12px; }
  label { color: var(--fg-muted); font-size: 0.75rem; }
  input, button { min-height: 44px; min-width: 44px; }
  .row, .snips { display: flex; gap: 6px; }
  .snips { flex-wrap: wrap; }
  .snip { display: inline-flex; border: 1px solid var(--border); border-radius: 12px; overflow: hidden; }
  .s1, .s2 { background: var(--bg); color: var(--fg); padding: 6px; }
  .s2 { border-left: 1px solid var(--border); }
  input { flex: 1; min-width: 0; }
  .row button { padding: 6px 8px; }
</style>
