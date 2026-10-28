---
id: audit-logging-no-secrets
title: Audit Logging Without Secrets
status: active
date: 2026-10-09
governed-by: 0006,0009,0142
grounded-in: [0008, 0011]
derivation-note: SSH auth + crypto decisions require audit trail; secrets guardrail forbids logging secrets
tags: [security, audit, logging, observability]
---

# Guardrail: Audit Logging Without Secrets

## Rule
**MUST** log all security-relevant actions. **MUST NOT** log any secret material (keys, challenges, signatures, nonces, session keys). **MUST NOT** log ACP message text, terminal output or keystrokes at any level.

## Required Audit Events
| Event | Fields (Logged) | Fields (Never Logged) |
|---|---|---|
| Auth attempt | `timestamp, ip, key_fingerprint, success, failure_reason` | `challenge, signature, session_key` |
| Session create | `timestamp, session_id, key_fingerprint, transport` | `session_keys, nonce` |
| Rekey | `timestamp, session_id, trigger (bytes/time)` | `old_key, new_key` |
| Agent event | `timestamp, session_id, agent, event_type` | `agent_output (if sensitive)` |
| Config change | `timestamp, session_id, key_fingerprint, change` | `full_config` |

## Log Format
Structured JSON Lines (one object per line), written to `<config-dir>/logs/termenbox.jsonl` as well as stderr (0142):
```json
{
  "ts": "2026-10-09T12:34:56.789Z",
  "level": "info",
  "event": "auth.success",
  "session_id": "abc123",
  "key_fingerprint": "SHA256:...",
  "transport": "webtransport",
  "ip": "192.0.2.1"
}
```

## Log Retention
- Local (server): 30 days rolling, max 100 MB, rotated by the server itself
- No central log aggregation required (self-hosted)
- User controls retention via systemd `journald` config

## Enforcement
- CI: Log sanitizer test — inject secret into log call, verify redacted
- Code review: Any `log::info!`/`warn!`/`error!` with dynamic data must use structured fields
- `tracing` crate with `tracing-subscriber` JSON formatter; custom `Layer` redacts `secret` fields