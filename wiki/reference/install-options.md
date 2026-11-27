# Install options

Where the installer can run your sessions, which release channel it follows, and how to check and remove an install. Back to the [README](../../README.md). Related: [Install, firewall and where workspaces run](../start/install.md), [Configuration](./configuration.md).

- `curl -fsSL https://github.com/termenbox/termenbox/releases/latest/download/install-verified.sh | sh`: on this machine as the user who runs the command, as a user service; no root (the default).
- `curl -fsSL https://github.com/termenbox/termenbox/releases/latest/download/install-verified.sh | sh -s -- container`: in an isolated Docker container; Docker is installed if missing.
- `curl -fsSL https://github.com/termenbox/termenbox/releases/latest/download/install-verified.sh | sh -s -- host`: on this machine as root, as a system service; asks for `sudo`.
- `curl -fsSL https://github.com/termenbox/termenbox/releases/latest/download/install-verified.sh | sh -s -- host-user`: on this machine as a dedicated unprivileged user `termenbox`, as a system service; asks for `sudo`.

Release channel: add `--channel stable|beta|nightly` (default `stable`), for example `curl -fsSL https://github.com/termenbox/termenbox/releases/latest/download/install-verified.sh | sh -s -- --channel beta`.

Every install command above checks the installer, the bundle and its signature against a key published in this repository before it runs anything.

Uninstall (without `--yes` it only shows what it would do): `curl -fsSL https://github.com/termenbox/termenbox/releases/latest/download/uninstall-verified.sh | sudo sh -s -- --yes`.
