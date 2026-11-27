# Is it safe to use?

Back to the [README](../../README.md).

What a stranger on the internet can and cannot do, and what you accept by installing:

- **Only you can open a workspace.** The first GitHub account to connect owns the server; everyone else is refused. Your browser holds a device key that cannot be exported, and the server checks a certificate for it on every connection.
- **Your terminal traffic never touches our servers.** It goes from your browser to your machine, encrypted. The free directory only maps your server's public key to its address.
- **Your GitHub token is not kept.** It is used once to sign the certificate and dropped.
- **Host install = a shell on your machine, for your browser.** Whoever controls your GitHub account and an enrolled browser can run commands as the user the workspaces run as. That is the product; choose the narrowest option you can live with: the isolated container, then a dedicated `termenbox` user, then your own user, then root. Turn on two-factor authentication on GitHub.
- **Nothing is installed silently.** The installer lists every change first, records each in a journal before making it, and can undo all of them. Binaries are checked against a signature by the release key before they run, on install and on every update; a signed freeze can stop updates.
- **Nothing else runs as root.** The server runs as an unprivileged user; root work (updates, removal) is done by a separate small helper that accepts only a fixed list of requests.
- **No shell without something you hold.** Nothing we run, and no flaw reachable without signing in, is meant to give anyone a shell on your machine unless they also have a factor you control: your GitHub account, an enrolled browser, or your host. That is a design rule checked edge by edge in an attack-surface map that lists every edge and whether it can reach a shell, not a promise that no bug exists. A flood of plain traffic can slow a server down and is not covered; a request that makes us or you spend far more than the sender is, and we want to hear about it.
- **The licence is a free-use agreement, not open source.** You may run Termenbox for any purpose, including commercial, at no cost. You may not redistribute it as a product or reverse engineer it except where law allows, and it comes with no warranty. The terms are in [LICENSE](../../LICENSE); the licences of the third-party components are in [NOTICES](../../NOTICES). To report a vulnerability, write to the address in [SECURITY.md](./reporting.md).
- **What we cannot protect against:** a compromised machine, a stolen unlocked phone with an enrolled browser (remove the device in the app), or a malicious agent you run in a workspace.

Full model and how to report a problem: [SECURITY.md](./reporting.md).
