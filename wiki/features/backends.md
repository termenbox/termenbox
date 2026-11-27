# Workspace backends and the Agent view

How a workspace is kept alive, and how to run it through another tool or through an agent's own protocol. Back to the [README](../../README.md). Related: [Workspaces and tabs](./workspaces.md), [Notifications and agent events](./notifications.md).

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

### Agent view over ACP

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

Keep `tmux` in `TERMENBOX_BACKEND`: the **Shell** key in an agent workspace opens a plain tmux session in the same directory (an ACP agent has no terminal of its own).
