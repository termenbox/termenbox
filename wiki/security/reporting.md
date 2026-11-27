# Security policy

## Reporting a vulnerability

Please report security problems by email to **security@termenbox.dev**.

Do not open a public issue for a vulnerability. The source repository is private and cannot receive GitHub advisories.
Include the version (`termenbox-server --version`), what you did and what happened.

What you can expect from us:

- We aim to acknowledge your report within 7 days.
- We aim to fix a confirmed issue within 90 days, and to publish an advisory when the fix ships in a signed release.
- Credit in the advisory if you want it.
- No legal action against good-faith research that stays inside the scope below, touches only your own server and
  accounts, and keeps the report private until the fix ships.

Please keep the report private until then. Reports are tracked in a private review record; details of an open
finding are never published before its fix is released.

## Scope

In scope: `termenbox-server`, the hosted client and its Pages Functions (`app.termenbox.dev`), the installer
(`/i`) and the published container images.

Out of scope: findings that need a rooted device or an already-compromised server owner account, a pure volumetric
flood (denial of service by sheer traffic volume, which we cannot out-spend), and issues in third-party services
(GitHub, Cloudflare).

In scope, and wanted: cheap amplification, a request that costs the project or an owner far more (Workers or D1
operations, memory, disk, relayed bytes) than it costs the sender. The invariant we hold ourselves to: nothing the
project operates, and no bug reachable without signing in, gives anyone a shell on a user's machine without a factor
the user holds. We keep an attack-surface map that lists every edge and its answer.

## What we protect

A server's owner can open a shell on their own VPS, so the main assets are the owner's GitHub identity, the signed
directory records, and the release pipeline.

## Release signing (native binaries)

Each release (`v<version>` tag, `.github/workflows/release-stable.yaml`, built by `release-build.yaml`) publishes the static binaries and a
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
  Publish one with the workflow `release-freeze.yaml` (Actions, Run workflow): pick the channel, give a version newer
  than the one the channel holds, and tick `publish` (unticked it is a dry run with a throwaway key). It signs in that
  channel's Environment with that channel's key, writes it to the channel's pointer on the download host first (stable:
  also the beta pointer, forward only) and then moves the channel's mirror alias (stable: a release of its own marked
  latest, plus `channel-beta`). The run fails if the channel's own pointer did not take the freeze. The release workflows then refuse a version that does not replace the freeze, so the
  freeze is lifted by releasing a version newer than it.
- **Client bundle and installer signature:** the list of every file of a channel's client, the installer served at
  `/i` included, is `bundle.json` (SHA-256 per file). A key the owner holds outside GitHub and Cloudflare signs it
  (`node scripts/bundle-sign.mjs sign DIST KEYFILE`, namespace `termenbox-bundle`); its public half is a line in
  `packages/client/bundle-keys.txt` (`kid ssh-ed25519 AAAA... channel=stable|beta|nightly`). While a channel has a
  line there, its release builds the client and stages it under the release (`client-bundle.tar`, `bundle.json`)
  instead of deploying and prints a tree hash of it in the run's log; the offline signing step (`scripts/sign-staged-bundle.sh <tag>`) shows the hash of what it downloaded and signs only after the last 8 characters of the CI log's hash are typed (that hash comes from the same CI run that built the bundle, so the check guards against changes after CI, such as a swapped release asset, and not against a compromised build); after the owner uploads `bundle.json.sig` to that release, `deploy-pages.yaml` run with the
  tag verifies the files against the signature and deploys exactly those bytes. The installer of that channel carries
  the key and, before changing anything, checks that `/bundle.json` is signed by it, names its channel, is not older than
  the lowest version baked into it (`packages/client/bundle-min-version.txt`) and lists the installer being run. Give it a fingerprint obtained from somewhere other than the address it is served from with
  `TERMENBOX_BUNDLE_FP=SHA256:...` (`node scripts/bundle-sign.mjs fingerprint packages/client/bundle-keys.txt stable`)
  and only a baked key with that fingerprint counts. A self-hosted client does the same with its own key file.
- **Verified install command:** to verify the installer against a key that is published in this repository, not on the
  site that serves it, run
  `curl -fsSL https://github.com/termenbox/termenbox/releases/latest/download/install-verified.sh | sh` (a mode or channel is passed after `-s --`).
  `scripts/install-verified.sh` carries the stable keys of `packages/client/bundle-keys.txt` (a test fails when the copy
  differs), downloads `/i`, `bundle.json` and `bundle.json.sig` into a private directory, checks the signature, the
  channel, that the bundle version is not below the script's `MIN_BUNDLE_VERSION` (a copy of
  `packages/client/bundle-min-version.txt`, raised together with it by `scripts/installer-floor.mjs bump` and never lowered) and the SHA-256 of `/i`, and runs
  that same file. It names the file in `TERMENBOX_VERIFIED_FILE`; the installer skips its own download and check only
  when that value is the installer's own path, and its `sudo` re-run executes that file too. Setting the variable by
  hand only skips the check on your own machine, for the file you run. The installer is fetched once on this route.
  `scripts/uninstall-verified.sh` does the same for `uninstall.sh` (the SHA-256 listed for it), and the installer prints
  that form as the way to remove Termenbox. Fingerprints: `bk1 SHA256:EcN+IBMm8lQWCl2JxgNA4ebTftzlxK2u3bp6P3PYO9g`,
  `bk2 SHA256:lfmv+huP625NbSM4qgP3na8YgOr+2dfK6cSxei0Gak4`.
- **What a manifest may contain:** only download URLs under this project's GitHub releases; anything else is
  refused even when correctly signed. The installer and the updater download exactly the addresses the verified
  manifest names, never one built from the release address or a tag.
- **Per-channel pipelines:** `release-stable.yaml` (tags `vX.Y.Z` on `master`), `release-beta.yaml`
  (`vX.Y.Z-rc.N`, `-beta.N`) and `release-nightly.yaml` (the head of a green CI run on `develop`, once a day) call
  `release-build.yaml`, which signs in the GitHub Environment `stable`, `beta` or `nightly`. Each environment holds
  its own `RELEASE_SIGNING_KEY`; the signature must verify against that channel's key (`r1`, `b1`, `n1`) in
  `release-keys.txt` or nothing is published.
- **Channels:** a manifest names its channel (`stable`, `beta`, `nightly`) and each channel has its own key, listed
  with `channel=...` in `release-keys.txt`. A server follows only the channels in its own
  `update.toml` policy; a key signs only its channel.
