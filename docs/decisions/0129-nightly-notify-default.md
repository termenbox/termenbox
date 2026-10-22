# DECISION-0129: Nightly notify-only default; server channel via installer

**Date:** 2026-10-22
**Status:** accepted

## Context

The server's daily updater timer runs automatically. For nightly builds (daily, unreviewed), auto-update is risky. The owner also wants the server's channel to be a host-level setting, not a web UI toggle.

## Decision

1. **Nightly default update mode = `notify`.** The daily timer checks for updates and records the available version in the server status, but does not install. The owner must explicitly run `termenbox-server --root-helper update` (via web UI "Update now" button or CLI) to apply. Auto-update for nightly requires explicit opt-in: `mode = "auto"` in server config or `--auto-update` flag at install.

2. **Beta default = `auto`** (same as stable). Beta is cumulative with stable; health check + rollback protects against bad releases.

3. **Server channel is set at install time.** The installer accepts `--channel stable|beta|nightly` and writes it to a root-owned config file:
   - System install: `/etc/termenbox/update.conf`
   - User install: `~/.config/termenbox/update.conf` (or equivalent)

   The systemd unit (system or user) gets `Environment=TERMENBOX_RELEASE_URL=<channel-specific-url>` so the helper picks the correct channel.

4. **Changing channel later is a host-level operation:**
   - Re-run installer with new `--channel` (idempotent, updates config + systemd env).
   - Or edit config file manually + `systemctl reload termenbox-update.timer`.
   - **Not** exposed in the web UI in v1. This prevents a compromised owner session from silently moving a server to nightly auto-update.

5. **Config schema:**
   ```toml
   [update]
   channel = "stable"   # stable | beta | nightly
   mode = "auto"        # auto | notify | hold
   maintenance_window = "03:00-04:00"  # optional, local time
   ```

## Consequences

- **Positive:** Nightly servers don't auto-restart daily; channel is a deliberate host decision; installer remains the single source of truth for channel config; Docker channel = image tag (no config needed).
- **Negative:** Less convenient for users who want to switch channels from the app; Docker auto-update not addressed in v1 (manual `docker pull` + recreate).
- **Migration:** Installer updated to write config; `helper.rs` reads `TERMENBOX_RELEASE_URL` from env (set by systemd); existing installs default to stable (backward compatible).

## Related

- DECISION-0125 (release channels)
- DECISION-0076 (updater), DECISION-0077 (root helper)
- R70 (implementation plan)
- `packages/client/public/install.sh`, `packages/server/src/helper.rs`, `packages/server/src/updater.rs`