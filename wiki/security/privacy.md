# Privacy

What Termenbox keeps about you, and where. Back to the [README](../../README.md). Related: [Safety](./safety.md), [Contact](../help/contact.md).

Your server, your terminals and your workspaces stay on your own machine. Termenbox has no account system and no hosted service that sees your sessions. The one exception is the push relay of the mobile apps, described below. The mobile apps and the relay are planned and not released; this page says what they will keep before anyone installs them.

## The push relay (mobile apps only)

A phone that wants to be woken when an agent needs you registers with the project's push relay. A web browser, the desktop apps and a server that no phone has registered with never touch it.

| What the relay keeps | Why | How long |
|---|---|---|
| The **device token** Apple or Google gives the app | to send the wake-up to that phone | until the phone unregisters, or Apple or Google reports the token invalid, whichever comes first |
| The **platform** (iOS or Android) | to pick the right push service | the same |
| A **wake key** (the public half of a key made for this one registration) | to check that a wake-up comes from the server you paired | the same |
| The time of registration | rate limits | the same |

The relay does not keep your server's address or identity key, a workspace name, a path, an agent's text or any message. A wake-up carries fixed text ("Something needs you"), at most a count, and an opaque registration number. Opening the app does the rest over the connection to your own server. The relay writes no token and no message to any log.

The relay runs on Cloudflare. As with any web service, the host sees the network address and the timing of each request; the relay does not store them.

If the relay cannot be reached, your server works as before and notifications fall back to Web Push and to what the app shows when you open it.

## Deleting what the relay holds

- Remove the server from the app, or remove the phone from the server's settings: the app signs an unregister request and the relay deletes the row at once.
- Uninstall the app: the next wake-up finds the token invalid and the relay deletes the row then.
- If the app is gone and the phone is lost, nothing else identifies the row: the relay keeps no name or address, so it cannot be looked up for you. Apple or Google rejecting the token deletes it the first time it is used.
