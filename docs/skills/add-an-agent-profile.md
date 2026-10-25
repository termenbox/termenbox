---
id: add-an-agent-profile
title: Add a T2 screen profile for an agent
status: active
date: 2026-10-16
tags: [agents, client, procedural]
---

# Procedure: Add a T2 screen profile for an agent

Use when a detected agent (see `packages/client/src/lib/agentIcons.ts`) only has the T3 generic view (decision 0103). The profile format is documented at the top of `packages/server/src/profile.rs` (schema, limits, how keys are derived); the profile files live in `packages/server/profiles/<agent>.toml`.

0. If the agent speaks ACP, first run `probe-an-agent-over-acp` so the profile can lean on structured data where it exists.
1. Run the agent in a throwaway tmux (`tmux -L scratch`), never the owner's sessions. Record screens for idle, working, a permission or choice prompt, a free-text question, and a finished task (`capture-pane -p -t =name:`). Save them under `packages/server/testdata/screens/<agent>-*.txt`.
2. Write `packages/server/profiles/<agent>.toml` and add it to `BUILTIN` in `profile.rs`: the agent name, its question and numbered-option or inline yes/no patterns, working and idle markers, the task line. Do not write keys: the engine derives them (a digit per numbered option, `y`/`n` plus Enter for inline), and a profile that names keys is rejected.
3. Add tests that run the profile over the recorded screens and assert the parsed state, options and keys. Include a screen where nothing should match.
4. Check that parsed text is sanitised and that a profile cannot send keys beyond the options the screen showed.
5. Update facts 0038 and 0040 with what the agent exposes and the agent version measured.

## Writing an adapter (decision 0136)

A T2 screen profile is data. A new agent that needs more than a profile is an **adapter** behind the server's adapter port: it turns the agent's payloads into the protocol (ACP v1 core plus extensions) and states its tier. Use the native ACP adapter when the agent speaks ACP; otherwise the hook or scan/screen adapter. An out-of-process adapter is declared in the server-side profile file and speaks the protocol over stdio. An adapter never emits content the agent did not provide (no invented diffs, options or plans), and the client needs no change: unknown content falls back to the generic renderer. Add fixtures and a test that strips the extensions and validates the core (guardrail `protocol-core-stays-acp-v1`).

