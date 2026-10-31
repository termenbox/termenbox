# Chapter 1: Install

In 3 minutes your server is running and showing a QR code.

You are at your desk, and the agent will work on this machine, so this is where Termenbox goes first. Sit at the machine that will run your agents: your own computer, or a server you reach over SSH. One command installs it, and when it finishes it prints a QR code and a link. Leave that window open; the next chapter needs it.

<!-- media:install-desktop -->
<!-- /media -->

<!-- course-steps:install -->
1. On the machine that will run your agents, open a terminal.
2. Paste the install command and press Enter.
3. Wait for the QR code and the link.
<!-- /course-steps -->

The command asks for no password and installs as you, with no `sudo`. It picks a free UDP port by itself and prints the firewall rule it needs instead of running it. It is safe to run twice: on a server that is already yours it just prints a fresh link.

## If it doesn't work

- **Your phone later says "Server unreachable on UDP port N".** A firewall is blocking that port. Open it on the machine, and in your provider's security group if you have one. [Firewalls](../guide/install.md#firewalls) shows how to read the port.
- **The installer ends by printing a command for root.** Without `sudo` it cannot start the server at boot or open the port itself. Run the printed command once with `sudo`. [Where your agents run](../guide/install.md#where-your-agents-run) explains the other choices.

## You can now

- See a QR code and a link in the terminal where you ran the command.

Next: [Chapter 2, Connect your phone](02-connect-your-phone.md).
