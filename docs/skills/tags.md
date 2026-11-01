---
id: tags
title: Tag Vocabulary
status: active
date: 2026-10-09
bootstrap-generated: true
---

# Tag Vocabulary

| Tag | Meaning |
|---|---|
| architecture | System structure, component boundaries |
| agents | Coding-agent tools (Claude Code, Codex) and their state or events |
| auth | Authentication, authorization, identity |
| build | Compilation, packaging, CI/CD |
| client | Browser-facing code (Svelte, WASM, UI) |
| cloudflare | Cloudflare services the project or an owner uses (Workers, TURN, STUN, R2, D1) |
| compatibility | Browser/version support matrix |
| container | Container installs (Docker image, host-side container updates) |
| crypto | Encryption, signatures, key derivation |
| deployment | VPS, Docker, systemd, networking |
| desktop | Desktop client, window chrome and desktop wrappers |
| distribution | App store, PWA, installation |
| guardrail | Derived normative rule |
| hooks | Shell integration, agent detection |
| infrastructure | Servers, databases, external services |
| ios | iOS Safari specific behavior |
| license | Legal, SPDX, compliance |
| lan | Local area network addressing, RFC1918, off-grid connectivity |
| mobile | Touch, gestures, battery, background |
| monorepo | Repository structure, workspaces |
| observability | Logs, crash records, diagnostics bundles, incident tracking |
| network | Transport, protocols, binding |
| parity | Web/native feature parity |
| performance | Latency, throughput, memory, CPU |
| platform | Native or browser platform detection and wrapper targets |
| privacy | What user data is collected, kept or sent |
| product | Product scope, positioning and promises |
| protocol | Wire format, handshake, sync |
| pwa | Progressive Web App features |
| rendering | Terminal display, WebGL, xterm.js |
| repo | Git, versioning, branching |
| security | Secrets, audit, hardening, paranoid |
| server | Backend Rust services |
| ssp | Screen Synchronization Protocol |
| terminal | ANSI/VT100/TUI emulation |
| tiers | Free, Pro, Team and Enterprise boundaries |
| billing | Payment, entitlements, invoicing |
| transport | WebTransport, WebSocket, QUIC |
| wsl | WSL2 and other NAT'd VM hosts |
| wasm | WebAssembly, Rust→WASM, wasm-bindgen |
| api-token | Scoped API tokens and the permissions they need |
| audit | Security-relevant action records |
| cellular | Mobile-carrier networks and their NAT behaviour |
| durable-object | Cloudflare Durable Objects |
| installer | The install script and what it changes on the host |
| limits | Quotas and caps set by a service or the plan |
| logging | The server's log output, files and levels |
| nat | Network address translation and hole punching |
| networking | Addressing, ports, firewalls and routes between devices |
| onboarding | First-run flow for a new user or server |
| release | Versioned builds, channels and their publication |
| stun | STUN discovery of public addresses |
| turn | TURN relays |
| updater | In-place server updates |
| control-plane | The attention control plane: conductor, operator session, clusters, contracts, deputy, dispatcher |
| deputy | The optional triage model that answers obligations on a ladder |
| fleet | Many nodes and agents run as one |
| dogfood | Using a slice on the owner's own fleet before it counts as done |

<!-- Umbrella tags (carried by >50% of active decisions): -->
<!-- None yet — decisions still drafting -->
cost — introduced by go-getter pack context@0.3.0
guardrail — introduced by go-getter pack context@0.3.0
harness — introduced by go-getter pack context@0.3.0
practice-packs — introduced by go-getter pack context@0.3.0
model-routing — introduced by go-getter pack cost-routing@0.2.0
agent-roles — introduced by go-getter pack orchestration@0.1.0
ci — introduced by go-getter pack verification-gate@0.3.0
verification — introduced by go-getter pack verification-gate@0.3.0
git-hooks — introduced by go-getter pack git-workflow@0.1.1
procedural — introduced by go-getter pack git-workflow@0.1.1
hooks — introduced by go-getter pack tracker@0.3.0
roadmap — introduced by go-getter pack tracker@0.3.0
notifications — push, inbox, badge and banner that tell the owner an agent needs them
sessions — terminal sessions and the managers that own them
acp — the Agent Client Protocol backend and the Agent view that draws it
legal — terms of use, licences and vendor policy that bound what the product may do
accessibility — contrast, text alternatives and sizes that keep the UI usable for everyone
automation — rules and actions that run without a person present
navigation — moving between screens, turns and landmarks
ux — user experience and interface design
procedural — tag for how-to procedures
process — how the project is built, organized or shipped
