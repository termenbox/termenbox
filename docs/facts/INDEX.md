# Facts Index

| ID | Title | Tags | Status |
|---|---|---|---|
| 0001 | Target Deployment Environment | deployment, infrastructure | active |
| 0002 | Client Framework Choice | client, framework, svelte | active |
| 0003 | Minimum Supported Browsers | client, browser-support, compatibility | active |
| 0004 | Transport Protocol Strategy | transport, webtransport, websocket, protocol | deferred |
| 0005 | Screen Synchronization Algorithm | ssp, protocol, crdt, terminal | deferred |
| 0006 | Wire Protocol Packet Format | protocol, wire-format, protobuf, serialization | active |
| 0007 | iOS Safari Mitigation Strategy | ios, safari, pwa, background, persistence | deferred |
| 0008 | SSH Authentication Method | auth, ssh, security | deferred |
| 0009 | Agent Detection and Hooking | agents, integration, hooks, pty | active |
| 0010 | Terminal Rendering Approach | client, rendering, xterm, webgl | active |
| 0011 | Packet Encryption Cipher | crypto, chacha20, poly1305, encryption | deferred |
| 0012 | Repository Structure | repo, monorepo, cargo, pnpm | active |
| 0013 | Project License | license, legal, mit | active |
| 0014 | Terminal Emulation Library | terminal, vte, wasm, emulation | deferred |
| 0015 | User-Managed Cloudflare Tunnel | deployment, networking, cloudflare | active |
| 0016 | Browser Capabilities Relevant to Termenbox | client, browser-support, ios, pwa, crypto | active |
| 0018 | Quick Tunnel, Directory Budget and Cloudflare OAuth Limits | networking, cloudflare, limits, tunnels | active |
| 0019 | Server Onboarding Options and VPS Provider Capabilities | onboarding, deployment, providers, ux | active |
| 0020 | Free Relay Service Limits for Terminal Traffic | networking, limits, relay | active |
| 0021 | WebRTC Direct Prototype Results | transport, webrtc, networking, prototype | active |
| 0022 | Free-tier capacity model | directory, scale, cloudflare, capacity | active |
| 0023 | Threat model | security, threat-model | active |
| 0024 | R2 and CDN spike results | directory, scale, cloudflare, r2, spike | active |
| 0025 | Free write path spike results | directory, scale, nostr, spike | active |
| 0026 | Client UI baseline before the v3 UI work | client, mobile, ios | active |
| 0027 | What iOS fires on a swipe-typing correction in xterm's textarea | client, mobile, ios, terminal | active |
| 0028 | The server binary and installer before the native host mode | server, deployment, build | active |
| 0029 | What Claude Code and Codex expose for agent events | agents, hooks, server | active |
| 0031 | Claude Code gives its status line command the prompt cache expiry | agents, hooks | active |
| 0032 | iOS Safari zooms the page into a focused field under 16px | ios, client, mobile | active |
| 0033 | Svelte 5 in legacy mode rebuilds a key block whose key is an object | client | active |
| 0034 | Without a hook, "working" means the pane printed something in the last 3 seconds | agents, server | active |
| 0035 | Which tier features need infrastructure beyond free tiers | tiers, scale, cloudflare, capacity | active |
| 0036 | The task name without a hook would come from tmux's pane title, which is not yet measured | agents, server | active |
| 0037 | Terminal output and agent status share one ordered JSON channel, with output as base64 | transport, protocol, agents | active |
| 0038 | The client has icons for 11 agents; only Claude Code and OpenCode have a verified event surface | agents, hooks, client | active |
| 0039 | Android is in the support floor but has never had a real-device check | client, mobile, android | active |
| 0040 | What each ACP agent really sends and accepts, measured for the tiered session view | agents, protocol, client | active |
| 0041 | Anthropic Claude model pricing including cache economics | cost, model-routing, governance | active |
| 0042 | OpenAI model pricing including cached input | cost, model-routing, governance | active |
| 0043 | Google Gemini API model pricing including context caching | cost, model-routing, governance | active |
| 0044 | Priced models are grouped into small, medium and large tiers per provider, and each host agent is mapped to the providers whose models it can run | model-routing, cost, host-agents | active |
| 0045 | A dialog's input takes focus a moment after the dialog is visible, so typing straight away loses the first letters | client | active |
| 0046 | One machine can already hold a system install and a user install for each user; what coexists and what conflicts | deployment, server | active |
| 0047 | One server instance idles at about 8.3 MiB RSS (8.7 with three sessions); five instances cost about 41 to 44 MiB | deployment, server, performance | active |
| 0048 | Claude Code 2.1.302 sends PermissionRequest for AskUserQuestion at once, Notification up to six seconds later, and no hook when a dialog or a turn is cancelled with Esc | agents, hooks, claude | active |
| 0049 | The U14 health sampler costs about 62 microseconds of CPU per 10 s tick and about 36 KiB of RSS, far under the 1% CPU budget | server, performance, health | active |
| 0050 | xterm.js 5.3 reads Android soft-keyboard input from its hidden textarea after a timer, so a microtask must not rewrite it | client, mobile, android, ios, terminal, xterm | active |
| 0051 | The server needs no external call to accept a connection after claim | transport, auth, network, deployment | active |
| 0052 | CPace over ristretto255 fits the pairing code on both ends with no WASM; the phone half of the spike is still open | pake, security, client, server, onboarding, spike | draft |
| 0053 | A server inside a NAT'd VM (WSL2 NAT mode, VirtualBox NAT, Docker Desktop) is reachable from other devices only through port forwarding or a bridged network | network, deployment, wsl, lan, topology | active |
| 0055 | A waiting question is pushed again on a ladder until it is answered anywhere, and a push opens its decision card | notifications, agents | active |
| 0056 | The UX harness builds on existing Playwright pieces, and CI emulates a phone, it does not verify one | client, mobile, ux, ci | active |
| 0057 | ACP's own extension rules and what the pinned schema crate covers, checked for the extension registry | agents, protocol, client | active |
| 0058 | Cloudflare STUN is free and unlimited, Cloudflare TURN is free to 1,000 GB a month, and Google publishes no STUN terms | network, cloudflare, limits, transport | active |
| 0059 | STUN-only punching failed from an iPhone on cellular to a WSL2 NAT server, and the Cloudflare TURN relay connected | network, nat, stun, turn, wsl, ios, cellular, transport | active |
| 0060 | The server uses the turn crate for its relay, and the Cloudflare setup needs three token scopes | turn, cloudflare, durable-object, api-token, transport | active |
| 0062 | The server logs to a file through a writer thread and builds a scrubbed diagnostics bundle on request | server, observability, performance, privacy | active |
| 0063 | A browser cannot pick a folder on the server, and a native modal dialog hides overlays that are not inside it | client, mobile, ux, browser-support | active |
| 0064 | The updater records what it knows in update-state.json and the server only reads it | server, updater, observability | active |
| 0065 | Agent TUIs can be scrolled to an earlier turn only by their own keys; none has an external jump | agents, terminal, client | active |
| 0066 | The repo has two video recorders and no docs media, and workflow artifacts expire | docs, ci, video, site | active |
| 0067 | A dispatch-only workflow cannot be run until its file is on the default branch, and issue and comment workflows run the default branch's copy with a write token | ci, github, security | active |
| 0068 | GitHub Markdown does not play a repository MP4 inline, and a chapter-length GIF is several megabytes | docs, video, github | active |
| 0069 | The Electron community template pins an end-of-life Electron 26, and the wrappers use Electron 45 | desktop, platform, security | active |
| 0074 | Claude Code status runs through three parallel paths that can disagree, and its asks lose their question text | agents, server, client, notifications | active |
| 0072 | Claude Code 2.1.308 shows a dialog on the screen and in the transcript within 80 ms of its hook, but the transcript lags the hook by 150 to 400 ms elsewhere, and Esc leaves a trace in the transcript and on the screen only | agents, hooks, claude, status | active |
| 0073 | Claude Code 2.1.308 reports background work in the Stop payload, wakes the session with a UserPromptSubmit, and ends a failing turn with StopFailure after about three minutes of retries | agents, hooks, claude, status | active |

<!-- bootstrap-generated: true -->
| 0070 | One person has about 28,800 seconds of attention a day, which is 1.2 seconds per agent-hour at 1,000 agents and 0.12 at 10,000 | product, control-plane, agents | active |
| 0071 | Anthropic's terms allow a subscription login in the unmodified Claude Code binary, but point Agent SDK products at API keys | agents, acp, legal | active |
| 0075 | Claude Code word-wraps with explicit row ends, so neither xterm nor tmux capture-pane -J can rejoin its lines | client, terminal, copy, tmux | active |
| 0076 | LiteLLM and OpenRouter publish model prices without a key; LiteLLM repeats each model under many provider prefixes and differs from fact 0041 on one cache price | meter, cost, network | active |
| 0077 | Neither transport carries a client-to-server upload frame yet; a tab's live folder is the path of its window state | protocol, server, client, upload | active |
