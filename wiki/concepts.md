# Concepts

The words these pages use, each defined once. Back to the [README](../README.md). Related: [Install](./start/install.md), [Workspaces and tabs](./features/workspaces.md).

## Where things run

### Server
The Linux machine that runs your agents: a VPS, a home server or a laptop. It runs `termenbox-server`, keeps your workspaces alive in tmux and belongs to the first GitHub account that connects. See [Install](./start/install.md).

### Client
The app you open in a browser, on a phone or a laptop. It shows your workspaces and talks to your server directly, over an encrypted connection on one UDP port. See [How it connects](./features/connecting.md).

### Owner
The GitHub account that claimed the server first. Only the owner can open a workspace on it; everyone else is refused. See [Is it safe to use?](./security/safety.md).

## What you work in

### Workspace
A record on the server: a name, a directory and the tabs inside it. It is never empty: its first tab is created with it, and deleting the last tab leaves a fresh shell tab instead. Closing a workspace asks first and ends it and all its tabs. See [Workspaces and tabs](./features/workspaces.md#workspaces-and-tabs).

### Tab
One process window of a workspace: a tmux window running a shell, an agent or any command. The tab strip and tmux's windows share one order.

### Agent
A coding tool you start in a tab, such as Claude Code or Codex; any program that runs in a terminal works too. The list the new-tab menu offers, and how many agent tabs a workspace holds, are on [Agents](./reference/agents.md).

### Shell tab
A tab that runs a plain shell. It has no agent status, it is ignored when the workspace status is worked out, and it does not count towards the limit of agent tabs.

### Agent view
A second way to open an agent. Instead of the agent's own terminal, the app draws its replies, tool calls, diffs and questions. It is off by default. See [Workspace backends and the Agent view](./features/backends.md).

## How you know what is happening

### Status
What a tab's agent does now. The words are listed on [Status](./reference/status.md).

### Roll-up
The one status a workspace row shows: the first, in the order on [Status](./reference/status.md), that any of its agent tabs has.

### Needs you
What the app calls an agent that is waiting for an answer from you. The banner names the workspace and offers its choices, and the answer goes to the tab that asked. See [Notifications and agent events](./features/notifications.md).

## Staying connected

### Persistence
When your phone drops off the network, the workspace keeps running on the server. Reconnecting attaches to it again and repaints the screen.

### Path
One way the client can reach a server: a LAN address, a public IPv4 or IPv6 address, an overlay address such as Tailscale or WireGuard, or a tunnel. The app tries them together and keeps the first that connects. See [How it connects](./features/connecting.md#home-servers-lan-and-pairing-links).

### Pairing link
The link, the QR code and the short code the installer prints. Open the link or scan the code in **Add Server** to add that server to the app.
