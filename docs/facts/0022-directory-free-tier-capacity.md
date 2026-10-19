---
id: 0022
title: Free-tier capacity model
status: active
tags: [directory, scale, cloudflare, capacity]
date: 2026-10-12
---

# Free-tier capacity model

## Limits (fetched from Cloudflare's docs on 2026-10-12)

| Service | Free limit |
|---|---|
| Workers and Pages Functions | 100k requests/day, shared (a Functions request counts against the Workers quota); 10 ms CPU, 50 subrequests per request; error 1027 when exceeded |
| Static assets | free and unlimited (a request that does not invoke Functions) |
| D1 | 5M rows read/day, 100k rows written/day, 5 GB |
| KV | 100k reads/day, **1,000 writes/day**, 1 GB (too few writes for this use) |
| R2 | 10 GB, 1M Class A (write) and 10M Class B (read) operations per month, free egress |

Not verified: whether R2 needs a payment method on file, Pages build limit (500/month recalled), GHCR and Actions being free for public repositories. The spike (plan step C.1) must confirm them.

## Today (`directory.rs`)

A claimed server announces on start, when its claim changes, and every 7 days (`KEEPALIVE`); an unclaimed one every 5 min until it is claimed. Its address, port and fingerprint are fixed for the process, so a change of any of them is a restart. That is about 1/7 request/day/server (10^5 servers: 1.4·10^4/day); before 2026-10-14 it was every 12 h, 2·10^5/day at 10^5 servers, over the limit. The binding limit is still Workers requests once reads, polls and user-data writes are added (about 10^4 servers with the other traffic until the CDN reads of R17).

## Target model (decisions 0060 and 0068)

Assumptions: 10^5 users, 1 server each, 2 devices each; 20% of users active per day with 2 app opens; 50% of devices active per week; 500 new servers/day with about 20 `pending` polls per Add Server; 3k user-data writes/day; announce on change plus a 7-day keepalive; 7-day device certificates.

| Service | Metric | Load | Limit | Used |
|---|---|---|---|---|
| Workers / Pages Functions | requests/day | 47k: certificate and login 14.3k, announces 17.8k, pending polls 10k, user-data writes 3k, account links 0.5k (one per Add Server, R08), other 1k | 100k | 47% |
| Workers | CPU per request | about 1–3 ms (Ed25519 or P-256 verify, HKDF) | 10 ms | 10–30% |
| Workers | subrequests per request | at most 2–3 | 50 | under 10% |
| R2 | Class A ops/month (writes) | 0.62M: record puts 17.8k/day, user-data puts 3k/day | 1M | 62% |
| R2 | Class B ops/month (reads) | 2.2M: user-data reads 40k/day, head before put 21k/day, claim-to-record reads 10k/day, record refreshes 3k/day | 10M | 22% |
| R2 | storage | records 70 MB, user data 400 MB | 10 GB | 5% |
| D1 (claims only) | rows read / written per day | about 12k / 2k | 5M / 100k | 0.2% / 2% |
| D1 | storage | under 10 MB | 5 GB | under 1% |
| Static assets / CDN | requests, egress | about 8k cold loads/day at 0.2 MB, plus public-bucket reads | unlimited | no cap |
| GitHub API | calls/day | 14k from our Workers; 10^5 updater checks from the servers' own IPs | 5k/h per token; 60/h per IP | far under |
| GHCR | pulls | about 500 installs/day at about 120 MB | free for public images (unverified) | n/a |
| GitHub Actions | minutes | PRs and the weekly base rebuild | unlimited for public repos (unverified) | n/a |
| Pages builds | per month | about 20 | 500 (unverified) | about 4% |
| Web Push (optional) | pushes/day | about 6k, subrequests inside write requests | not metered by us | n/a |
| WebRTC, UDP, tmux | all | runs on each user's VPS | no cost to us | n/a |

## Where it binds

Workers requests are at 46% (the guardrail allows 60%); they reach 60% at about 1.3·10^5 users and 100% at about 2.2·10^5. R2 Class A is at 62% and reaches 100% at about 1.6·10^5 users, so it is the real ceiling; it scales with announces. Everything else has more than 5x headroom.

## Levers

- Keepalive 7 to 14 days: halves announce load on Workers and R2 Class A.
- Certificate validity 7 to 30 days: certificate load from about 14k to about 3k requests/day.
- Drop the head-before-put on announces (use `seq` in the signed record): announce cost from 2 operations to 1.
- Move the high-volume writes (announces, claim polls, user data) off Cloudflare entirely: see decision 0068, alternatives under investigation.
