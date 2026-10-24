# Sessions, agents and notifications

Back to the [README](../../README.md).

## Sessions, tabs and windows

- **Tabs.** Each running session is a tab at the top, showing the folder and the agent of the window it speaks for; sessions from every saved server share one strip. Several sessions in the same folder are told apart by name. Tap an inactive tab to open it (its last screen shows at once, then the live one), tap the active tab for its windows, long-press for actions.
- **Windows.** The dropdown of the active tab lists the tmux windows of the session: switch to one, close it (a running program or the last window asks first), or open a new one with **+ New window** in the folder of the current window.
- **Detach or close.** Detach hides the tab and keeps the session running; Close session asks first and ends it.
- **Needs you.** When an agent waits for an answer, a banner names the session and offers its choices without changing the tab you are on.
- **Servers.** The Servers screen lists the saved servers; Add Server shows the install command for a container or for host mode and waits for the server to appear.

These behaviours are tested in a real browser (`packages/client/e2e/`), including two real server processes with real tmux. The browser runs emulate a phone; the soft keyboard, safe areas and gestures on a real iPhone are checked by hand with `docs/skills/mobile-ui-device-check.md`.

## Session backends

The default (`TERMENBOX_BACKEND=auto`) uses tmux. To use another tool with a CLI, give three commands (no shell; `{name}` and `{id}` are replaced by a validated session name):

```
TERMENBOX_BACKEND=command
TERMENBOX_BACKEND_NAME=zellij
TERMENBOX_BACKEND_LIST=zellij list-sessions --short
TERMENBOX_BACKEND_CREATE=zellij attach --create-background {name}
TERMENBOX_BACKEND_ATTACH=zellij attach {id}
```

The attach command runs in a PTY. It must repaint the session when it starts and only detach when it is killed; that is how a dropped connection resumes. The zellij lines are an illustration and untested. Otherwise implement the `SessionBackend` trait (`packages/server/src/backend.rs`).

### Agent sessions over ACP (T0)

An agent that speaks the Agent Client Protocol can run as a session the server starts itself, with a view the app draws (Markdown replies, tool calls, diffs, the plan, permission questions you answer with one tap) instead of a terminal. The server starts only the commands you list, in `acp-agents.toml` in the config directory (a missing file means none):

```
max_sessions = 2            # agents at once; each is a Node or Rust process of a few hundred MB
[agents.claude]
command = "claude-agent-acp"
env_remove = ["CLAUDECODE"]
```

Run `TERMENBOX_BACKEND=tmux,acp` to offer both: New session then asks for the manager ("Agent session (ACP)") and the directory. Keep `tmux` in the list: the **Shell** key in an agent session opens a plain tmux session in the same directory (an ACP agent has no terminal of its own). See `docs/skills/probe-an-agent-over-acp.md` for adding an agent.

## Notifications

When a Claude Code session asks for you, finishes or fails, the server raises an item the app shows in the banner, the tab title, the app badge and the Inbox (Settings → Appearance, notifications and more → Inbox). To also get a notification on a phone that is not looking at the app, set up Web Push once:

```
termenbox-server --setup-push     # writes the key to connectors.env (mode 0600) next to the server's other state
```

Restart the server, then Settings → Appearance, notifications and more → Turn on notifications (on iPhone the app must be added to the Home Screen first, iOS 16.4+). A notification is sent about 5 seconds after the item appears, unless a device is showing that session. It names the session and what happened and never carries anything the agent wrote. A phone follows one server at a time.

Tapping a notification opens Home on that session's decision card, so one tap on an option answers it. A question that waits is pushed again after 10 minutes, then at growing gaps (twice as long each time, up to an hour) until you answer it on any device. Settings → Push reminders sets the wait, the longest gap, quiet hours in your local time (for example `22:00-07:00`) and which agents are muted; quiet hours and mutes silence every push, the first included. Finished and failed items are pushed once.

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

Only sessions created by Termenbox carry the variables hooks need. Agent text in events is cleaned (control characters, secrets, addresses) and capped before it is stored or sent. Details: `docs/plans/archive/termenbox-v6-agent-events.md`.
