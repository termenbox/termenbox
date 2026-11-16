# Questions

Short answers, each with the page that has the detail. Back to the [README](../../README.md). Related: [Concepts](../concepts.md), [Troubleshooting](./troubleshooting.md).

## Can I use Claude Code or Codex from my phone?

Yes. Install Termenbox on the machine where the agent runs, then use the app as the terminal: start the agent in a workspace, answer its questions from the banner, and come back later to the same workspace. Supported browsers are iOS Safari 16.4+, Chrome on Android 111+, and current Chrome, Firefox, Edge and Safari on desktop. See [Install](../start/install.md).

## Is this like Mosh or tmux over SSH?

The persistence is tmux's: the server keeps the workspace and each connection attaches to it. Termenbox adds a browser client made for touch, a connection that tries its paths again after a network change, and notifications. It is not an SSH client and does not need one. See [How it connects](../features/connecting.md).

## Does my terminal traffic go through your servers?

No. It goes from your browser to your machine, encrypted, and the browser pins the server's fingerprint. Only a signed record, the server's public key and address, goes through the directory, and LAN addresses never do. See [Is it safe to use?](../security/safety.md).

## Does it need a public IP or port forwarding?

On the same network as your phone, no: scan the QR and it connects over the LAN. Away from home you need one reachable UDP port, an overlay such as Tailscale, or a tunnel. On an iPhone the LAN path depends on Safari's Local Network permission, which is not fully tested; the link and the QR work wherever the phone can reach the machine. The app lists every path it tried and what to fix. See [How it connects](../features/connecting.md).

## Who can open a workspace?

Only the GitHub account that claims the server first, from a browser you have enrolled. Everyone else is refused. See [Is it safe to use?](../security/safety.md).

## What does the default install give the app access to?

The default install runs workspaces directly on the machine as your own user, so the app gets a shell with your repositories, keys and agent logins. Choose the container path if that is more access than you want. See [Where your agents run](../start/install.md#where-your-agents-run).

## Will I be told when an agent needs me?

Yes. When a Claude Code workspace asks for you, finishes or fails, the app shows it in a banner and in the Inbox. To get a notification on a phone that is not looking at the app, run `termenbox-server --setup-push` once and turn notifications on in the app; on an iPhone the app must be on the Home Screen first (iOS 16.4+). A notification only says that a workspace needs you and never carries what the agent wrote. See [Notifications and agent events](../features/notifications.md).

## What does it cost?

Nothing beyond the machine you run it on. There is no subscription, and using Termenbox never requires installing an app: the browser is the product. See [Install](../start/install.md).
