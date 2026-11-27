# Notifications and agent events

How the app tells you that an agent needs you, and where the status comes from. Back to the [README](../../README.md). Related: [Workspaces and tabs](./workspaces.md), [Workspace backends and the Agent view](./backends.md).

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

Only workspaces created by Termenbox carry the variables hooks need. Agent text in events is cleaned (control characters, secrets, addresses) and capped before it is stored or sent.
