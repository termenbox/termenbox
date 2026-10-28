---
id: kill-session-needs-confirmation
title: Killing a workspace or an agent tab needs confirmation
status: active
governed-by: 0082
grounded-in: [0082, 0147]
derivation-note: Killing ends a running agent and cannot be undone, and phone taps are imprecise; a tab is a process now, so it needs the same rule a workspace has.
tags: [client, protocol]
---

# Guardrail: Killing a workspace or an agent tab needs confirmation

## Rule
**MUST** ask for confirmation before sending `close_workspace`. **MUST** ask before deleting an agent tab, and before deleting a shell tab whose foreground process is not a shell. **MUST NOT** put a kill action on the tab itself: Delete lives in the tab's menu, never as a one-tap on the row, and the workspace's Close stays a separate, confirmed action.
