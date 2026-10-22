# Is it safe to use?

Back to the [README](../../README.md).

What a stranger on the internet can and cannot do, and what you accept by installing:

- **Only you can open a session.** The first GitHub account to connect owns the server; everyone else is refused. Your browser holds a device key that cannot be exported, and the server checks a certificate for it on every connection.
- **Your terminal traffic never touches our servers.** It goes from your browser to your machine, encrypted. The free directory only maps your server's public key to its address.
- **Your GitHub token is not kept.** It is used once to sign the certificate and dropped.
- **Host install = a shell on your machine, for your browser.** Whoever controls your GitHub account and an enrolled browser can run commands as the user the sessions run as. That is the product; choose the narrowest option you can live with: the isolated container, then a dedicated `termenbox` user, then your own user, then root. Turn on two-factor authentication on GitHub.
- **Nothing is installed silently.** The installer lists every change first, records each in a journal before making it, and can undo all of them. Binaries are checked against a signature by the release key before they run, on install and on every update; a signed freeze can stop updates.
- **Nothing else runs as root.** The server runs as an unprivileged user; root work (updates, removal) is done by a separate small helper that accepts only a fixed list of requests.
- **What we cannot protect against:** a compromised machine, a stolen unlocked phone with an enrolled browser (remove the device in the app), or a malicious agent you run in a session.

Full model and how to report a problem: [SECURITY.md](../../SECURITY.md).
