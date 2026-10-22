# GUARDRAIL-nightly-notify-default

**Name:** Nightly channel defaults to notify-only update mode
**Type:** blocking
**Scope:** packages/client/public/install.sh, packages/server/src/helper.rs, packages/server/src/updater.rs
**Detection:** installer default check / config validation test

## Rule

The nightly channel's default update mode is `notify`, not `auto`. The installer writes `mode = "notify"` to the server config when `--channel nightly` is used. The systemd unit for nightly does not enable auto-update unless the user explicitly passes `--auto-update` at install time or edits the config later.

## Behaviour

- `mode = "notify"`: daily timer runs the updater, which checks for a newer nightly manifest. If found, it records the available version in the server status (exposed via API) and exits without installing. The owner sees "Nightly 0.1.36-nightly.20261022 available" in the web UI and can trigger installation via "Update now" button (sends `Request::UpdateNow`).
- `mode = "auto"`: same as stable/beta — installs immediately after health check.
- `mode = "hold"`: timer does nothing.

## Enforcement

- `install.sh`: `--channel nightly` sets `UPDATE_MODE="notify"` unless `--auto-update` is also given.
- `helper.rs`: no special logic; reads `TERMENBOX_RELEASE_URL` from env (set by systemd unit). The config file is read by the server at startup and exposed via API; the web UI shows the mode.
- `updater.rs`: checks a `mode` field in the server config (future) or relies on the timer not being enabled for auto. For v1, the timer is always enabled; the mode is enforced by the installer not writing the auto-update env for nightly. More robust enforcement (updater reads config) is a follow-up.

## Rationale

Nightly builds are daily, unreviewed, and may break protocols, sessions, or tmux behaviour. Auto-updating them daily would cause unexpected restarts and potential instability on servers where the owner just wanted to "try nightly". Notify-only puts the owner in control.

## Related

- DECISION-0129 (nightly notify default)
- DECISION-0125 (release channels)
- `packages/client/public/install.sh`