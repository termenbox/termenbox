---
id: 0158-agent-view-is-the-default-view-and-for-claude-only-on-api-or-cloud-credentials
title: Agent view (ACP) is the default view for every agent; for Claude only on API-key or cloud credentials, a subscription login stays on Terminal
status: draft
date: 2026-11-01
track: product
accepted-by: Sergey Strebulaev <s@vivantel.dev>
tags: [agents, acp, client, legal]
amends: [0137]
related: [0149, 0111, 0159, 0160, 0161]
expires: draft until R74 (stable ids) lands and the owner records the terms decision; the default flip (R81) is blocked on both
---

## Decision

- **Agent view (ACP) becomes the main way an agent runs.** When an adapter for an agent is installed and ACP is enabled, a new agent tab opens in Agent view. Terminal stays one tap away per tab and is always the view of a `shell` tab. This amends 0137, where Terminal is the default and ACP is off by default.
- **Claude is credential-aware.** The Claude adapter (`claude-agent-acp`) is built on the Agent SDK. Agent view is the default for Claude only when the server sees API-key or cloud-provider credentials (`ANTHROPIC_API_KEY`, Bedrock, Vertex or Foundry settings). With a Claude subscription login, or when nothing is detected, Terminal stays the default for Claude because it runs the unmodified Claude Code binary, which Anthropic's terms allow with a subscription (fact 0066). The user can still choose Agent view by hand; the New Tab sheet says in one line why Terminal is the default.
- **Detection reads names, never secrets.** The server reports one value per agent (`api`, `cloud`, `subscription`, `mixed`, `none`) from environment variable names and file existence. It never reads, stores, logs or sends a credential value (`security-secrets-handling`). `mixed` is an API key or cloud setting present together with a subscription login: which one the adapter uses is measured first (plan step 0.2), and until it is known `mixed` is treated like `subscription`, so Terminal stays the default.
- **The switch is the last step.** All Agent view work ships with ACP still off by default. The flip (`TERMENBOX_ACP` on by default) waits for R74 (an added `acp` manager must not rename existing `tmux:` ids) and for the owner's recorded terms decision. Until both, this decision stays `draft`.
- **No fallback is removed.** Hooks and screen-scan tiers (T1 to T3) stay, so an adapter fault never leaves an agent unreachable.

## Why

The owner wants Agent view to be the main experience. The terms text restricts subscription logins through Agent SDK products but explicitly allows them in the unmodified binary, so one rule per credential type gives the main experience where it is clearly allowed and keeps the permitted path where it is not.

## Not chosen

- Flip for all agents including subscription Claude (an unresolved terms risk on the most used agent).
- Agent view only, Terminal removed for adapter-backed agents (one adapter bug would leave no way to reach the agent).
- Per-agent flip with Claude last (inconsistent defaults and no path for Claude users with an API key).
- Terminal default with Agent view opt-in (not "main", and the UI work would go unseen).

## Consequences

- The hello's `acp_agents` grows from names to names plus a credential kind (`packages/core/src/protocol.rs`, mirrored in `packages/client/src/lib/protocol.ts`), compatible with old clients.
- A user who logs in later changes the default only for new tabs; existing tabs keep their view.
