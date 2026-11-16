# Troubleshooting

What goes wrong most often, why, and what to do. Each entry says how the fix is checked. Back to the [README](../../README.md). Related: [Install](../start/install.md), [How it connects](../features/connecting.md), [Concepts](../concepts.md).

## Connecting

### The app says "Server unreachable on UDP port N"

**Cause.** A firewall is blocking that one UDP port. The installer opens it in `ufw` or `firewalld` when they are active, but a provider's firewall or security group (AWS, GCP, Oracle, Hetzner) is outside its reach.

**Fix.** Open the port named in the message on the machine and in the provider's security group. To read the port: `cat /opt/termenbox/udp_port` for the isolated container, or `sudo cat /var/lib/termenbox-root/udp_port` for an install on the machine. See [Firewalls](../start/install.md#firewalls).

Verified by: the end-to-end test `failures`, which shows each Add Server failure on one phone screen with one primary action and its detail behind a fold, and the unit test "is unreachable with the bound port when the transport throws Unreachable", which names the port the server is bound to.

### The app cannot connect from a mobile or office network

**Cause.** Some mobile and corporate networks block UDP, so the direct connection cannot be made.

**Fix.** Join the same Wi-Fi as the server, or let a tunnel carry the connection. A server starts a temporary address by itself when nothing else reaches it; for a permanent one, open Settings, then Tunnels, and set up your own. See [Tunnels](../features/connecting.md#tunnels-reaching-a-server-from-anywhere).

Verified by: the end-to-end test `tunnel`, in which a server only a tunnel reaches is paired once, then reached through the tunnel alone, and reconnects by itself after the hostname changes.

## Installing

### The installer ends by printing a command for root

**Cause.** An install for your own user needs no root, but starting at boot needs lingering, and a firewall rule needs root. When your system refuses, the installer says so and prints the one command root would have to run. It never runs it itself.

**Fix.** Run the printed command once with `sudo`. For lingering you can also run `loginctl enable-linger` as yourself. See [Where your agents run](../start/install.md#where-your-agents-run).

Verified by: the command `loginctl show-user "$USER" -p Linger`, which prints `Linger=yes` once lingering is on.

### An update does not install, or seems stuck

**Cause.** The update policy may be set to ask first (`notify`) or to do nothing (`hold`). A release whose server did not come up healthy is rolled back and not tried again until a newer release appears. An update never crosses a major version by itself.

**Fix.** Run `termenbox-server --root-helper status` to see the version, the policy, the last check and why it failed. `apply` installs the release now, `skip` stops offering it, and `postpone` adds one more grace period. See [Updating and removing](../start/install.md#updating-and-removing).

Verified by: the server tests `an_unhealthy_new_server_is_rolled_back` and `a_rolled_back_version_is_not_retried`, and the command `termenbox-server --root-helper status`.

## After it worked

### The server stopped without a clean exit

**Cause.** The server died without a clean exit. The next time you sign in, the app says so once.

**Fix.** Choose **Server diagnostics** to read what the server collected, with addresses, ports, tokens and terminal content removed. You can download it or open a prefilled issue. Nothing is sent until you tap. See [Logs and diagnostics](../start/install.md#logs-and-diagnostics).

Verified by: the unit test "announces an unclean exit once per at_ms and again for a newer one".

### A workspace is marked dormant

**Cause.** The tmux server on the machine was stopped, for example by a restart, so the workspace record is still there but nothing is running in it.

**Fix.** Open the workspace. Its tabs are recreated as shells with the same names and order; the work that was running in them is gone. See [Workspaces and tabs](../features/workspaces.md#workspaces-and-tabs).

Verified by: the server test `a_migrated_tmux_session_keeps_its_windows_as_tabs_and_comes_back_as_shells_when_dormant` and the recorded case `resume-after-close`.

## Still stuck?

Report it from the [issue forms](https://github.com/termenbox/termenbox/issues/new/choose) and include what **Server diagnostics** shows. To report a security problem, follow [SECURITY.md](../security/reporting.md) and do not post it publicly.
