---
id: 0058
title: Cloudflare STUN is free and unlimited, Cloudflare TURN is free to 1,000 GB a month, and Google publishes no STUN terms
status: active
kind: observation
governed-by: 0140-assisted-connection-runs-on-the-owners-cloudflare-account
tags: [network, cloudflare, limits, transport]
date: 2026-10-26
last-verified: 2026-10-26
---

# Free STUN and TURN terms, checked 2026-10-26

Read from Cloudflare's documentation and a web search; nothing here was run against the services.

## What is true

- **Cloudflare STUN** (`stun.cloudflare.com:3478`): Cloudflare's TURN FAQ describes it as free and unlimited. No account is needed.
- **Cloudflare Realtime TURN**: $0.05 per GB of egress with 1,000 GB free each month; the free allowance is shared with the SFU product. Billing counts traffic from Cloudflare's edge to the client. ([pricing](https://developers.cloudflare.com/realtime/pricing/), [FAQ](https://developers.cloudflare.com/realtime/turn/faq/index.md))
- **Credentials**: a TURN key is a long-term secret that is never given to a browser. The owner's backend calls `POST https://rtc.live.cloudflare.com/v1/turn/keys/<KEY_ID>/credentials/generate-ice-servers` with the key's bearer token and a body such as `{"ttl": 86400}`; the response's `iceServers` goes to the peer. ([docs](https://developers.cloudflare.com/calls/turn/generate-credentials))
- **Google STUN** (`stun.l.google.com:19302`, also 19305): no current published terms or availability promise were found.

## Not verified

- The maximum credential TTL (a community reference says 48 hours; not in official docs found).
- Whether Cloudflare's STUN has a rate limit or acceptable-use rule for a self-hosted app.
- Whether the shared TURN/SFU allowance and its overage price apply to a free-plan account without a payment method (the call may fail instead of bill).
- The API token scopes `--setup-cloudflare` needs, and whether a Durable Object with a hibernating WebSocket fits the Workers Free plan; step 1.2 of the plan checks both.
- Any real-world punch or relay success rate (plan step 1.1).

## Consequences

- STUN can be a default, but only as a replaceable hint (0140).
- The server mints TURN credentials itself and caps relayed bytes, because Cloudflare gives no hard spend limit.
