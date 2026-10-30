# Workspaces, agents and notifications

Back to the [README](../../README.md).

## Workspaces and tabs

A **workspace** is a server-owned record: a name, a directory and the tabs inside it. A **tab** is one process window of that workspace: a tmux window in this stage, running a shell, an agent TUI or any command. The workspace is never empty; its first tab is created with it, and deleting the last tab leaves a fresh shell tab instead.

- **Desktop.** The sidebar lists workspaces; the tab strip at the top of an open workspace lists its tabs. `+` creates another tab in the same workspace, not another workspace. A pencil renames the tab; the server keeps the order, labels and focus for every device.
- **Phone.** One plaque names the current tab; tap it for the tab list. There is no separate Workspaces sheet in an open workspace. Swipe or tap a row to switch tabs; the ⋮ menu renames, reorders or deletes a tab (deleting asks first).
- **New tab.** Choose Shell, Claude Code, Codex, Copilot, OpenCode, Gemini, Kilo, Aider, Goose, Amp, Qwen or Cursor. Its directory is relative to the workspace and may not leave it (`..`, an absolute path or a symlink escape is refused).
- **Six agent tabs.** A workspace holds at most six agent tabs; shell tabs do not count and can still be added. A seventh agent tab is refused with a message to close one first.
- **Status.** Each tab shows what its agent does now: **Needs you**, **Working**, **Error**, **Done**, **Idle** or **No status**. The workspace row follows **Needs you → Error → Working (only when every agent tab works) → Idle → Done/Unknown**. A shell tab has no agent status and is ignored.
- **Recent and cache.** The tab menu filters by **Recent** (last reply first) or **Cache expiring** (a Claude Code prompt cache in its last minute). These filters belong to the tab menu, not the Workspaces page.
- **Detach or close.** On a server without tabs, Detach still hides the workspace and keeps it running. In the tab model, closing a workspace asks first and ends it and all its tabs. Every workspace row on the Workspaces page has a pencil to rename and a cross to close, and the cross asks first, naming the workspace and its server; each tab's own actions live behind its ⋮ menu, and deleting a tab asks first too. A finished or failed tab is acknowledged only when that tab is visible.
- **Restore.** When the tmux server is gone, the workspace record remains, marked dormant. Opening it recreates its tabs as shells with the same labels and order; the work inside them is gone.
- **Needs you.** When an agent waits for an answer, the banner names the workspace and offers its choices; the answer goes to the tab that asked, even when another tab of the same workspace is in front. A push deep link opens that tab, not just the workspace.
- **Servers.** The Servers screen lists the saved servers; Add Server shows the install command for a container or for host mode and waits for the server to appear.

An older server (protocol 1) keeps the previous one-tab-per-workspace UI: the top strip, the Workspaces button and the windows dropdown. The two paths share the same terminal and event stream; only a server that advertises `auth_ok.tabs` uses the new one.

ACP remains a first-class backend. The New workspace sheet still creates an ACP workspace with the Terminal/Agent view choice; it is shown as a single-agent workspace with one tab until R75 folds its window host into the tab model (decision 0149). The ACP view itself is unchanged.

These behaviours are tested in a real browser (`packages/client/e2e/`), including two real server processes with real tmux. The browser runs emulate a phone; the soft keyboard, safe areas and gestures on a real iPhone are checked by hand with `docs/skills/mobile-ui-device-check.md`.

## Workspace backends

The default (`TERMENBOX_BACKEND=auto`) uses tmux. To use another tool with a CLI, give three commands (no shell; `{name}` and `{id}` are replaced by a validated workspace name):

```
TERMENBOX_BACKEND=command
TERMENBOX_BACKEND_NAME=zellij
TERMENBOX_BACKEND_LIST=zellij list-sessions --short
TERMENBOX_BACKEND_CREATE=zellij attach --create-background {name}
TERMENBOX_BACKEND_ATTACH=zellij attach {id}
```

The attach command runs in a PTY. It must repaint the workspace when it starts and only detach when it is killed; that is how a dropped connection resumes. The zellij lines are an illustration and untested. Otherwise implement the `WorkspaceBackend` trait (`packages/server/src/backend.rs`).

### Agent view over ACP (T0)

New workspace lets you pick, per agent, a **Terminal** view (the agent's own TUI in tmux, the default) or an **Agent view** (ACP): the server starts the agent itself and the app draws Markdown replies, tool calls, diffs, the plan and permission questions you answer with one tap. The choice is remembered per server and agent. The control shows only for an agent the server lists and only while `tmux` is also offered; a server with only `acp` lists just the ACP agents.

The Agent view is off by default. Set `TERMENBOX_ACP=on` (or list `acp` in `TERMENBOX_BACKEND`) to turn it on; it adds a second workspace manager, so workspace ids become `tmux:<name>` and anything saved under the old id (hidden tabs, earlier push links) no longer matches. Two agents are built in: `claude`, which runs `claude-agent-acp`, and `opencode`, which runs `opencode acp`. An agent is offered when its command is on the server's `PATH`, checked on every connection, so an install shows up without a restart. The adapters are not part of the image. Install and log in inside the container, for example:

```
docker exec -it <container> npm i -g @agentclientprotocol/claude-agent-acp
```

then log in to the agent the way its own CLI does (the claude adapter reuses the Claude Code login; `opencode auth login` for opencode).

Login is not checked up front: a logged-out agent fails to start and says to log in on the server. Check the adapter's terms of use before you use a subscription login with it.

To change the list, write `acp-agents.toml` in the config directory. With no file the built-ins are the list. A file replaces them, unless it sets `builtin = true`, which adds the built-ins it does not name; an entry with the same name always wins:

```
builtin = true              # keep claude and opencode next to the agents below
max_sessions = 6            # agents at once (default 6); each is a Node or Rust process of a few hundred MB
[agents.claude]
command = "claude-agent-acp"
env_remove = ["CLAUDECODE"]
elicitation_form = false    # true for an agent that asks questions through the form request (opencode)
```

**Memory.** Plan on roughly a few hundred MB per running agent workspace. Six at once want a box larger than 512 MB; on a small box lower `max_sessions`, because there is no memory check and the kernel may kill a process instead.

**Restarts.** The server owns the agent process, so a closed browser changes nothing. When the server restarts it starts each agent again and resumes its conversation. A turn that was running at that moment is lost: the workspace says "A server restart interrupted the running turn" and raises a Failed item in the needs-you list. A workspace that could not be restored (too many for `max_sessions`, an agent that would not start) stays in the list with the reason and is tried again at the next restart; close it to remove it. While workspaces are still restoring, one shows "Restoring after a server restart"; reopen it when it is done.

Keep `tmux` in `TERMENBOX_BACKEND`: the **Shell** key in an agent workspace opens a plain tmux session in the same directory (an ACP agent has no terminal of its own). See `docs/skills/probe-an-agent-over-acp.md` for adding an agent.

## Notifications

When a Claude Code workspace asks for you, finishes or fails, the server raises an item the app shows in the banner, the tab title, the app badge and the Inbox (Settings → Appearance, notifications and more → Inbox). To also get a notification on a phone that is not looking at the app, set up Web Push once:

```
termenbox-server --setup-push     # writes the key to connectors.env (mode 0600) next to the server's other state
```

Restart the server, then Settings → Appearance, notifications and more → Turn on notifications (on iPhone the app must be added to the Home Screen first, iOS 16.4+). A notification is sent about 5 seconds after the item appears, unless a device is showing that workspace. It names the workspace and what happened and never carries anything the agent wrote. A phone follows one server at a time.

Tapping a notification opens Home on that workspace's decision card, so one tap on an option answers it. A question that waits is pushed again after 10 minutes, then at growing gaps (twice as long each time, up to an hour) until you answer it on any device. Settings → Push reminders sets the wait, the longest gap, quiet hours in your local time (for example `22:00-07:00`) and which agents are muted; quiet hours and mutes silence every push, the first included. Finished and failed items are pushed once.

## Agent events

The server turns what agents do into a small typed stream (idle, working, needs input, done) that clients can follow. With no setup it comes from a process scan and screen heuristics (low confidence). For exact tool and "needs input" events from Claude Code, opt in:

```
termenbox-server --connect-claude          # shows the hook lines it would add
termenbox-server --connect-claude --yes    # applies them (backup kept)
termenbox-server --disconnect-claude       # removes exactly those again
```

For exact OpenCode events, opt in to its plugin the same way:

```
termenbox-server --connect-opencode          # shows the plugin it would write
termenbox-server --connect-opencode --yes    # writes <config>/plugins/termenbox.js (backup kept)
termenbox-server --disconnect-opencode       # removes exactly that again
```

The OpenCode config directory is `$OPENCODE_CONFIG_DIR`, else `$XDG_CONFIG_HOME/opencode`, else `~/.config/opencode`; `--opencode-dir DIR` names it explicitly. Restart OpenCode after connecting.

Only workspaces created by Termenbox carry the variables hooks need. Agent text in events is cleaned (control characters, secrets, addresses) and capped before it is stored or sent. Details: `docs/plans/archive/termenbox-v6-agent-events.md`.
