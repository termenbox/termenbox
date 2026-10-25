---
id: 0136-a-workspace-exposes-its-agent-windows-over-acp-v1-plus-an-extension-registry
title: A workspace exposes its agent windows over one protocol, ACP v1 plus an extension registry, behind replaceable ports
status: active
amends: 0111-acp-sessions-use-one-event-model-and-per-agent-quirk-profiles
date: 2026-10-25
track: product
accepted-by: Sergey Strebulaev <s@vivantel.dev>
tags: [agents, protocol, architecture, roadmap]
---

## Decision

- **One protocol for every agent window.** The wire between server and client carries a protocol whose **core is ACP v1 verbatim** (message shapes, `session/update` kinds, content blocks, permission requests, plan, commands, config options) plus an **extension registry**. An extension is namespaced, versioned, negotiated at `initialize`, and ignorable: a peer that does not know one skips it. The protocol is termenbox's own and is the base for the owner's future ACP successor; the successor changes the core by a version bump and pilots changes as extensions. This replaces the draft design in which `AcpView`/`AcpEntry` snapshots (`rev`, `epoch`, `first`) were the wire; they become a client-side projection, if kept at all.
- **Adapters are the only place that knows an agent.** Behind one server-side port, three built-in adapters turn an agent window into that protocol: **native ACP** (the agent is a server-spawned stdio child, tier T0, near identity), **hook** (T1/T2) and **scan/screen** (T3) for an agent TUI running in a PTY window. Lower tiers emit the same messages at lower fidelity and state their tier in an extension; they never invent content (no fabricated diffs, options or plans). The tier mapping of 0103 and the per-event mapping of 0111 stay; the hub, reducer, attention and push (0095, 0122) are unchanged.
- **Ports, not concretes (SOLID).** The protocol (types and codec), the adapter, the transport, the window host and the client renderer are each behind an interface so that any one can be replaced: a new protocol version, another transport, an own PTY host instead of tmux. v1 pluggability is a compiled-in adapter registry, **out-of-process adapters** speaking the protocol over stdio and declared in a server-side profile file (the command allowlist stays server-side, 0111), and a client **renderer registry** keyed by content or extension type with a generic fallback for unknown ones. The browser loads no dynamic code.
- **Core types come from the schema crate, our transport stays ours.** Server and client share the typed ACP v1 core (`agent-client-protocol-schema`, as adopted in plan `acp-session-modes`, choice G) and the extension registry's own types in `termenbox-core`. Lenient decoding stays: a message that fails typed decoding is dropped alone.
- **Agent windows are addressed by ACP `sessionId`.** See 0138: a workspace holds windows; each agent window is exactly one ACP session, and everything on the stream is routed by `sessionId`. Window, focus and tier information travel as extensions.
- **Either host behind the same port.** A base agent window is a native ACP child (no tmux pane, as agreed in `acp-session-modes`) or the agent's own TUI in a tmux PTY window wrapped by the hook/scan adapter. Session code does not know which. Terminal windows (shell, lazygit, mc) and the raw TUI of a PTY agent window use the terminal byte channel (0139).
- **Conformance is tested, not asserted.** Guardrail `protocol-core-stays-acp-v1`: stripping every extension leaves a valid ACP stream. CI replays the recorded agent fixtures (`packages/server/testdata/acp/`) and asserts that the client renders every standard update kind and that the stripped stream validates against the schema.

## Why

0111 already made ACP updates and hooks share one event model but left two view protocols on the wire (`AcpView` for ACP, `SessionViewMessage` for the rest), so the client carries two render paths and a permission prompt looks different by source. Making the wire ACP itself removes that split at the protocol, makes the stream readable by any ACP-aware tool, and gives the owner's successor a conformant base to evolve. Extensions rather than a fork keep the core from drifting while still carrying what a workspace needs and ACP lacks (windows, focus, control, replay, tier). Ports are required by the owner so that the protocol and its implementation are replaceable. Tmux stays under the window host (0054): re-implementing persistence, scrollback and resize in an own multiplexer is a large rewrite with no gain for this goal.

## Rejected

| Option | Why not |
|---|---|
| Keep `AcpView`/`AcpEntry` on the wire (the draft of this decision) | The wire is not ACP, so conformance holds only at the adapter edge, and every ACP revision is a manual port |
| Own schema, ACP translated at the edge | Bigger design up front and drift from what the agents follow |
| Defer: ACP plus `_meta` only | "Pluggable" stays informal; no place to pilot the successor |
| Dynamically loaded client renderers | Arbitrary code in an authenticated owner session; revisit after the protocol settles |
| Only server-spawned ACP agents | Existing TUI workflows never get the unified view |
| Always a native ACP child | Loses agents without an adapter and the TUI escape hatch |

## Consequences

- `AcpView.svelte` becomes the single agent view over the protocol's reducer; `SessionViewMessage` and the `acp_view` message are served to old clients for one release window and then removed (0138).
- Adapters live behind one trait in `packages/server`; the old `AcpEntry` mapping becomes a client projection or is dropped.
- The ACP extension and `_meta` rules are checked against the ACP specification before the registry's shape is fixed (plan step 3.1).
- Amends 0111: ACP is no longer only T0 mapped into `AgentEvent`; it is the protocol that all tiers emit. Extends 0103: tiers are carried in an extension.
