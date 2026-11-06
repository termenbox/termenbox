# Security policy

## Reporting a vulnerability

Use [private vulnerability reporting](https://github.com/termenbox/termenbox/security/advisories/new) on this
repository, or email **security@termenbox.dev**. Do not open a public issue or discussion for a vulnerability.

Include the version (`termenbox-server --version`), what you did and what happened.

What you can expect:

- We aim to acknowledge your report within 7 days.
- We aim to fix a confirmed issue within 90 days and to publish an advisory when the fix ships in a signed release.
- Credit in the advisory if you want it.
- No legal action against good-faith research that stays inside the scope below, touches only your own server and
  accounts, and keeps the report private until the fix ships.

## Scope

In scope: `termenbox-server`, the hosted client at `app.termenbox.dev`, the installer (`/i`) and the published
container images. Also wanted: cheap amplification, a request that costs the project or an owner far more than it
costs the sender.

Out of scope: findings that need a rooted device or an already-compromised server owner account, pure volumetric
floods, and issues in third-party services (GitHub, Cloudflare).

## Verifying downloads

Releases are signed. The installer and the updater check the signature and the file hashes before installing; they
never trust the download address alone.
