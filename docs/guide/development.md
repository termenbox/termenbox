# Development

Back to the [README](../../README.md).

```bash
cargo test --workspace                 # needs tmux for the end-to-end tests
cd packages/client && npx pnpm@9 install && ./node_modules/.bin/vitest run
# browser end-to-end (needs a built server, `pnpm build`, `pnpm e2e:build` and Chrome)
node e2e/run.mjs && node e2e/touch.mjs && node e2e/cards.mjs && node e2e/windows.mjs && node e2e/two-servers.mjs
```

No compiler on the machine? `scripts/dev.sh` runs any command in a pinned build container (Rust, tmux, and with
`--full` Node, pnpm and Chromium). The repo is mounted; cargo's registry and `target/`, and pnpm's store, live in a
named Docker volume, so repeat builds are incremental (`docker compose -f docker/docker-compose.dev.yml down -v`
clears them):

```bash
scripts/dev.sh cargo test --workspace
scripts/dev.sh --full sh -c 'pnpm install --frozen-lockfile && pnpm --filter termenbox-client e2e:build && cd packages/client && node e2e/run.mjs'
```

All builds and image publishing run in CI (`.github/workflows/`). Layout:

```
packages/core     shared protocol types (Rust)
packages/server   termenbox-server: auth, sessions, WebRTC + WebSocket transports, directory announcer
packages/client   Svelte 5 + xterm.js client, Pages Functions (OAuth exchange, signed directory)
packages/agent    parked (agent detection moves into the server, issue #19)
docker/           image (built by CI), dev build container, sample compose file
docs/             facts, decisions, guardrails, plans
```

The plan and its status live in GitHub issues; see `docs/plans/termenbox-roadmap.md`.

## Roadmap

Mobile UX, the installable PWA, agent notifications, end-to-end tests and resource gates are done. What comes next is ordered in `docs/plans/termenbox-roadmap.md`.
