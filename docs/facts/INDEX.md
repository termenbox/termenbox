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

<!-- bootstrap-generated: true -->
