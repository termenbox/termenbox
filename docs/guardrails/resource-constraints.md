---
id: resource-constraints
title: Resource Constraints (512MB RAM, 1 vCPU)
status: active
governed-by: 0001
grounded-in: 0001
derivation-note: Target deployment fact directly defines resource limits; amended by 0137 to cover termenbox's own processes, not agent children
tags: [deployment, performance, limits]
---

# Guardrail: Resource Constraints (512MB RAM, 1 vCPU)

## Rule
**MUST** operate within 512 MB RSS and 1 vCPU on the server. **MUST NOT** introduce dependencies or patterns that exceed this envelope. The envelope covers termenbox's own processes; agent children the owner has termenbox start (ACP adapters, a few hundred MB each) are sized by the box, not by this rule (decision 0137).

## Limits
| Resource | Limit | Measurement |
|---|---|---|
| Server RSS | ≤ 300 MB, termenbox's own processes only (agent children excluded) | `ps -o rss= -p $PID` |
| Agent helper RSS | ≤ 50 MB, termenbox's own agent helpers (hooks, `termenbox-agent`); ACP adapter children excluded | same |
| Hook RSS | ≤ 20 MB | same |
| CPU (steady) | ≤ 50% of 1 vCPU | `top -p $PID` |
| CPU (burst) | ≤ 100% for < 5s | same |
| Disk (binaries) | ≤ 50 MB total | `du -sh /usr/local/bin/termenbox-*` |
| Disk (data) | ≤ 100 MB (session state) | `du -sh ~/.termenbox/` |

## Performance Budgets
- **Connection handshake**: < 100ms (WebTransport 0-RTT), < 300ms (WebSocket)
- **Keystroke latency**: < 50ms p99 (local echo), < 150ms p99 (server echo)
- **Delta frame encode**: < 1ms for 1000-line diff
- **WASM init**: < 500ms (instantiate + compile)
- **Client bundle**: < 2 MB gzipped (WASM + JS + xterm.js)

## Prohibited Patterns
- In-memory databases (Redis, SQLite in-memory) for session state — use Rope + disk checkpoint
- Unbounded caches — all caches have TTL and max entries
- Thread-per-connection — use tokio async (single-threaded runtime acceptable)
- Large dependency trees — `cargo bloat` and `webpack-bundle-analyzer` in CI

## Enforcement
- CI: `cargo bloat --release --time 10` — top 20 functions by size
- CI: `cargo check --release 2>&1 | grep "warning: unused" | wc -l` — zero unused deps
- Release: `docker run --memory=512m --cpus=1 termenbox-server --bench`
- Monitoring: Server exports Prometheus metrics (`rss_bytes`, `cpu_seconds_total`)