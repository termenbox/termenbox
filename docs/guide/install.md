# Install, firewall and where sessions run

Back to the [README](../../README.md).

The command is the same on every network and carries no secret:

```bash
curl -fsSL https://app.termenbox.dev/i | sh
```

It picks a free UDP port, starts the server and ends with a QR, a link and a short code. Scan the QR (or paste the link) in **Add Server**. It is safe to run again: on an owned server it prints a link that adds the server to another browser.

## Firewalls

The server needs **one inbound UDP port** (the installer picks a free one, usually 4443, and opens it in `ufw` or `firewalld` if they are active). If you use a provider firewall or security group (AWS, GCP, Oracle, Hetzner Cloud Firewall), open that port there. The app tells you which port when it cannot reach your server: *"Server unreachable on UDP port N"*. To see the port: `cat /opt/termenbox/udp_port` (isolated container) or `sudo cat /var/lib/termenbox-root/udp_port` (on the machine).

Some mobile and corporate networks block UDP. Those clients cannot connect until the tunnel fallback is available (see the roadmap).

## Where your agents run

Add Server offers these places (decisions 0058, 0075, 0078 and 0113). The bare command uses the first; the others add one word after `sh -s --`.

**In an isolated container** (`curl -fsSL https://app.termenbox.dev/i | sh -s -- container`). Sessions run inside a Docker container, in `/opt/termenbox/home` (mounted as the container's home, so it survives restarts and upgrades). Docker is installed if missing. The container cannot see the rest of your VPS. The image has tmux, git, curl and Node. Install your agents in a Termenbox session, for example:

```bash
npm install -g @anthropic-ai/claude-code @openai/codex
```

**On the machine itself** (no Docker; `… | sh -s -- host-user` for a dedicated user, `… | sh -s -- host` for root). The server is a single static binary run as a systemd service, and sessions run directly on your VPS, where your repositories and agent tools already are. You choose who the sessions run as: a dedicated unprivileged user `termenbox` (created for you, recommended), the user who runs the install command (sessions open on your behalf, in your home, with your repositories, keys and agent logins; nothing is created), or `root`. This gives your browser a shell on the server itself, so the app shows a warning and asks you to confirm. Requirements: systemd, `curl`, `ssh-keygen` (package `openssh-client`), x86_64 or aarch64. A pinned static `tmux` (3.2 or newer, decision 0119) is installed next to the server, so none has to be installed first; `TERMENBOX_TMUX` points the server at a tmux of your own instead.

**As your own user, with no root** (the default, `curl -fsSL https://app.termenbox.dev/i | sh`; decision 0113). The same machine-itself install, but as a user service of the account that runs the command: no `sudo`, no new user. The binary lives in `~/.local/bin`, state in `~/.config/termenbox`, and the update check is a timer of your own. Starting at boot needs lingering, which systemd lets you switch on for yourself (`loginctl enable-linger`); if your system refuses, the installer says so and prints the one command root would have to run. A firewall rule is printed the same way, never run. The pinned static `tmux` goes to `~/.local/bin` beside the server, so none has to be installed. The dedicated user and root choices still use a system service and ask for `sudo`.

The installer also sets up a daily update check that runs as root, separately from the server (which can stay an unprivileged user). Before anything is installed, and before every update, the release is checked: its manifest must carry a valid signature by the project's release key, and the binary must match the signed SHA-256 (see `SECURITY.md`). It lists every change it will make, records each one in a journal in `/var/lib/termenbox-root` before making it, and can undo them.

## Updating and removing

Remove the server from the machine with one command. Without `--yes` it only shows what it would do:

```bash
curl -fsSL https://app.termenbox.dev/uninstall.sh | sudo sh            # preview, changes nothing
curl -fsSL https://app.termenbox.dev/uninstall.sh | sudo sh -s -- --yes
```

An install made for your own user needs no `sudo` to remove: run the same command as that user, without `sudo`.

It handles both install kinds. For the container it removes the container and `/opt/termenbox`. On the machine itself it stops the service and the updater, deletes the server's state and undoes every journaled change (binary, units, firewall rule, and the `termenbox` user and tmux only if the installer added them). It never touches your repositories, or the home of the user your sessions run as. Add `--keep-state` to keep the server's keys and owner, `--delete-home` to also delete the home of a `termenbox` user the installer created. Running tmux sessions are left alone. Then use **Remove server** in the app to drop it from your list; you can also uninstall from there while connected.

Updates: the container updates when you run the install command again. On the machine itself a timer (root-owned, or your own for the no-root install) checks for a new signed release every day; if the new server does not come up healthy the old one is put back. Check now: `sudo termenbox-server --root-helper update`.

Release channels and update policy (decisions 0130-0132): `--channel stable|beta|nightly` picks the release stream (default stable), for example `curl -fsSL https://app.termenbox.dev/i | sh -s -- --channel beta`. A stable server never installs a beta or nightly; a beta server installs beta and stable releases; a nightly server installs only nightlies. The container uses the image tag of the channel (`latest` for stable, `beta`, `nightly`; `X.Y.Z` and the exact prerelease version pin a release). `latest` moves only when a stable `vX.Y.Z` tag is released, no longer with every change on the release branch, so a container on `latest` stays where it is until the first stable release made with the channel workflows. The choice is written to `update.toml` in the journal directory (`/var/lib/termenbox-root`, or `~/.local/state/termenbox` for the no-root install), which only the host can change; run the installer again with `--channel`, or edit the file:

```toml
[update]
channel = "stable"             # stable | beta | nightly
mode = "auto"                  # auto: install daily | notify: record it, install when asked | hold: do nothing
maintenance_window = "03:00-04:00"   # optional, local time; limits the automatic install
```

`auto` is the default, except on nightly, which defaults to `notify` (an unattended daily restart is a surprise on a server that wanted to try nightly); `--auto-update` turns it on. In `notify` the daily check records the available version in `update-status.json` in the server's state directory, and "update now" from the app installs it. `hold` does nothing, even when asked. An automatic update never crosses a major version: it is recorded, and installing it is your call. A file that does not parse stops the updater, so fix the typo and the check resumes.
