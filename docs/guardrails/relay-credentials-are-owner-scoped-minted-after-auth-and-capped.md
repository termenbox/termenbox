---
id: relay-credentials-are-owner-scoped-minted-after-auth-and-capped
title: Relay and signaling resources belong to the owner's account, credentials are minted only after authentication, and relayed bytes are capped
status: active
governed-by: 0140-assisted-connection-runs-on-the-owners-cloudflare-account
grounded-in: [0140-assisted-connection-runs-on-the-owners-cloudflare-account, 0058]
derivation-note: Given 0140 (the relay and signaling live in the owner's Cloudflare account) and FACT-0058 (TURN overage bills the account and has no hard cap), an unauthenticated credential or a project key would turn a free tier into someone's bill.
tags: [security, network, cost, cloudflare, guardrail]
---

# Guardrail: Relay Credentials Are Owner-Scoped, Authenticated and Capped

## Rule

1. The project **MUST NOT** hold a TURN key, mint relay credentials, or run signaling for an owner's server. The directory, the R2 mailbox and the project's Workers quota are not used by the assisted path.
2. The server **MUST** mint a relay credential only for a client that proved a valid device certificate (or the one-time claim token on first contact), with a short TTL, and **MUST NOT** put a credential in a bundle, pairing link or log.
3. The server **MUST** stop minting once relayed bytes reach the monthly cap (default 500 GB) and report that in its status.
4. The setup API token **MUST** be read from stdin, used once and never stored or logged; only the TURN key id and its credential-minting token persist, in a `0600` file in the config directory.
5. The signaling Worker **MUST** hold no secret and **MUST** cap message size and per-source rate.

## Tests Required

| Test | Description |
|---|---|
| `no_credential_without_device_cert` | A signaling request with no or an invalid device certificate gets no credential. |
| `credential_never_in_bundle_or_logs` | A minted credential is absent from the signed bundle, pairing link, logs and status output. |
| `cap_stops_minting` | With the counter at the cap, a valid client gets a "relay unavailable" answer and no API call is made. |
| `setup_token_not_persisted` | After `--setup-cloudflare`, the config directory holds only the key id and token file, mode `0600`. |
| `worker_rejects_oversize_and_flood` | The Worker drops a message over the size cap and a source over the rate limit. |
