# Security policy

## Reporting a vulnerability

Please report security problems privately through GitHub security advisories:
https://github.com/termenbox/termenbox/security/advisories/new

Do not open a public issue for a vulnerability. You will get an acknowledgement within 7 days. We aim to fix
confirmed issues within 90 days and publish an advisory when the fix ships; please keep the report private until then.

## Scope

In scope: `termenbox-server`, the hosted client and its Pages Functions (`app.termenbox.dev`), the installer
(`/i`) and the published container images.

Out of scope: findings that need a rooted device or an already-compromised server owner account, denial of service by
sheer traffic volume, and issues in third-party services (GitHub, Cloudflare).

## What we protect

A server's owner can open a shell on their own VPS, so the main assets are the owner's GitHub identity, the signed
directory records, and the release pipeline. See `docs/facts/0023-threat-model.md` for the threat model and
`docs/decisions/0062` to `0067` for the planned hardening.

## Release signing (native binaries)

Each release (`v<version>` tag, `.github/workflows/release.yaml`) publishes the static binaries and a
`manifest.json` that lists their SHA-256 hashes. The manifest is signed with an Ed25519 **release key** in the
OpenSSH signature format (namespace `termenbox-release`), so anyone can check it with
`ssh-keygen -Y verify`, and the server checks it with the public keys built into the binary
(`packages/server/release-keys.txt`, or `TERMENBOX_RELEASE_KEYS` for a fork that publishes its own releases).
Hosts never trust the download location: the installer and the updater verify the signature, then the hash.

- **Where the key lives:** the private key is the GitHub Actions secret `RELEASE_SIGNING_KEY` (an OpenSSH private
  key). It is written to a temporary file for the signing step and deleted. A tag build without the secret fails
  instead of publishing an unsigned release.
- **Create or rotate it:** `ssh-keygen -t ed25519 -N '' -C 'termenbox-release r2' -f release_key`, add the public
  line as `r2 ssh-ed25519 AAAA...` to `packages/server/release-keys.txt` and to the installer, ship that in a
  release signed by the old key, then set the secret to the new private key. Remove the old line one release later.
  If the key leaks, rotate at once and publish a signed `freeze` (below) so hosts stop applying releases signed
  by the leaked key.
- **Freeze:** a manifest with `"freeze": true` tells every updater to stop; it is signed like any manifest.
- **What a manifest may contain:** only download URLs under this project's GitHub releases; anything else is
  refused even when correctly signed. The installer and the updater download exactly the addresses the verified
  manifest names (decision 0132), never one built from the release address or a tag.
- **Channels:** a manifest names its channel (`stable`, `beta`, `nightly`) and each channel has its own key, listed
  with `channel=...` in `release-keys.txt` (decision 0130). A server follows only the channels in its own
  `update.toml` policy (decision 0131); a key signs only its channel.
