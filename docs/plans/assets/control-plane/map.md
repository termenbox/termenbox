# Map of today's attention path

Read from the code on `develop` at the time of writing (step 0.2 of `docs/plans/attention-control-plane.md`); line numbers drift, grep the symbol. Paths are under `packages/`. Decisions 0095 and 0122 give the intent; this file is what the code does.

## 1. Where an obligation is raised, cleared and acknowledged

There are two things called attention. **Obligations** live inside a per-tab reducer; **Attention items** are the server-owned list a client sees.

- **Reducer (obligations).** `server/src/reducer.rs`: `Edge` (l.28) has `TurnOpen`, `TurnClose`, `BlockOpen(key)`, `BlockClose(key)`, `ToolStart`, `ToolEnd`. A `Block` is an open permission, question or elicitation; any open block makes the tab `NeedsInput` and nothing but its close ends it (`apply` l.84, `blocked` l.160). `reconcile` (l.165) closes a block or turn the screen no longer shows. A new prompt or a turn end drops every open block. The reducer holds no ids a client sees, only block keys.
- **Reducers are keyed `(workspace, tab)`** in `server/src/events.rs` `EventHub.reducers` (l.465); `aggregate` (l.325) makes the workspace state (a wait beats work, work beats a result). Bounded by `MAX_SESSIONS = 20` (l.21).
- **Event in.** Hook events enter `EventHub::ingest*` (l.518, l.708); agent-own events (ACP) enter at l.736 and are turned into edges at l.351 to l.369. `edge_of` (l.349): `Turn` to `TurnOpen`/`TurnClose`, `Block` to `BlockOpen`/`BlockClose`, hook `ToolStart`/`ToolEnd`.
- **Raise.** `EventHub::raise` (`events.rs` l.1135): one open item per `(workspace, tab, kind)`, a newer replaces the older; id is `a<counter>` from `next_attention` (l.460, l.1153), a per-server counter. List `attention: Vec<Attention>` (l.459), capped by `MAX_ATTENTION = 200` (l.27), oldest dropped first. Only a hook or Codex (a fact) raises (l.811 `tab` path, l.1074 workspace path); scan and screen only produce a `hint`.
- **Kinds.** `core/src/events.rs` `AttentionKind` (l.215): `needs_input`, `finished_unseen`, `failed`. Item struct `Attention` (l.234): `id, workspace, tab?, kind, raised_at, task?, prompt?` (the prompt is never sent in a push).
- **Clear by fact.** `clear_where` (`events.rs` l.1203) when the reducer reports the block gone (audit `attention_cleared by=fact`); `needs_input` clears only this way. Closing a tab or workspace: l.1175 and the retain at l.1321.
- **Acknowledge.** `EventHub::ack` (l.1217): a `finished_unseen` or `failed` item only, never `needs_input`; it also clears the tab's result in `tab_status`. Audit `attention_cleared by=ack`.
- **Answer.** Terminal agents: `answer_tab` (`server/src/workspaces.rs` l.2124) sends only the keys the agent profile lists for the chosen option; called from `ClientMessage::Answer` (`workspaces.rs` l.1082). ACP agents: `ClientMessage::AcpAnswer` (l.1483) to `backend.acp_answer` (`backend.rs` l.219 trait, `acp.rs` l.2096). The permission body is rendered in `acp_stream.rs` (`permission` l.824, `permission_answered` l.868, `cancel_open_permissions` l.906).
- **Audit lines** use `target: "audit"` with workspace, kind and source, never the prompt text.

## 2. Wire messages that carry attention, answers and acks

`core/src/protocol.rs`, mirrored by hand in `client/src/lib/protocol.ts`.

- `ClientMessage::Ack { attention_id }` (l.401) and `Dismiss { attention_id }` (l.405), handled together at `workspaces.rs` l.1077; TS l.57 to l.58.
- `ClientMessage::SessionAnswer { workspace, prompt_id, option }` (l.419, wire name `workspace_answer`): tap option N of the parsed prompt.
- `ClientMessage::Answer { workspace, tab, prompt_id, seq, choice }` handled at `workspaces.rs` l.1082 (the tab-aware answer).
- `ClientMessage::AcpAnswer { workspace, prompt_id, option_id }` (l.463).
- `ClientMessage::Foreground { workspace, tab }` (l.393) and `FocusTab` (l.313): which workspace this device shows.
- `ClientMessage::PushSubscribe/PushUnsubscribe` (l.383).
- `ServerMessage::Attention { items }` (l.730): the whole open list, sent on connect (`status_messages`, `workspaces.rs` l.695) and after every change (`status_message`, `workspaces.rs` l.1813 from `StatusChange::Attention`).
- `ServerMessage::Activity { workspaces }` (l.726): `WorkspaceActivity` (`core/src/events.rs` l.251) with `activity, task, agent, hint`.
- No message today carries an actor or a device name; an answer is audited with the connection's `identity` (`account_id`, provider).

## 3. Per server and per device

Per server (survives a client leaving): the `attention` list, reducers, rings (events), `tab_status`, push subscriptions and config (`push.rs` `Push`, subscriptions one per device, oldest dropped), quiet hours, muted agents. Everything is in memory except push state.

Per device or connection: the **foreground** report (`push.rs` `foreground: HashMap<conn, (workspace, tab)>` l.331, `set_foreground` l.533), the client's inbox (`client/src/lib/inbox.ts` `inbox` store l.62, persisted in `localStorage` by `loadInbox` l.41), the open view, drafts, scroll, notification settings. There is no operator cursor, lease or draft on the server (slice 3).

Nothing is cross-server: each server has its own counter, and the client holds one connection per server (`App.svelte` hub, `serverId`).

## 4. Where push is triggered

`server/src/push.rs`. `spawn_watcher` (l.684) subscribes to `StatusChange::Attention`, remembers seen ids (cleared past 1000), skips when push is unconfigured or no subscription exists, and spawns `climb` (l.712) per new item: wait `push.grace`, stop if the item left the list ("answered or acknowledged, on any device"), skip when `shown_tab` (a device shows it, l.551) or `suppressed` (quiet hours, muted agent), else `notify_tab`. A `needs_input` repeats on a ladder of growing waits (`next_wait` l.676, `EscalationConfig`); other kinds push once. Payload carries workspace name and kind only (`payload` l.258), never the prompt.

## 5. Where the client builds Needs-you

- Ingest: `App.svelte` l.191 on `msg.type === 'attention'` calls `mergeAttention` (`client/src/lib/inbox.ts` l.23), keyed by `workspaceKey(serverId, workspace)`, into the persisted `inbox` store.
- Waiting list: `waitingList` (`client/src/lib/notify.ts` l.11) over `allWorkspaces` and `allAgents`; `App.svelte` l.435 `$: waiting = waitingList(...)`.
- Cards and decisions: `decisionFor` (`lib/decision.ts` l.48) builds the question card from agent, attention prompt and task; `lib/cards.ts` has `chipFor` (l.24), `ageText` (l.38), `toolLine`, `staleText` (formatting only).
- Views: `Home.svelte` (oldest waiting decision, the queue, four fleet counts; `KIND_WORD` l.51), `NeedsYouBanner.svelte` (mounted at `App.svelte` l.1218, answers through `sendWorkspace({type:'answer',...})` and dismisses through `sendAttention`), `InboxSheet.svelte` (list with `WORDS` l.27), `DecisionPanel.svelte`, `QuickReply.svelte`.
- Ack on foreground: `App.svelte` l.455 to l.462 `sendAttention(activeKey, {type:'ack',...})`; foreground reports at l.384 to l.385.

## Ids that need a node key

A conductor merging N nodes would collide or mis-route on each of these (today's scope, and the key they need):

| Id today | Scope today | Needs |
|---|---|---|
| Attention `id` `a<n>` (`events.rs` l.1153) | one server's counter, restarts at 1 | `(node, id)`; also an epoch so a node restart does not reuse `a1` |
| `workspace` name / key | unique per server; the client already prefixes `serverId` (`workspaceKey`) | `(node, workspace)` |
| `tab` id | per workspace | `(node, workspace, tab)` |
| `prompt_id` and `seq` (answers, `Answer`) | per tab prompt | `(node, workspace, tab, prompt_id)`; the node validates `seq` |
| ACP `prompt_id` / `option_id` | per ACP session | `(node, workspace, prompt_id)`; `option_id` is echoed unchanged to the agent |
| Reducer block key | per tab, never leaves the node | stays node-local |
| Push subscription endpoint, `conn` id in `foreground` | per server | stays node-local; the conductor needs its own device set for the lease (slice 3) |
| `ClientMessage::Ack/Dismiss { attention_id }` | per server | must carry the node, or be routed by the conductor |
| Audit identity (`account_id`, `provider`) | the single owner | an `actor` and `owner` pair on every conductor answer (guardrail `operator-actions-carry-an-actor-id-apart-from-the-owner`) |
| `MAX_ATTENTION = 200`, `MAX_SESSIONS = 20` | per server | per-node bounds at the conductor (step 2.4) |
