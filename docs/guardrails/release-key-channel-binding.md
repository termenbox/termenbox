# GUARDRAIL-release-key-channel-binding

**Name:** Release keys are bound to channels; binaries enforce trust sets
**Type:** blocking
**Scope:** packages/server/src/release.rs, packages/server/src/updater.rs, packages/server/release-keys.txt, .github/workflows/release-*.yaml
**Detection:** static analysis / unit tests / CI gate

## Rule

Every release signing key in `release-keys.txt` must declare its allowed channel(s) in a 4th field: `channel=stable|beta|nightly` (comma-separated for multiple). The updater must verify that the manifest's `channel` field matches one of the key's allowed channels. Binaries built for a channel only trust keys that allow that channel.

## Trust sets

| Binary channel | Allowed key channels |
|---|---|
| stable | stable |
| beta | stable, beta |
| nightly | nightly |

## Enforcement

- `ReleaseKeys::parse` accepts 3 or 4 fields per line. Missing 4th field defaults to `stable` (backward compat for old `r1`).
- `Manifest` struct has required `channel` field.
- `Manifest::verify` returns `(Manifest, kid)`.
- Updater calls `keys.allows_channel(kid, manifest.channel)` before accepting the manifest.
- Installer (`install.sh`) embeds the channel-appropriate key list (stable: only stable keys; beta: stable+beta; nightly: only nightly).

## Rationale

Nightly workflow runs on `develop` and has access to the nightly signing key. Without channel binding, a compromised nightly workflow could sign a manifest with `channel="stable"` and the stable updater would accept it (same key). Channel binding limits the blast radius: nightly key can only sign nightly manifests.

## Exceptions

- Old stable binaries (pre-channel) only know the stable key and will reject beta/nightly signatures. This is safe.
- Beta alias (`channel-beta`) may point to a stable release. The stable manifest has `channel="stable"` signed by stable key. Beta binary trusts stable key → accepts. Correct.

## Tests required

- `release_keys_parse_channel_field`
- `manifest_verify_returns_kid`
- `updater_rejects_cross_channel_signature` (nightly key + stable manifest)
- `updater_accepts_beta_alias_to_stable`
- `installer_embeds_correct_keys_per_channel`

## Related

- DECISION-0125 (release channels)
- GUARDRAIL-releases-are-signed
- GUARDRAIL-updates-are-verified-and-reversible