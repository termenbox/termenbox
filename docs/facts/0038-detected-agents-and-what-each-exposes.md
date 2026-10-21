---
id: 0038
title: The client has icons for 11 agents; only Claude Code and OpenCode have a verified event surface
status: active
tags: [agents, hooks, client]
date: 2026-10-16
kind: environmental
governed-by: 0103
last-verified: 2026-10-20
---

# The client has icons for 11 agents; only Claude Code and OpenCode have a verified event surface

Icons read from `packages/client/src/lib/agentIcons.ts` on 2026-10-16: claude, codex, copilot, opencode, kilo, gemini, aider, cursor-agent, amp, goose, qwen. The pane title is not a task source for Claude Code or Codex (fact 0036). Decision 0103 guarantees a T2 or T3 view per agent, so agents with no surface rely on the process scan and the screen.

## Measured 2026-10-16

Method: version from `--version`; hooks, notify and JSON mode from `--help` and `features list`; ACP by sending `initialize` over stdio from a throwaway script with a scratch `HOME` (no login, no owner sessions). Events below were measured on 2026-10-16 by a stdio script that sends one prompt (create a file, then a short plan) with `nvidia/nemotron-3.5-super-vl-120b-a12b` through the owner's NVIDIA key, and rejects any permission request. Not measured: the terms on driving an agent through an adapter.

| Agent | Version | Hooks | Notify | Log / JSON mode | ACP command | ACP `initialize` capabilities |
|---|---|---|---|---|---|---|
| claude | 2.1.301 | yes (fact 0029) | no | transcript file; `--output-format stream-json` | adapter `@agentclientprotocol/claude-agent-acp` 0.93.0 (was `@zed-industries/claude-code-acp` 0.16.2, deprecated) | `loadSession`; fork, list, resume; no `close` |
| codex | 0.162.1 | `hooks` feature, stable | `notify` program (unverified, R21) | `codex exec --json`; app-server | adapter `@agentclientprotocol/codex-acp` 2.2.2 (was `@zed-industries/codex-acp` 0.16.0, deprecated) | `loadSession`; list, resume, close |
| copilot | 1.0.98 | not seen in help | not seen | `--output-format json` (JSONL), `--log-dir` | native `copilot --acp` | `loadSession`; list, close; no resume |
| opencode | 2.0.29 (plugin re-check: 2.0.31) | no settings hooks; plugin events (below) | not seen | `run --format json`; `serve` API | native `opencode acp` | `loadSession`; additionalDirectories, close, delete, fork, list, resume |
| kilo | 7.6.2 | not seen in help | not seen | `run --format json`; `serve`, `export` | native `kilo acp` | `loadSession`; close, fork, list, resume |
| gemini | 0.64.0 | `gemini hooks` | not seen | `-o json` or `stream-json` | native `gemini --acp` | `loadSession` only |
| qwen | 0.25.2 | `qwen hooks` | not seen | telemetry outfile only | native `qwen --acp` | `loadSession`; list, resume |
| aider | 0.86.2 | not seen | `--notifications-command` runs a program when it waits for input | `--chat-history-file`, `--input-history-file` | no (not on the official list; no `acp` command) | n/a |
| cursor-agent | 2026.10.16-e3cb207 | not seen in help | not seen | `--print --output-format json` or `stream-json` | native `cursor-agent acp` | `loadSession`; list; no resume or close |
| amp | 0.0.1792179105-gcb8fd7 | plugins API (`amp plugins`, `show-docs`; not examined) | not seen | `amp threads export` (JSON) | no (`--acp` and `acp` rejected) | n/a |
| goose | 1.55.0 | not seen | not seen | `run --output-format json` or `stream-json` | native `goose acp` (and `goose serve` over HTTP/WebSocket) | `loadSession`; list, delete, close; no resume |

All eleven installed on 2026-10-16 (aider, cursor-agent, goose and amp by the owner from their official installers). Auth: the claude and codex adapters reuse the agent's own login (codex-acp also takes `CODEX_API_KEY` or `OPENAI_API_KEY`); opencode, kilo and copilot need their own `login`; gemini needs a Google login, API key or Vertex; qwen needs an OpenAI-style key. 

## ACP events actually sent

Superseded by fact 0040, which measured events, permissions, usage and lifecycle for all agents but cursor-agent (needs a login), with fixtures in `packages/server/testdata/acp/`. This table's earlier opencode, kilo, qwen and goose rows were incomplete (qwen does send `usage_update`; opencode sends `session_info_update`); codex-acp 0.16.0 and the `@zed-industries` adapter names are deprecated for `@agentclientprotocol/codex-acp` 2.2.2 and `@agentclientprotocol/claude-agent-acp` 0.93.0.

## Screens

`packages/server/testdata/screens/` has idle, working and after (or permission or error) captures for opencode, kilo, qwen and aider, plus goose's first-run telemetry and provider-setup prompts, and for claude (v2.1.302, 2026-10-17) idle, working (thinking and tool phases), finished, permission, trust, and an AskUserQuestion choice with its free-text row. aider asks `Create new file? (Y)es/(N)o` before writing. Not captured: a free-text question for any agent but claude, and screens for codex, gemini, copilot, cursor-agent and amp. Two older names are misleading: `opencode-permission` was recorded while opencode was working (footer `esc interrupt`), and `opencode-working` is the finished screen; no opencode permission prompt exists on file. `kilo-idle` is a blank capture and `kilo-working` shows an error, so kilo has no recorded working indicator.

## T2 profiles (step U07, 2026-10-17)

T2 is a profile file in `packages/server/profiles/`; the session view reports `t2` for an agent that has one and `t3` for the rest (`Profiles::for_agent`, matched on the foreground command name).

| Agent | Profile | What it reads | Not measured |
|---|---|---|---|
| claude 2.1.302 | yes | permission prompt (`Do you want to ...` + numbered list, keys are digits); working from the spinner line (`thinking)`, `tokens)`); idle/finished from `← for agents` and `Worked for` | AskUserQuestion screens are left unparsed on purpose: the digit on the `Type something.` row only moves the cursor (the `blocked` marker `Esc to cancel` / `Enter to select` still tells the reducer a dialog is open, decision 0122) |
| opencode 2.0.31 | yes | working (`esc interrupt`) and idle (`ask anything`, `ctrl+p commands`, idle only when not working) | any permission prompt or question |
| kilo | yes, minimal | idle footer (`tab agents`) | working indicator, permission prompt, question (no screens recorded) |
| codex | **no** | nothing | not installed on the build host, so no screen was recorded; stays T3 |

Claude Code 2.1.302 no longer shows `esc to interrupt` while working, and `auto mode on` is on every screen, so neither is a reliable marker any more.

## OpenCode plugin (2026-10-20)

OpenCode 2.x has no settings-file hooks, but it loads JavaScript/TypeScript plugins from
`<config>/plugins/` (global) and `.opencode/plugins/` (project). A plugin's default export has an `id` and a
`setup(ctx)`; `ctx.event.subscribe()` yields the same events the TUI sees. Termenbox's opt-in
`--connect-opencode` writes one plugin to `<config>/plugins/termenbox.js`; under systemd the server takes
`<config>` from a running `opencode` process's environment, since a service does not inherit the login shell's
`OPENCODE_CONFIG_DIR`/`XDG_CONFIG_HOME`. It sends only
`session.inbox.enqueued`, `session.execution.started/succeeded/failed/interrupted`,
`permission.asked/replied`, `form.created/replied/cancelled`, and the `session.tool.called/success/failed`
events to `termenbox-server --hook opencode` (through the `TERMENBOX_*` variables in a Termenbox session).
The adapter maps the execution events to turn edges, permissions and forms to obligations, and the prompt's
first line to the task title.

The event names and `data` fields were read from the generated types of `@opencode/client` 2.0.31 and then
recorded from a live OpenCode 2.0.31 run on 2026-10-20: a mock OpenAI-compatible provider, a Unix socket in
place of the event hub, and the generated plugin. The samples in
`packages/server/testdata/events/opencode-*.json` follow the captured events. Two shapes matter for the
adapter: a question form is titled `Questions`, with the actual question in the first field's `description`,
and `permission.asked` carries `action` and `resources` (it may omit `message`).

The plugin compacts each event to the fields the adapter reads, because tool output and provider metadata can
exceed the 64 KiB one hook line accepts; each string is capped, and the remembered tool names are bounded. It
remembers a tool's name from `session.tool.input.started` because `session.tool.called/success/failed` do not
carry one. `session.status` is not used: `busy` after a
pending permission would clear the obligation, and `idle` repeats the turn end already carried by
`session.execution.interrupted`/`succeeded`. The plugin does not forward `session.idle` for that reason: a late
idle would close a turn that already ended as `Done` (and clear any block). Disconnect restores an unchanged
plugin file or removes one Termenbox wrote; a file the owner edited since is left in place with the journal, and
the owner removes it by hand.

## T2 priority

The owner uses Claude Code most, opencode a little, kilo and Codex rarely (2026-10-16).

1. Claude Code (verified surface, fact 0029; profile done), then Codex (verify `notify` and hooks under R21; no profile yet), then opencode (profile done).
2. Kilo (minimal profile done).
3. The rest by how easily their prompts can be told apart: ACP-native agents first (opencode, kilo, gemini, qwen, copilot), then goose and cursor-agent, then aider and amp (no ACP; aider's notifications command is its only signal).

PR #139 (hook installers for claude, codex, copilot, kilo, opencode) is not in master: master was force-pushed at 2026-10-15T22:23Z (`7f0fd63` to `87988ac`), orphaning its merge commit `cb1098c`. Its source survives at `cb1098c` and `origin/test/agent-status-detection`; its per-agent hook formats were not verified against the agents.
